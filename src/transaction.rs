//! Checked scalar edits. All backups are persisted before the first process write.
use crate::{backend, memory::Process, model::*};
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn data_dir() -> PathBuf {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = root.join("TouchlineEditor");
    let _ = std::fs::create_dir_all(&path);
    path
}
fn history_dir() -> Result<PathBuf> {
    let p = data_dir().join("history");
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
fn clock() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
fn persist(receipt: &Receipt) -> Result<()> {
    let p = history_dir()?.join(format!("{}.json", receipt.id));
    persist_json(&p, receipt)
}
/// Durable same-volume JSON replacement shared by scalar and atomic batch journals.
pub(crate) fn persist_json(p: &Path, value: &impl serde::Serialize) -> Result<()> {
    let tmp = p.with_extension("pending");
    use std::io::Write;
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    // Same-volume replacement. Windows rename cannot replace an existing file.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
        }
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(tmp, p)?;
    Ok(())
}
pub fn history() -> Vec<Receipt> {
    let Ok(dir) = history_dir() else {
        return vec![];
    };
    let mut list: Vec<Receipt> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|x| x.ok())
        .filter(|x| x.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|x| std::fs::read(x.path()).ok())
        .filter_map(|x| serde_json::from_slice(&x).ok())
        .collect();
    list.sort_by(|a, b| b.time.cmp(&a.time));
    list.truncate(500);
    list
}

pub fn encode_value(field: &Field, value: f64) -> Result<Vec<u8>> {
    ensure!(
        value.is_finite() && field.min.is_finite() && field.max.is_finite(),
        "유한한 숫자를 입력하세요."
    );
    ensure!(
        value >= field.min && value <= field.max,
        "{}: {}–{} 범위로 입력하세요.",
        field.label,
        field.min,
        field.max
    );
    let b = field
        .binding
        .as_ref()
        .ok_or_else(|| anyhow!("{}은 읽기 전용입니다.", field.label))?;
    ensure!(b.scale.is_finite() && b.scale > 0., "잘못된 필드 배율");
    ensure!(
        field.step.is_finite() && field.step > 0.,
        "잘못된 입력 단위"
    );
    let steps = (value - field.min) / field.step;
    ensure!(
        (steps - steps.round()).abs() < 1e-6,
        "{}: {} 단위로 입력하세요.",
        field.label,
        field.step
    );
    let v = value * b.scale;
    ensure!(v.is_finite(), "숫자 범위 초과");
    macro_rules! integer {
        ($t:ty) => {{
            ensure!(
                v >= <$t>::MIN as f64 && v <= <$t>::MAX as f64 && (v - v.round()).abs() < 1e-6,
                "{} 저장 범위 초과",
                field.label
            );
            (v.round() as $t).to_le_bytes().to_vec()
        }};
    }
    let bytes = match b.storage {
        Storage::Bit(bit) => crate::traits::encode(bit, &b.original, value)?,
        Storage::U8 => integer!(u8),
        Storage::U16 => integer!(u16),
        Storage::U32 => integer!(u32),
        Storage::I32 => integer!(i32),
        Storage::U64 => {
            ensure!(
                v <= 9_007_199_254_740_991.,
                "정확한 정수 범위를 초과했습니다."
            );
            integer!(u64)
        }
        Storage::I64 => {
            ensure!(
                v.abs() <= 9_007_199_254_740_991.,
                "정확한 정수 범위를 초과했습니다."
            );
            integer!(i64)
        }
        Storage::F32 => {
            ensure!((v as f32).is_finite(), "숫자 범위 초과");
            (v as f32).to_le_bytes().to_vec()
        }
        Storage::F64 => v.to_le_bytes().to_vec(),
        Storage::FmDate => crate::extended::encode_date(v, &b.original)?
            .to_le_bytes()
            .to_vec(),
        Storage::OptionalU32 => {
            if v == -1. {
                u32::MAX.to_le_bytes().to_vec()
            } else {
                integer!(u32)
            }
        }
    };
    ensure!(
        b.original.len() == b.storage.size(),
        "원본 데이터 크기 불일치"
    );
    Ok(bytes)
}
pub fn prepare(detail: &Detail, edits: &[Edit]) -> Result<Vec<Change>> {
    let changes = prepare_allow_unchanged(detail, edits)?;
    ensure!(!changes.is_empty(), "실제 변경한 값이 없습니다.");
    Ok(changes)
}
/// Validate a desired state even when all values already match. This is used
/// for freeze previews; ordinary apply still requires actual changed bytes.
pub fn prepare_allow_unchanged(detail: &Detail, edits: &[Edit]) -> Result<Vec<Change>> {
    ensure!(!edits.is_empty(), "변경한 값이 없습니다.");
    ensure!(
        edits.len() <= 256,
        "한 번에 최대 256개 필드를 변경할 수 있습니다."
    );
    let mut keys = HashSet::new();
    let mut changes = vec![];
    for e in edits {
        ensure!(keys.insert(&e.key), "중복 변경 항목: {}", e.key);
        let f = detail
            .fields
            .iter()
            .find(|x| x.key == e.key)
            .ok_or_else(|| anyhow!("알 수 없는 필드: {}", e.key))?;
        let bytes = encode_value(f, e.value)?;
        let b = f.binding.as_ref().unwrap();
        if bytes == b.original {
            continue;
        }
        ensure!(
            b.address >= 0x10000 && b.address < 0x7fff_ffff_fff0,
            "유효하지 않은 주소"
        );
        changes.push(Change {
            key: e.key.clone(),
            label: f.label.clone(),
            before: f.value,
            after: e.value,
            address: b.address,
            old_bytes: b.original.clone(),
            new_bytes: bytes,
        });
    }
    crate::traits::merge_changes(detail, &mut changes)?;
    changes.sort_by_key(|c| c.address);
    for p in changes.windows(2) {
        ensure!(
            p[0].address + p[0].old_bytes.len() as u64 <= p[1].address
                || crate::traits::shared_bit_storage(detail, &p[0], &p[1]),
            "겹치는 필드 변경"
        );
    }
    let value = |key: &str| -> Option<f64> {
        edits
            .iter()
            .find(|x| x.key == key)
            .map(|x| x.value)
            .or_else(|| detail.fields.iter().find(|x| x.key == key).map(|x| x.value))
    };
    if let (Some(ca), Some(pa)) = (value("ca"), value("pa")) {
        ensure!(
            ca <= pa,
            "현재 능력(CA)은 잠재 능력(PA)을 넘을 수 없습니다."
        );
    }
    crate::extended::validate_edits(detail, edits)?;
    Ok(changes)
}

