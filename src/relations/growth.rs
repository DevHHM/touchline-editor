//! Existing PERSON+0x78 holder only: native ctor RVA 0x333A5D0 creates a
//! 0x50-byte object, including three vector headers. Never create a 24-byte
//! substitute. The first vector contains inline 16-byte relations.
use super::*;
use crate::native_alloc::NativeAllocation;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelationVectorGrowth {
    pub holder: u64,
    pub begin: u64,
    pub end: u64,
    pub capacity: u64,
    pub requested_bytes: usize,
    pub original_records: Vec<u8>,
    pub appended_record: Vec<u8>,
}

pub(super) fn intent(snapshot: &PersonRelations, new: Vec<u8>) -> Result<RelationVectorGrowth> {
    ensure!(
        snapshot.vector_growth_available && snapshot.spare_slots == 0,
        "관계 용량 확장 기능이 검증되지 않았습니다"
    );
    ensure!(
        pointer(snapshot.holder) && new.len() == 16,
        "관계 확장 대상이 유효하지 않습니다"
    );
    ensure!(
        guarded_bytes(&snapshot.guards, snapshot.entity.address + 0x78, 8)?
            == snapshot.holder.to_le_bytes(),
        "관계 목록 소유자가 변경됐습니다"
    );
    let bounds = vector_bounds(&guarded_bytes(&snapshot.guards, snapshot.holder, 24)?)?;
    ensure!(
        bounds == (snapshot.begin, snapshot.end, snapshot.capacity)
            && snapshot.end == snapshot.capacity,
        "관계 확장 전 경계가 다릅니다"
    );
    let original_records: Vec<u8> = snapshot
        .rows
        .iter()
        .flat_map(|row| row.raw.iter().copied())
        .collect();
    ensure!(
        original_records.len() as u64 == snapshot.end - snapshot.begin
            && guarded_bytes(&snapshot.guards, snapshot.begin, original_records.len())?
                == original_records,
        "관계 확장 전 전체 목록이 다릅니다"
    );
    let required = original_records
        .len()
        .checked_add(16)
        .context("관계 개수 범위 초과")?;
    ensure!(
        snapshot.rows.len() < MAX_RELATION_COUNT && required <= MAX_VECTOR_BYTES as usize,
        "관계 목록이 지원하는 최대 크기에 도달했습니다"
    );
    let requested_bytes = (original_records.len().max(16) * 2)
        .max(required)
        .min(MAX_VECTOR_BYTES as usize);
    Ok(RelationVectorGrowth {
        holder: snapshot.holder,
        begin: snapshot.begin,
        end: snapshot.end,
        capacity: snapshot.capacity,
        requested_bytes,
        original_records,
        appended_record: new,
    })
}

fn validate_intent(plan: &RelationPlan, intent: &RelationVectorGrowth) -> Result<()> {
    ensure!(
        matches!(
            plan.request,
            RelationRequest::AddSocialRelation { .. }
                | RelationRequest::AddFavouriteClub { .. }
                | RelationRequest::AddSecondaryNationality { .. }
        ),
        "이 관계 작업은 용량 확장을 지원하지 않습니다"
    );
    ensure!(
        pointer(intent.holder)
            && intent.appended_record.len() == 16
            && intent.original_records.len() % 16 == 0,
        "관계 확장 기록의 크기가 다릅니다"
    );
    let header = guarded_bytes(&plan.guards, intent.holder, 24)?;
    ensure!(
        vector_bounds(&header)? == (intent.begin, intent.end, intent.capacity)
            && intent.end == intent.capacity,
        "관계 확장 기록의 경계가 다릅니다"
    );
    ensure!(
        guarded_bytes(&plan.guards, plan.entity.address + 0x78, 8)? == intent.holder.to_le_bytes(),
        "관계 확장 기록의 소유자가 다릅니다"
    );
    ensure!(
        intent.original_records.len() as u64 == intent.end - intent.begin
            && guarded_bytes(&plan.guards, intent.begin, intent.original_records.len())?
                == intent.original_records,
        "관계 확장 기록의 원본 목록이 다릅니다"
    );
    ensure!(
        intent.requested_bytes % 16 == 0
            && intent.requested_bytes >= intent.original_records.len() + 16
            && intent.requested_bytes <= MAX_VECTOR_BYTES as usize,
        "관계 확장 용량이 지원 범위를 벗어납니다"
    );
    ensure!(
        intent.original_records.len() / 16 < MAX_RELATION_COUNT,
        "관계 확장 기록이 게임의 255개 제한을 벗어납니다"
    );
    Ok(())
}

