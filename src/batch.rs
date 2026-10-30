//! All-or-nothing, persisted multi-entity scalar transactions.
//! Nothing reaches a writable handle until every target has been re-resolved and
//! the complete batch backup has been synced. One suspension spans final checks,
//! writes, readback and any necessary rollback across all targets.
pub use crate::model::BatchReceipt;
use crate::{backend, memory::Process, model::*, transaction};
use anyhow::{Context, Result, anyhow, ensure};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 200;
static SERIAL: AtomicU64 = AtomicU64::new(0);

fn history_dir() -> Result<PathBuf> {
    let dir = transaction::data_dir().join("batch-history");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
fn persist(receipt: &BatchReceipt) -> Result<()> {
    transaction::persist_json(
        &history_dir()?.join(format!("{}.json", receipt.id)),
        receipt,
    )
}
pub fn history() -> Vec<BatchReceipt> {
    let Ok(dir) = history_dir() else {
        return Vec::new();
    };
    let mut list: Vec<BatchReceipt> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|raw| serde_json::from_slice(&raw).ok())
        .collect();
    list.sort_by(|a, b| b.time.cmp(&a.time).then(b.id.cmp(&a.id)));
    list.truncate(500);
    list
}

fn validate_targets(items: &[(Detail, Vec<Edit>)]) -> Result<()> {
    ensure!(
        (1..=LIMIT).contains(&items.len()),
        "대량 편집 대상은 1–200개입니다"
    );
    let mut identities = HashSet::new();
    for (detail, _) in items {
        ensure!(
            identities.insert((detail.entity.kind, detail.entity.id)),
            "중복 대량 편집 대상: {}",
            detail.entity.name
        );
    }
    Ok(())
}

fn validated_changes(original: &Detail, fresh: &Detail, edits: &[Edit]) -> Result<Vec<Change>> {
    ensure!(
        fresh.entity.kind == original.entity.kind
            && fresh.entity.id == original.entity.id
            && fresh.guards == original.guards,
        "{}의 대상 정보가 달라졌습니다. 다시 불러오세요",
        original.entity.name
    );
    for edit in edits {
        let old = original
            .fields
            .iter()
            .find(|field| field.key == edit.key)
            .context("원본 편집 필드가 없습니다")?;
        let new = fresh
            .fields
            .iter()
            .find(|field| field.key == edit.key)
            .context("현재 편집 필드가 없습니다")?;
        ensure!(
            old.binding == new.binding,
            "{} · {}의 원본이 변경되었습니다. 다시 불러오세요",
            original.entity.name,
            old.label
        );
    }
    transaction::prepare(fresh, edits)
}

/// Produce one physical write per storage cell. Trait rows remain separate in
/// the journal/UI while sharing one merged 8-byte write. Different entities may
/// never edit overlapping bytes, including a shared stadium or contract.
fn physical_changes(items: &[Receipt]) -> Result<Vec<Change>> {
    let mut sorted = Vec::new();
    for (entity, item) in items.iter().enumerate() {
        ensure!(
            !item.changes.is_empty() && item.changes.len() <= 256,
            "대량 변경 항목 수 오류"
        );
        for change in &item.changes {
            ensure!(
                !change.old_bytes.is_empty()
                    && change.old_bytes.len() <= 8
                    && change.old_bytes.len() == change.new_bytes.len(),
                "대량 변경 저장 크기 오류"
            );
            ensure!(
                change.address >= 0x10000 && change.address < 0x7fff_ffff_fff0,
                "대량 변경 주소 오류"
            );
            sorted.push((entity, change));
        }
    }
    sorted.sort_by_key(|(entity, change)| (change.address, *entity));
    let mut writes: Vec<(usize, Change)> = Vec::new();
    for (entity, change) in sorted {
        if let Some((previous_entity, previous)) = writes.last() {
            if previous.address + previous.old_bytes.len() as u64 > change.address {
                ensure!(
                    *previous_entity == entity
                        && previous.address == change.address
                        && previous.old_bytes == change.old_bytes
                        && previous.new_bytes == change.new_bytes
                        && previous.old_bytes.len() == 8
                        && previous.key.starts_with("trait_")
                        && change.key.starts_with("trait_"),
                    "서로 다른 대상이나 필드의 저장 영역이 겹칩니다"
                );
                continue;
            }
        }
        writes.push((entity, change.clone()));
    }
    Ok(writes.into_iter().map(|(_, change)| change).collect())
}