pub(crate) trait MemoryAccess {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>>;
    fn write(&self, address: u64, bytes: &[u8]) -> Result<()>;
}
impl MemoryAccess for Process {
    fn read(&self, a: u64, s: usize) -> Result<Vec<u8>> {
        Process::read(self, a, s)
    }
    fn write(&self, a: u64, b: &[u8]) -> Result<()> {
        Process::write(self, a, b)
    }
}
pub(crate) fn validate_originals(
    m: &impl MemoryAccess,
    guards: &[Guard],
    changes: &[Change],
) -> Result<()> {
    ensure!(
        !guards.is_empty(),
        "대상 식별 정보가 없습니다. 다시 연결하세요."
    );
    for g in guards {
        ensure!(
            !g.expected.is_empty() && g.expected.len() <= 256,
            "잘못된 검증 정보"
        );
        ensure!(
            m.read(g.address, g.expected.len())? == g.expected,
            "게임 데이터가 변경되었습니다. 다시 불러오세요."
        );
    }
    for c in changes {
        ensure!(
            m.read(c.address, c.old_bytes.len())? == c.old_bytes,
            "{} 값이 다른 곳에서 변경되었습니다. 다시 불러오세요.",
            c.label
        );
    }
    Ok(())
}
pub(crate) fn dependency_guards(detail: &Detail, changes: &[Change]) -> Result<Vec<Guard>> {
    let mut guards = crate::extended::dependency_guards(detail, changes)?;
    if changes.iter().any(|c| c.key == "ca" || c.key == "pa") {
        for key in ["ca", "pa"] {
            let b = detail
                .fields
                .iter()
                .find(|f| f.key == key)
                .and_then(|f| f.binding.as_ref())
                .context("능력치 검증 정보가 없습니다")?;
            guards.push(Guard {
                address: b.address,
                expected: b.original.clone(),
            });
        }
    }
    Ok(guards)
}
/// Caller holds the target suspended throughout validation + writes + rollback.
pub(crate) fn write_batch(m: &impl MemoryAccess, changes: &[Change]) -> Result<()> {
    for (i, c) in changes.iter().enumerate() {
        let result = m.write(c.address, &c.new_bytes).and_then(|()| {
            ensure!(
                m.read(c.address, c.new_bytes.len())? == c.new_bytes,
                "읽기 검증 실패: {}",
                c.label
            );
            Ok(())
        });
        if let Err(err) = result {
            let mut failures = vec![];
            // Include the failing write: WriteProcessMemory can partially modify it.
            for old in changes[..=i].iter().rev() {
                if let Err(e) = m.write(old.address, &old.old_bytes).and_then(|()| {
                    ensure!(
                        m.read(old.address, old.old_bytes.len())? == old.old_bytes,
                        "복구 검증 실패"
                    );
                    Ok(())
                }) {
                    failures.push(format!("{}: {e}", old.label));
                }
            }
            if failures.is_empty() {
                bail!("적용 실패; 원래 값으로 복구했습니다. {err:#}");
            }
            bail!(
                "적용 실패 및 일부 복구 실패. 기록을 확인하세요. {err:#}; {}",
                failures.join("; ")
            );
        }
    }
    Ok(())
}
pub fn apply(session: &Session, detail: &Detail, edits: &[Edit]) -> Result<Receipt> {
    crate::compatibility::require_scalar_write(session)?;
    backend::validate_session(session)?;
    // Re-resolve the entity's structure and bindings. Never trust addresses from imported UI data.
    let fresh = backend::detail(session, &detail.entity)?;
    ensure!(
        fresh.guards == detail.guards,
        "대상 정보가 달라졌습니다. 다시 불러오세요."
    );
    for e in edits {
        let old = detail
            .fields
            .iter()
            .find(|f| f.key == e.key)
            .ok_or_else(|| anyhow!("필드 없음"))?;
        let new = fresh
            .fields
            .iter()
            .find(|f| f.key == e.key)
            .ok_or_else(|| anyhow!("필드가 변경되었습니다."))?;
        ensure!(
            old.binding == new.binding,
            "{}의 현재 값이 변경됐습니다. 다시 불러오세요.",
            old.label
        );
    }
    let changes = prepare(&fresh, edits)?;
    let id = format!("{}-{}", clock(), session.pid);
    let mut receipt = Receipt {
        id: id.clone(),
        time: clock(),
        session: session.clone(),
        entity: fresh.entity.clone(),
        guards: fresh.guards.clone(),
        changes,
        status: "prepared".into(),
    };
    // CA/PA form a pair: an unedited member may change while the backup is written.
    receipt
        .guards
        .extend(dependency_guards(&fresh, &receipt.changes)?);
    persist(&receipt).context("변경 전 백업을 저장하지 못했습니다. 적용하지 않았습니다.")?;
    let mut resume_warning = None;
    let result = (|| -> Result<()> {
        let proc = Process::open(session.pid, true)?;
        ensure!(
            proc.created()? == session.created,
            "FM 프로세스가 교체되었습니다."
        );
        let suspended = proc.suspend()?;
        let edit_result = (|| -> Result<()> {
            backend::validate_session(session)?;
            validate_originals(&proc, &receipt.guards, &receipt.changes)?;
            write_batch(&proc, &receipt.changes)
        })();
        if let Err(e) = suspended.resume() {
            resume_warning = Some(format!("FM 재개 확인 실패: {e:#}"));
        }
        edit_result
    })();
    match result {
        Ok(()) => {
            receipt.status = resume_warning
                .map(|s| format!("applied_resume_warning: {s}"))
                .unwrap_or_else(|| "applied".into());
            if let Err(e) = persist(&receipt) {
                receipt.status = format!("{}_log_warning: {e}", receipt.status);
            }
            Ok(receipt)
        }
        Err(e) => {
            let e = if let Some(w) = resume_warning {
                anyhow!("{e:#}; {w}")
            } else {
                e
            };
            receipt.status = format!("failed: {e:#}");
            let _ = persist(&receipt);
            Err(e)
        }
    }
}
pub fn undo(receipt: &Receipt) -> Result<Receipt> {
    ensure!(
        receipt.status.starts_with("applied"),
        "완료된 적용만 되돌릴 수 있습니다."
    );
    backend::validate_session(&receipt.session)?;
    // Audit records must correspond to currently supported bindings before any write.
    let fresh = backend::detail(&receipt.session, &receipt.entity)?;
    let mut edits = vec![];
    for c in &receipt.changes {
        let f = fresh
            .fields
            .iter()
            .find(|f| f.key == c.key)
            .ok_or_else(|| anyhow!("기록의 필드를 현재 찾을 수 없습니다."))?;
        let b = f
            .binding
            .as_ref()
            .ok_or_else(|| anyhow!("이 필드는 읽기 전용입니다."))?;
        ensure!(
            b.address == c.address && b.original == c.new_bytes,
            "{} 값이 적용 후 변경되어 되돌리기를 중단했습니다.",
            c.label
        );
        edits.push(Edit {
            key: c.key.clone(),
            value: c.before,
        });
    }
    let mut reversed = apply(&receipt.session, &fresh, &edits)?;
    reversed.status = if reversed.status.contains("warning") {
        format!("undone_warning:{}; {}", receipt.id, reversed.status)
    } else {
        format!("undone:{}", receipt.id)
    };
    if let Err(e) = persist(&reversed) {
        reversed.status = format!("{}_log_warning: {e}", reversed.status);
    }
    Ok(reversed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};
    struct Mock {
        data: RefCell<BTreeMap<u64, u8>>,
        fail: RefCell<Option<u64>>,
    }
    impl MemoryAccess for Mock {
        fn read(&self, a: u64, n: usize) -> Result<Vec<u8>> {
            (0..n)
                .map(|i| {
                    self.data
                        .borrow()
                        .get(&(a + i as u64))
                        .copied()
                        .ok_or_else(|| anyhow!("unmapped"))
                })
                .collect()
        }
        fn write(&self, a: u64, b: &[u8]) -> Result<()> {
            for (i, v) in b.iter().enumerate() {
                self.data.borrow_mut().insert(a + i as u64, *v);
            }
            if *self.fail.borrow() == Some(a) {
                self.fail.replace(None);
                bail!("partial write")
            };
            Ok(())
        }
    }
    fn field(key: &str, value: f64) -> Field {
        Field {
            key: key.into(),
            label: key.into(),
            section: "test".into(),
            value,
            min: 1.,
            max: 200.,
            step: 1.,
            hint: String::new(),
            binding: Some(Binding {
                address: 0x10000,
                storage: Storage::U16,
                scale: 1.,
                original: (value as u16).to_le_bytes().to_vec(),
            }),
        }
    }
    fn change(a: u64, b: u8, n: u8) -> Change {
        Change {
            key: "test".into(),
            label: "test".into(),
            before: b as f64,
            after: n as f64,
            address: a,
            old_bytes: vec![b],
            new_bytes: vec![n],
        }
    }
    #[test]
    fn rejects_invalid_numeric() {
        let f = field("ca", 100.);
        for v in [0., 201., f64::NAN, f64::INFINITY, 1.5] {
            assert!(encode_value(&f, v).is_err());
        }
    }
    #[test]
    fn scale_is_applied() {
        let mut f = field("passing", 10.);
        f.max = 20.;
        f.binding.as_mut().unwrap().storage = Storage::U8;
        f.binding.as_mut().unwrap().original = vec![50];
        f.binding.as_mut().unwrap().scale = 5.;
        assert_eq!(encode_value(&f, 16.).unwrap(), vec![80]);
    }
    fn morale_field(value: u8) -> Field {
        let mut f = field("morale", value as f64);
        f.min = 0.;
        f.max = 20.;
        let binding = f.binding.as_mut().unwrap();
        binding.address = 0x1026C;
        binding.storage = Storage::U8;
        binding.original = vec![value];
        f
    }
    #[test]
    fn morale_is_unscaled_and_preserves_neighboring_bytes() {
        for old in 0..20 {
            let mut d = detail_for_test();
            let f = morale_field(old);
            let a = f.binding.as_ref().unwrap().address;
            d.fields.push(f);
            let changes = prepare(&d, &[Edit { key: "morale".into(), value: 20. }]).unwrap();
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].address, a);
            assert_eq!(changes[0].old_bytes, vec![old]);
            assert_eq!(changes[0].new_bytes, vec![20]);
            let memory = Mock {
                data: RefCell::new([(a - 1, 0xA5), (a, old), (a + 1, 0x5A)].into()),
                fail: RefCell::new(None),
            };
            let identity = [Guard { address: a - 1, expected: vec![0xA5] }];
            validate_originals(&memory, &identity, &changes).unwrap();
            write_batch(&memory, &changes).unwrap();
            assert_eq!(memory.read(a - 1, 3).unwrap(), vec![0xA5, 20, 0x5A]);
        }
    }
    #[test]
    fn morale_rejects_out_of_range_fractional_and_nonfinite_values() {
        let f = morale_field(10);
        assert_eq!(encode_value(&f, 0.).unwrap(), vec![0]);
        assert_eq!(encode_value(&f, 20.).unwrap(), vec![20]);
        for v in [-1., 21., 255., 0.5, f64::NAN, f64::INFINITY] {
            assert!(encode_value(&f, v).is_err());
        }
    }
    #[test]
    fn stale_value_causes_zero_writes() {
        let m = Mock {
            data: RefCell::new([(10, 7), (20, 3)].into()),
            fail: RefCell::new(None),
        };
        assert!(
            validate_originals(
                &m,
                &[Guard {
                    address: 10,
                    expected: vec![7]
                }],
                &[change(20, 2, 9)]
            )
            .is_err()
        );
        assert_eq!(m.read(20, 1).unwrap(), vec![3]);
    }
    #[test]
    fn partial_failure_rolls_back_all_touched_fields() {
        let m = Mock {
            data: RefCell::new([(10, 1), (20, 2)].into()),
            fail: RefCell::new(Some(20)),
        };
        assert!(write_batch(&m, &[change(10, 1, 8), change(20, 2, 9)]).is_err());
        assert_eq!(m.read(10, 1).unwrap(), vec![1]);
        assert_eq!(m.read(20, 1).unwrap(), vec![2]);
    }
    #[test]
    fn successful_write_readback() {
        let m = Mock {
            data: RefCell::new([(10, 1)].into()),
            fail: RefCell::new(None),
        };
        write_batch(&m, &[change(10, 1, 8)]).unwrap();
        assert_eq!(m.read(10, 1).unwrap(), vec![8]);
    }
    fn detail_for_test() -> Detail {
        let mut ca = field("ca", 100.);
        ca.binding.as_mut().unwrap().address = 0x10000;
        let mut pa = field("pa", 150.);
        pa.binding.as_mut().unwrap().address = 0x10002;
        Detail {
            entity: Entity {
                nationality: String::new(),
                nationality_id: None,
                football_category: crate::model::FootballCategory::Unknown,
                id: 1,
                kind: EntityKind::Player,
                name: "Test".into(),
                english_name: String::new(),
                club: String::new(),
                club_id: None,
                age: Some(20),
                ca: Some(100),
                pa: Some(150),
                role: String::new(),
                group: String::new(),
                address: 0x10000,
                base: 0x10000,
                index: Some(1),
            },
            fields: vec![ca, pa],
            guards: vec![],
            notes: vec![],
        }
    }
    fn bit_field(key: &str, address: u64, bit: u8, original: u64) -> Field {
        Field {
            key: key.into(),
            label: key.into(),
            section: "test".into(),
            value: ((original >> bit) & 1) as f64,
            min: 0.,
            max: 1.,
            step: 1.,
            hint: String::new(),
            binding: Some(Binding {
                address,
                storage: Storage::Bit(bit),
                scale: 1.,
                original: original.to_le_bytes().to_vec(),
            }),
        }
    }
    #[test]
    fn current_club_retirement_preserves_every_other_person_flag() {
        let key = "retire_after_current_club";
        for untouched_bit in 0..64 {
            if untouched_bit == 15 { continue; }
            for turn_on in [false, true] {
                let mut d = detail_for_test();
                // Includes ordinary retirement bit 0, sex/category bit 12 and
                // the high bit without ever representing the mask as f64.
                let original = (1u64 << untouched_bit) | if turn_on { 0 } else { 1 << 15 };
                let address = d.entity.address + 0x18;
                d.fields.push(bit_field(key, address, 15, original));
                let changes = prepare(&d, &[Edit { key: key.into(), value: u8::from(turn_on) as f64 }]).unwrap();
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0].old_bytes, original.to_le_bytes());
                let after = u64::from_le_bytes(changes[0].new_bytes.clone().try_into().unwrap());
                assert_eq!(original ^ after, 1 << 15);
                assert_ne!(after & (1 << untouched_bit), 0);
                let m = Mock {
                    data: RefCell::new(original.to_le_bytes().into_iter().enumerate()
                        .map(|(i, b)| (address + i as u64, b)).collect()),
                    fail: RefCell::new(None),
                };
                m.data.borrow_mut().insert(d.entity.address, 7);
                let identity = [Guard { address: d.entity.address, expected: vec![7] }];
                validate_originals(&m, &identity, &changes).unwrap();
                write_batch(&m, &changes).unwrap();
                assert_eq!(m.read(address, 8).unwrap(), after.to_le_bytes());
            }
        }
    }
    #[test]
    fn current_club_retirement_accepts_only_zero_or_one() {
        let f = bit_field("retire_after_current_club", 0x10018, 15, 0);
        assert!(encode_value(&f, 0.).is_ok());
        assert!(encode_value(&f, 1.).is_ok());
        for value in [-1., 0.5, 2., f64::NAN, f64::INFINITY] {
            assert!(encode_value(&f, value).is_err());
        }
    }
    #[test]
    fn person_flags_do_not_inherit_preferred_move_conflicts() {
        let mut d = detail_for_test();
        let address = d.entity.address + 0x18;
        d.fields.push(bit_field("person_fixture_flag", address, 48, 1 << 49));
        let changes = prepare(&d, &[Edit { key: "person_fixture_flag".into(), value: 1. }]).unwrap();
        assert_eq!(changes[0].new_bytes, ((1u64 << 48) | (1 << 49)).to_le_bytes());
    }
    #[test]
    fn retirement_preferred_move_conflict_still_rejected() {
        let mut d = detail_for_test();
        d.fields.push(bit_field("trait_49", d.entity.address + 0xC0, 48, 1 << 49));
        d.fields.push(bit_field("retire_after_current_club", d.entity.address + 0x18, 15, 0));
        assert!(prepare(&d, &[
            Edit { key: "retire_after_current_club".into(), value: 1. },
            Edit { key: "trait_49".into(), value: 1. },
        ]).is_err());
        // Turning on the separate PersonFlags checkbox leaves preferred moves untouched.
        let changes = prepare(&d, &[Edit { key: "retire_after_current_club".into(), value: 1. }]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, d.entity.address + 0x18);
    }
    #[test]
    fn ability_pair_bounds_and_duplicates() {
        let d = detail_for_test();
        assert!(
            prepare(
                &d,
                &[Edit {
                    key: "pa".into(),
                    value: 90.
                }]
            )
            .is_err()
        );
        assert!(
            prepare(
                &d,
                &[
                    Edit {
                        key: "pa".into(),
                        value: 160.
                    },
                    Edit {
                        key: "pa".into(),
                        value: 170.
                    }
                ]
            )
            .is_err()
        );
        assert!(
            prepare(
                &d,
                &[Edit {
                    key: "unknown".into(),
                    value: 12.
                }]
            )
            .is_err()
        );
        assert_eq!(
            prepare(
                &d,
                &[
                    Edit {
                        key: "ca".into(),
                        value: 170.
                    },
                    Edit {
                        key: "pa".into(),
                        value: 180.
                    }
                ]
            )
            .unwrap()
            .len(),
            2
        );
    }
    #[test]
    fn unedited_ability_is_checked_before_pa_write() {
        let d = detail_for_test();
        let changes = prepare(
            &d,
            &[Edit {
                key: "pa".into(),
                value: 120.,
            }],
        )
        .unwrap();
        let guards = dependency_guards(&d, &changes).unwrap();
        assert_eq!(guards.len(), 2);
        let m = Mock {
            data: RefCell::new([(0x10000, 130), (0x10001, 0), (0x10002, 150), (0x10003, 0)].into()),
            fail: RefCell::new(None),
        };
        assert!(validate_originals(&m, &guards, &changes).is_err());
        assert_eq!(m.read(0x10002, 2).unwrap(), vec![150, 0]);
    }
    #[test]
    fn overlapping_and_read_only_fields_are_rejected() {
        let mut d = detail_for_test();
        d.fields[1].binding.as_mut().unwrap().address = 0x10001;
        assert!(
            prepare(
                &d,
                &[
                    Edit {
                        key: "ca".into(),
                        value: 120.
                    },
                    Edit {
                        key: "pa".into(),
                        value: 180.
                    }
                ]
            )
            .is_err()
        );
        d.fields[0].binding = None;
        assert!(
            prepare(
                &d,
                &[Edit {
                    key: "ca".into(),
                    value: 120.
                }]
            )
            .is_err()
        );
    }
}