fn matches_session(a: &Session, b: &Session) -> Result<bool> {
    Ok(serde_json::to_vec(a)? == serde_json::to_vec(b)?)
}

/// Bind the reviewed logical append to a verified, unpublished native buffer.
/// No byte is written here. The existing holder's header is always published last.
pub(super) fn materialize(
    memory: &impl RelationMemory,
    plan: &RelationPlan,
    allocation: NativeAllocation,
    retained_original: Option<NativeAllocation>,
) -> Result<RelationPlan> {
    let intent = plan.growth.as_ref().context("관계 확장 계획이 없습니다")?;
    validate_intent(plan, intent)?;
    ensure!(
        plan.changes.is_empty() && plan.allocation.is_none() && plan.retained_original.is_none(),
        "이미 확정한 관계 확장 계획입니다"
    );
    ensure!(
        matches_session(&plan.session, &allocation.session)?
            && pointer(allocation.address)
            && allocation.len >= intent.requested_bytes
            && allocation.len <= MAX_VECTOR_BYTES as usize
            && allocation.len % 16 == 0,
        "할당 결과가 검토한 용량·세션과 다릅니다"
    );
    let limit = allocation
        .address
        .checked_add(allocation.len as u64)
        .context("할당 주소 범위 초과")?;
    ensure!(pointer(limit - 1), "할당 끝 주소가 유효하지 않습니다");
    for (start, end) in [
        (intent.begin, intent.capacity),
        (intent.holder, intent.holder + 0x50),
        (plan.entity.address, plan.entity.address + 0x80),
    ] {
        ensure!(
            limit <= start || allocation.address >= end,
            "새 관계 배열이 기존 객체와 겹칩니다"
        );
    }
    match (&retained_original, intent.begin) {
        (None, 0) => (),
        (Some(old), begin) => ensure!(
            matches_session(&plan.session, &old.session)?
                && old.address == begin
                && old.len as u64 == intent.capacity - begin,
            "보존한 원본 배열의 소유권 기록이 다릅니다"
        ),
        _ => bail!("원본 관계 배열의 보존 기록이 없습니다"),
    }
    validate_guards(memory, &plan.guards)?;
    let mut out = plan.clone();
    let before = guard(memory, &mut out.guards, allocation.address, allocation.len)?;
    let mut after = intent.original_records.clone();
    after.extend_from_slice(&intent.appended_record);
    let used = after.len();
    after.resize(allocation.len, 0);
    for (index, (old, new)) in before.chunks(64).zip(after.chunks(64)).enumerate() {
        if old != new {
            out.changes.push(change(
                "relation_grow_data",
                "확장한 관계 목록 준비",
                allocation.address + (index * 64) as u64,
                old.to_vec(),
                new.to_vec(),
            )?);
        }
    }
    let header = [
        allocation.address.to_le_bytes(),
        (allocation.address + used as u64).to_le_bytes(),
        limit.to_le_bytes(),
    ]
    .concat();
    out.changes.push(change(
        "relation_grow_publish",
        "확장한 관계 목록 연결",
        intent.holder,
        guarded_bytes(&plan.guards, intent.holder, 24)?,
        header,
    )?);
    out.allocation = Some(allocation);
    out.retained_original = retained_original;
    Ok(out)
}