/// Caller must hold the process suspended. All records are validated before
/// invoking the existing writer, whose rollback spans the entire physical list.
fn write_prepared(memory: &impl transaction::MemoryAccess, items: &[Receipt]) -> Result<()> {
    let changes = physical_changes(items)?;
    for item in items {
        transaction::validate_originals(memory, &item.guards, &item.changes)?;
    }
    transaction::write_batch(memory, &changes)
}

fn run_after_backup<T>(
    backup: impl FnOnce() -> Result<()>,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    backup().context("대량 변경 전 전체 백업을 저장하지 못했습니다. 적용하지 않았습니다")?;
    operation()
}

pub fn apply_many(session: &Session, items: &[(Detail, Vec<Edit>)]) -> Result<BatchReceipt> {
    crate::compatibility::require_scalar_write(session)?;
    validate_targets(items)?;
    backend::validate_session(session)?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    let id = format!(
        "{time}-{}-{}",
        session.pid,
        SERIAL.fetch_add(1, Ordering::Relaxed)
    );
    let mut records = Vec::with_capacity(items.len());
    for (n, (original, edits)) in items.iter().enumerate() {
        let fresh = backend::detail(session, &original.entity)?;
        let changes = validated_changes(original, &fresh, edits)?;
        let mut guards = fresh.guards.clone();
        guards.extend(transaction::dependency_guards(&fresh, &changes)?);
        records.push(Receipt {
            id: format!("{id}-{n}"),
            time: time.clone(),
            session: session.clone(),
            entity: fresh.entity,
            guards,
            changes,
            status: "prepared".into(),
        });
    }
    physical_changes(&records)?;
    let mut receipt = BatchReceipt {
        id,
        time,
        session: session.clone(),
        items: records,
        status: "prepared".into(),
    };
    let mut resume_warning = None;
    let result = run_after_backup(
        || persist(&receipt),
        || {
            let process = Process::open(session.pid, true)?;
            ensure!(
                process.created()? == session.created,
                "FM 프로세스가 교체되었습니다"
            );
            let suspended = process.suspend()?;
            let result = (|| {
                backend::validate_with(&process, session)?;
                write_prepared(&process, &receipt.items)
            })();
            if let Err(error) = suspended.resume() {
                resume_warning = Some(format!("FM 재개 확인 실패: {error:#}"));
            }
            result
        },
    );
    match result {
        Ok(()) => {
            receipt.status = resume_warning
                .map(|warning| format!("applied_resume_warning: {warning}"))
                .unwrap_or_else(|| "applied".into());
            for item in &mut receipt.items {
                item.status = receipt.status.clone();
            }
            if let Err(error) = persist(&receipt) {
                receipt.status = format!("{}_log_warning: {error}", receipt.status);
            }
            Ok(receipt)
        }
        Err(error) => {
            let error = if let Some(warning) = resume_warning {
                anyhow!("{error:#}; {warning}")
            } else {
                error
            };
            receipt.status = format!("failed: {error:#}");
            for item in &mut receipt.items {
                item.status = receipt.status.clone();
            }
            // A failed initial backup still leads to zero process writes; recording
            // failure is best effort and never triggers a retry of the operation.
            let _ = persist(&receipt);
            Err(error)
        }
    }
}

fn same_session(a: &Session, b: &Session) -> bool {
    a.pid == b.pid
        && a.created == b.created
        && a.module_base == b.module_base
        && a.module_size == b.module_size
        && a.module_stamp == b.module_stamp
        && a.db == b.db
        && a.registry_begin == b.registry_begin
        && a.profile == b.profile
}

/// A dependency guard may cover an edited CA/PA cell. For undo, require the
/// recorded post-apply bytes there while preserving every untouched guard byte.
fn guards_after(item: &Receipt) -> Result<Vec<Guard>> {
    let mut guards = item.guards.clone();
    for guard in &mut guards {
        ensure!(
            !guard.expected.is_empty() && guard.expected.len() <= 256,
            "대량 기록 검증 정보 오류"
        );
        for change in &item.changes {
            let begin = guard.address.max(change.address);
            let end = (guard.address + guard.expected.len() as u64)
                .min(change.address + change.new_bytes.len() as u64);
            if begin < end {
                let target = (begin - guard.address) as usize;
                let source = (begin - change.address) as usize;
                let len = (end - begin) as usize;
                guard.expected[target..target + len]
                    .copy_from_slice(&change.new_bytes[source..source + len]);
            }
        }
    }
    Ok(guards)
}

fn reverse_edits(item: &Receipt, fresh: &Detail) -> Result<Vec<Edit>> {
    let mut edits = Vec::new();
    for change in &item.changes {
        let binding = fresh
            .fields
            .iter()
            .find(|field| field.key == change.key)
            .and_then(|field| field.binding.as_ref())
            .context("기록의 편집 필드를 현재 찾을 수 없습니다")?;
        ensure!(
            binding.address == change.address && binding.original == change.new_bytes,
            "{} · {} 값이 적용 후 변경되어 전체 되돌리기를 중단했습니다",
            item.entity.name,
            change.label
        );
        edits.push(Edit {
            key: change.key.clone(),
            value: change.before,
        });
    }
    let reverse = transaction::prepare(fresh, &edits)?;
    for original in &item.changes {
        let restored = reverse
            .iter()
            .find(|change| change.key == original.key)
            .context("대량 기록 복원 항목 불일치")?;
        ensure!(
            restored.new_bytes == original.old_bytes && restored.old_bytes == original.new_bytes,
            "대량 기록의 복원 값과 원본 바이트가 일치하지 않습니다"
        );
    }
    Ok(edits)
}