/// Check the canonical publication and both buffers, not caller-recorded write
/// addresses. Native allocation provenance is checked separately against its ledger.
pub(super) fn validate_undo(
    memory: &impl RelationMemory,
    current: &PersonRelations,
    original: &RelationPlan,
) -> Result<()> {
    let intent = original
        .growth
        .as_ref()
        .context("관계 확장 기록이 없습니다")?;
    let allocation = original
        .allocation
        .as_ref()
        .context("관계 할당 기록이 없습니다")?;
    let mut symbolic = original.clone();
    symbolic.changes.clear();
    symbolic.allocation = None;
    symbolic.retained_original = None;
    let canonical = materialize(
        &GuardMemory(&original.guards),
        &symbolic,
        allocation.clone(),
        original.retained_original.clone(),
    )?;
    ensure!(
        serde_json::to_vec(&canonical.changes)? == serde_json::to_vec(&original.changes)?,
        "관계 확장 변경 기록이 변조됐습니다"
    );
    ensure!(
        current.holder == intent.holder
            && current.begin == allocation.address
            && current.end == allocation.address + intent.original_records.len() as u64 + 16
            && current.capacity == allocation.address + allocation.len as u64,
        "확장 후 관계 배열이 변경됐습니다"
    );
    validate_guards(
        memory,
        &expected_guards(&original.guards, &original.changes),
    )
}

pub(super) fn allocate_and_materialize(plan: &RelationPlan) -> Result<RelationPlan> {
    let intent = plan.growth.as_ref().context("관계 확장 계획이 없습니다")?;
    validate_intent(plan, intent)?;
    crate::native_alloc::available(&plan.session)?;
    // The allocator runs before the publication transaction suspends FM. Never
    // call a potentially locking native allocator while every thread is frozen.
    let allocation = crate::native_alloc::allocate(&plan.session, intent.requested_bytes)?;
    let mut retained = None;
    let result = (|| {
        let memory = Process::open(plan.session.pid, false)?;
        backend::validate_with(&memory, &plan.session)?;
        validate_guards(&memory, &plan.guards)?;
        if intent.begin != 0 {
            retained = Some(crate::native_alloc::adopt_retained(
                &plan.session,
                intent.begin,
                (intent.capacity - intent.begin) as usize,
                intent.holder,
            )?);
        }
        materialize(&memory, plan, allocation.clone(), retained.clone())
    })();
    if result.is_err() {
        let _ = crate::native_alloc::mark_uncertain(&allocation);
        if let Some(old) = retained {
            let _ = crate::native_alloc::mark_uncertain(&old);
        }
    }
    result
}

pub(super) fn validate_ownership(plan: &RelationPlan) -> Result<()> {
    let allocation = plan
        .allocation
        .as_ref()
        .context("관계 할당 기록이 없습니다")?;
    crate::native_alloc::verify_retained(allocation)?;
    if let Some(old) = &plan.retained_original {
        crate::native_alloc::verify_retained(old)?;
    }
    Ok(())
}

pub(super) fn record_publication(plan: &RelationPlan, undo: bool) -> Option<String> {
    record_publication_with(
        plan,
        undo,
        crate::native_alloc::mark_published,
        crate::native_alloc::mark_detached,
    )
}

fn record_publication_with(
    plan: &RelationPlan,
    undo: bool,
    mut published: impl FnMut(&NativeAllocation, u64) -> Result<()>,
    mut detached: impl FnMut(&NativeAllocation) -> Result<()>,
) -> Option<String> {
    let Some(intent) = &plan.growth else {
        return None;
    };
    let Some(allocation) = &plan.allocation else {
        return Some("관계 할당 기록이 없습니다".into());
    };
    let mut warnings = vec![];
    let results = if undo {
        vec![
            detached(allocation),
            plan.retained_original
                .as_ref()
                .map_or(Ok(()), |old| published(old, intent.holder)),
        ]
    } else {
        vec![
            published(allocation, intent.holder),
            plan.retained_original
                .as_ref()
                .map_or(Ok(()), &mut detached),
        ]
    };
    for result in results {
        if let Err(error) = result {
            warnings.push(format!("{error:#}"));
        }
    }
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    }
}

pub(super) fn mark_uncertain(plan: &RelationPlan) {
    if let Some(allocation) = &plan.allocation {
        let _ = crate::native_alloc::mark_uncertain(allocation);
    }
    if let Some(old) = &plan.retained_original {
        let _ = crate::native_alloc::mark_uncertain(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    struct Memory {
        bytes: RefCell<BTreeMap<u64, u8>>,
        fail: RefCell<Option<u64>>,
        writes: RefCell<Vec<u64>>,
    }
    impl RelationMemory for Memory {
        fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
            (0..size)
                .map(|i| {
                    self.bytes
                        .borrow()
                        .get(&(address + i as u64))
                        .copied()
                        .context("unmapped synthetic byte")
                })
                .collect()
        }
        fn write(&self, address: u64, bytes: &[u8]) -> Result<()> {
            self.writes.borrow_mut().push(address);
            let fail = *self.fail.borrow() == Some(address);
            for (i, byte) in bytes
                .iter()
                .take(if fail { 1 } else { bytes.len() })
                .enumerate()
            {
                self.bytes.borrow_mut().insert(address + i as u64, *byte);
            }
            if fail {
                self.fail.replace(None);
                bail!("synthetic partial write");
            }
            Ok(())
        }
    }
    fn fixture(empty: bool, allocated_bytes: usize) -> (Memory, PersonRelations, RelationPlan) {
        let (index, _, snapshots) = fixtures::data();
        let mut snapshot = snapshots[0].clone();
        snapshot.guards.retain(|g| g.address != snapshot.end);
        snapshot.capacity = snapshot.end;
        snapshot.spare_slots = 0;
        snapshot.append_bytes = None;
        snapshot.vector_growth_available = true;
        if empty {
            snapshot
                .guards
                .retain(|g| !(snapshot.begin..snapshot.end).contains(&g.address));
            snapshot.begin = 0;
            snapshot.end = 0;
            snapshot.capacity = 0;
            snapshot.rows.clear();
        }
        snapshot
            .guards
            .iter_mut()
            .find(|g| g.address == snapshot.holder)
            .unwrap()
            .expected = [
            snapshot.begin.to_le_bytes(),
            snapshot.end.to_le_bytes(),
            snapshot.capacity.to_le_bytes(),
        ]
        .concat();
        let club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let symbolic = prepare_snapshot(
            &index.session,
            &snapshot,
            &RelationRequest::AddFavouriteClub {
                template_slot: None,
                club,
                reason: 1,
                level: 90,
            },
            Some(&target),
            None,
            vec![],
        )
        .unwrap();
        assert!(symbolic.changes.is_empty());
        let mut bytes = BTreeMap::new();
        for guard in &snapshot.guards {
            for (i, byte) in guard.expected.iter().enumerate() {
                bytes.insert(guard.address + i as u64, *byte);
            }
        }
        for i in 0..allocated_bytes {
            bytes.insert(0xF0000 + i as u64, 0xCC);
        }
        let memory = Memory {
            bytes: RefCell::new(bytes),
            fail: RefCell::new(None),
            writes: RefCell::new(vec![]),
        };
        let allocation = NativeAllocation {
            id: "synthetic-new".into(),
            address: 0xF0000,
            len: allocated_bytes,
            session: index.session.clone(),
        };
        let retained = (!empty).then(|| NativeAllocation {
            id: "synthetic-original".into(),
            address: snapshot.begin,
            len: (snapshot.capacity - snapshot.begin) as usize,
            session: index.session,
        });
        let plan = materialize(&memory, &symbolic, allocation, retained).unwrap();
        (memory, snapshot, plan)
    }
    fn after(snapshot: &PersonRelations, plan: &RelationPlan) -> PersonRelations {
        let mut out = snapshot.clone();
        let allocation = plan.allocation.as_ref().unwrap();
        out.begin = allocation.address;
        out.end =
            allocation.address + plan.growth.as_ref().unwrap().original_records.len() as u64 + 16;
        out.capacity = allocation.address + allocation.len as u64;
        out
    }
    #[test]
    fn native_growth_existing_and_zero_empty_vector_preserve_original_and_undo() {
        for empty in [false, true] {
            // Allocator may provide a larger verified block than requested.
            let (memory, snapshot, plan) = fixture(empty, 128);
            let mut expected_after_undo = memory.bytes.borrow().clone();
            assert!(
                plan.changes[..plan.changes.len() - 1]
                    .iter()
                    .all(|c| c.address >= 0xF0000 && c.old_bytes.len() <= 64)
            );
            assert_eq!(plan.changes.last().unwrap().address, snapshot.holder);
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            assert_eq!(memory.writes.borrow().last(), Some(&snapshot.holder));
            if !empty {
                assert_eq!(
                    memory
                        .read(snapshot.begin, (snapshot.end - snapshot.begin) as usize)
                        .unwrap(),
                    plan.growth.as_ref().unwrap().original_records
                );
            }
            validate_undo(&memory, &after(&snapshot, &plan), &plan).unwrap();
            // Native threads may retain this pointer across the header swap.
            for (offset, byte) in memory.read(0xF0000, 128).unwrap().iter().enumerate() {
                expected_after_undo.insert(0xF0000 + offset as u64, *byte);
            }
            let reverse = reverse_plan(&plan);
            assert_eq!(reverse.changes.len(), 1);
            assert_eq!(reverse.changes[0].address, snapshot.holder);
            write_batch(
                &memory,
                &reverse.changes,
                &expected_guards(&reverse.guards, &reverse.changes),
            )
            .unwrap();
            assert_eq!(*memory.bytes.borrow(), expected_after_undo);
        }
    }
    #[test]
    fn native_growth_each_data_publish_and_undo_write_failure_rolls_back() {
        let (_, _, exemplar) = fixture(false, 128);
        for failure in exemplar.changes.iter().map(|c| c.address) {
            let (memory, _, plan) = fixture(false, 128);
            let before = memory.bytes.borrow().clone();
            memory.fail.replace(Some(failure));
            assert!(
                write_batch(
                    &memory,
                    &plan.changes,
                    &expected_guards(&plan.guards, &plan.changes)
                )
                .is_err()
            );
            assert_eq!(*memory.bytes.borrow(), before);
        }
        for failure in reverse_plan(&exemplar).changes.iter().map(|c| c.address) {
            let (memory, _, plan) = fixture(false, 128);
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            let published = memory.bytes.borrow().clone();
            memory.fail.replace(Some(failure));
            let reverse = reverse_plan(&plan);
            assert!(
                write_batch(
                    &memory,
                    &reverse.changes,
                    &expected_guards(&reverse.guards, &reverse.changes)
                )
                .is_err()
            );
            assert_eq!(*memory.bytes.borrow(), published);
        }
    }
    #[test]
    fn native_growth_rejects_external_changes_to_owner_header_old_and_new_buffers() {
        for changed in [0x10078, 0x80000, 0x81000, 0xF0000, 0xF007F] {
            let (memory, snapshot, plan) = fixture(false, 128);
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            let old = memory.bytes.borrow()[&changed];
            memory.bytes.borrow_mut().insert(changed, old ^ 0xFF);
            memory.writes.borrow_mut().clear();
            assert!(validate_undo(&memory, &after(&snapshot, &plan), &plan).is_err());
            assert!(memory.writes.borrow().is_empty());
        }
    }
    #[test]
    fn native_growth_rejects_forged_publication_and_missing_original_ownership() {
        let (memory, snapshot, mut plan) = fixture(false, 128);
        write_batch(
            &memory,
            &plan.changes,
            &expected_guards(&plan.guards, &plan.changes),
        )
        .unwrap();
        plan.changes.last_mut().unwrap().address += 8;
        assert!(validate_undo(&memory, &after(&snapshot, &plan), &plan).is_err());
        let (_, _, mut plan) = fixture(false, 128);
        plan.retained_original = None;
        assert!(validate_undo(&memory, &after(&snapshot, &plan), &plan).is_err());
    }
    #[test]
    fn native_growth_committed_ledger_failures_are_warnings_and_update_both_owners() {
        let (_, _, plan) = fixture(false, 128);
        for undo in [false, true] {
            let called = RefCell::new(vec![]);
            let warning = record_publication_with(
                &plan,
                undo,
                |allocation, owner| {
                    assert_eq!(owner, 0x80000);
                    called
                        .borrow_mut()
                        .push(format!("published:{}", allocation.id));
                    bail!("synthetic publication ledger failure")
                },
                |allocation| {
                    called
                        .borrow_mut()
                        .push(format!("detached:{}", allocation.id));
                    bail!("synthetic retention ledger failure")
                },
            )
            .unwrap();
            assert!(
                warning.contains("publication ledger failure")
                    && warning.contains("retention ledger failure")
            );
            assert_eq!(
                *called.borrow(),
                if undo {
                    vec!["detached:synthetic-new", "published:synthetic-original"]
                } else {
                    vec!["published:synthetic-new", "detached:synthetic-original"]
                }
            );
        }
    }
}