pub fn undo_many(receipt: &BatchReceipt) -> Result<BatchReceipt> {
    ensure!(
        receipt.status.starts_with("applied"),
        "완료된 대량 적용만 되돌릴 수 있습니다"
    );
    ensure!(
        (1..=LIMIT).contains(&receipt.items.len()),
        "대량 기록의 대상 수 오류"
    );
    backend::validate_session(&receipt.session)?;
    physical_changes(&receipt.items)?;
    let process = Process::open(receipt.session.pid, false)?;
    ensure!(
        process.created()? == receipt.session.created,
        "FM 프로세스가 교체되었습니다"
    );
    let mut identities = HashSet::new();
    let mut targets = Vec::new();
    for item in &receipt.items {
        ensure!(
            same_session(&item.session, &receipt.session),
            "대량 기록의 세션이 일치하지 않습니다"
        );
        ensure!(
            identities.insert((item.entity.kind, item.entity.id)),
            "대량 기록에 중복 대상이 있습니다"
        );
        let fresh = backend::detail(&receipt.session, &item.entity)?;
        transaction::validate_originals(&process, &guards_after(item)?, &[])?;
        let edits = reverse_edits(item, &fresh)?;
        targets.push((fresh, edits));
    }
    let mut reversed = apply_many(&receipt.session, &targets)?;
    reversed.status = if reversed.status.contains("warning") {
        format!("undone_warning:{}; {}", receipt.id, reversed.status)
    } else {
        format!("undone:{}", receipt.id)
    };
    for item in &mut reversed.items {
        item.status = reversed.status.clone();
    }
    if let Err(error) = persist(&reversed) {
        reversed.status = format!("{}_log_warning: {error}", reversed.status);
    }
    Ok(reversed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };
    use transaction::MemoryAccess;
    struct Mock {
        data: RefCell<BTreeMap<u64, u8>>,
        writes: Cell<usize>,
        fail: Cell<Option<u64>>,
    }
    impl Mock {
        fn new() -> Self {
            Self {
                data: RefCell::new([(0x10000, 1), (0x20000, 2), (0x30000, 3), (0x40000, 4)].into()),
                writes: Cell::new(0),
                fail: Cell::new(None),
            }
        }
    }
    impl MemoryAccess for Mock {
        fn read(&self, a: u64, n: usize) -> Result<Vec<u8>> {
            (0..n)
                .map(|i| {
                    self.data
                        .borrow()
                        .get(&(a + i as u64))
                        .copied()
                        .context("unmapped")
                })
                .collect()
        }
        fn write(&self, a: u64, b: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            for (i, v) in b.iter().enumerate() {
                self.data.borrow_mut().insert(a + i as u64, *v);
            }
            if self.fail.get() == Some(a) {
                self.fail.set(None);
                anyhow::bail!("partial write");
            }
            Ok(())
        }
    }
    fn session() -> Session {
        Session {
            pid: 1,
            created: 2,
            module_base: 0x10000,
            module_size: 10,
            module_stamp: 20,
            db: 30,
            registry_begin: 40,
            game_date: "2027-11-26".into(),
            profile: "fixture".into(),
            own_club_id: None,
        }
    }
    fn item(uid: u32, a: u64, b: u8, n: u8) -> Receipt {
        Receipt {
            id: uid.to_string(),
            time: "1".into(),
            session: session(),
            entity: Entity {
                nationality: String::new(),
                nationality_id: None,
                football_category: crate::model::FootballCategory::Unknown,
                id: uid,
                kind: EntityKind::Player,
                name: uid.to_string(),
                english_name: String::new(),
                club: String::new(),
                club_id: None,
                age: None,
                ca: None,
                pa: None,
                role: String::new(),
                group: String::new(),
                address: 0x10000 + uid as u64,
                base: 0x10000,
                index: Some(uid),
            },
            guards: vec![Guard {
                address: 0x30000,
                expected: vec![3],
            }],
            changes: vec![Change {
                key: "passing".into(),
                label: "passing".into(),
                before: b as f64,
                after: n as f64,
                address: a,
                old_bytes: vec![b],
                new_bytes: vec![n],
            }],
            status: "prepared".into(),
        }
    }
    #[test]
    fn stale_later_entity_causes_zero_writes() {
        let m = Mock::new();
        let items = [item(1, 0x10000, 1, 8), item(2, 0x20000, 99, 9)];
        assert!(write_prepared(&m, &items).is_err());
        assert_eq!(m.writes.get(), 0);
        assert_eq!(m.read(0x10000, 1).unwrap(), vec![1]);
    }
    #[test]
    fn later_entity_partial_failure_rolls_back_whole_batch() {
        let m = Mock::new();
        m.fail.set(Some(0x20000));
        assert!(write_prepared(&m, &[item(1, 0x10000, 1, 8), item(2, 0x20000, 2, 9)]).is_err());
        assert_eq!(m.read(0x10000, 1).unwrap(), vec![1]);
        assert_eq!(m.read(0x20000, 1).unwrap(), vec![2]);
        assert_eq!(m.writes.get(), 4);
    }
    #[test]
    fn failed_complete_backup_never_reaches_writer() {
        let m = Mock::new();
        let items = [item(1, 0x10000, 1, 8), item(2, 0x20000, 2, 9)];
        let result = run_after_backup(|| anyhow::bail!("disk full"), || write_prepared(&m, &items));
        assert!(result.is_err());
        assert_eq!(m.writes.get(), 0);
    }
    #[test]
    fn cross_entity_shared_storage_is_rejected_before_write() {
        let m = Mock::new();
        assert!(write_prepared(&m, &[item(1, 0x10000, 1, 8), item(2, 0x10000, 1, 8)]).is_err());
        assert_eq!(m.writes.get(), 0);
    }
    #[test]
    fn one_entity_traits_share_exactly_one_physical_write() {
        let mut receipt = item(1, 0x10000, 1, 8);
        let first = &mut receipt.changes[0];
        first.key = "trait_1".into();
        first.old_bytes = 0u64.to_le_bytes().to_vec();
        first.new_bytes = 3u64.to_le_bytes().to_vec();
        let mut second = first.clone();
        second.key = "trait_2".into();
        receipt.changes.push(second);
        assert_eq!(physical_changes(&[receipt.clone()]).unwrap().len(), 1);
        receipt.changes[1].new_bytes = 4u64.to_le_bytes().to_vec();
        assert!(physical_changes(&[receipt]).is_err());
    }
    #[test]
    fn successful_batch_checks_every_original_then_writes_all() {
        let m = Mock::new();
        write_prepared(&m, &[item(1, 0x10000, 1, 8), item(2, 0x20000, 2, 9)]).unwrap();
        assert_eq!(m.read(0x10000, 1).unwrap(), vec![8]);
        assert_eq!(m.read(0x20000, 1).unwrap(), vec![9]);
        assert_eq!(m.writes.get(), 2);
    }
    #[test]
    fn undo_guards_use_changed_dependency_bytes_and_preserve_rest() {
        let mut receipt = item(1, 0x10000, 1, 8);
        receipt.guards = vec![Guard {
            address: 0x10000,
            expected: vec![1, 7, 6],
        }];
        let guards = guards_after(&receipt).unwrap();
        assert_eq!(guards[0].expected, vec![8, 7, 6]);
    }
    #[test]
    fn undo_refuses_changed_applied_value_and_tampered_before() {
        let receipt = item(1, 0x10000, 1, 8);
        let mut detail = Detail {
            entity: receipt.entity.clone(),
            fields: vec![Field {
                key: "passing".into(),
                label: "passing".into(),
                section: String::new(),
                value: 8.,
                min: 0.,
                max: 20.,
                step: 1.,
                hint: String::new(),
                binding: Some(Binding {
                    address: 0x10000,
                    storage: Storage::U8,
                    scale: 1.,
                    original: vec![8],
                }),
            }],
            guards: receipt.guards.clone(),
            notes: Vec::new(),
        };
        assert_eq!(reverse_edits(&receipt, &detail).unwrap()[0].value, 1.);
        let mut corrupt = receipt.clone();
        corrupt.changes[0].before = 2.;
        assert!(reverse_edits(&corrupt, &detail).is_err());
        detail.fields[0].binding.as_mut().unwrap().original = vec![9];
        assert!(reverse_edits(&receipt, &detail).is_err());
    }

    #[test]
    fn batch_trait_undo_restores_multiple_bits_and_unknown_bits() {
        let before = 1u64 << 61;
        let after = before | 3;
        let mut receipt = item(1, 0x10000, 0, 1);
        receipt.changes = (0..2)
            .map(|bit| Change {
                key: format!("trait_{}", bit + 1),
                label: "trait".into(),
                before: 0.,
                after: 1.,
                address: 0x10000,
                old_bytes: before.to_le_bytes().to_vec(),
                new_bytes: after.to_le_bytes().to_vec(),
            })
            .collect();
        let detail = Detail {
            entity: receipt.entity.clone(),
            fields: (0..2)
                .map(|bit| Field {
                    key: format!("trait_{}", bit + 1),
                    label: "trait".into(),
                    section: String::new(),
                    value: 1.,
                    min: 0.,
                    max: 1.,
                    step: 1.,
                    hint: String::new(),
                    binding: Some(Binding {
                        address: 0x10000,
                        storage: Storage::Bit(bit),
                        scale: 1.,
                        original: after.to_le_bytes().to_vec(),
                    }),
                })
                .collect(),
            guards: receipt.guards.clone(),
            notes: Vec::new(),
        };
        let edits = reverse_edits(&receipt, &detail).unwrap();
        assert_eq!(edits.len(), 2);
        let reverse = transaction::prepare(&detail, &edits).unwrap();
        assert!(
            reverse
                .iter()
                .all(|change| change.new_bytes == before.to_le_bytes())
        );
    }

    #[test]
    fn later_dependency_guard_blocks_all_entities() {
        let memory = Mock::new();
        let first = item(1, 0x10000, 1, 8);
        let mut second = item(2, 0x20000, 2, 9);
        second.guards.push(Guard {
            address: 0x40000,
            expected: vec![99],
        });
        assert!(write_prepared(&memory, &[first, second]).is_err());
        assert_eq!(memory.writes.get(), 0);
    }
}
