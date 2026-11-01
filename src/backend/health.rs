//! Opt-in FM27 injury experiment. Public DTOs contain no native addresses.
//! The complete native removal helper performs cleanup; no list-end shortcut.
use super::*;
use crate::native_health;
use serde::Serialize;

const MAX_INJURIES: usize = 128;
const HELPER: u64 = 0x29C87C0;
// Digest-bound static source: reference/fm27-health-followup-20261101.md.
// Full helper entry plus every consequential call/store/return boundary.
const CODE: &[(u64, &str)] = &[
    (
        HELPER,
        "415741565657534883ec304889ce488b99f80000004885db7445",
    ),
    (
        0x29C87F0,
        "4889f9e8c89dbafe4983c6084d39fe7417498b3e4885ff74ef488b0f4885c974dfe82a607affebd8",
    ),
    (0x29C8818, "488b0348894308488b0dead4d202"),
    (0x29C8833, "4531c0e8f54343004885c0742066837814047519"),
    (0x29C885B, "e870c742000fb60daa6ad302fec966c7865c0200001027"),
    (
        0x29C887B,
        "488b0dae9ad302488b01c7442420010000004889f26641b810274531c9ff90e0020000",
    ),
    (
        0x29C88B6,
        "66c7865802000010276689865a020000488b0d3bd4d2024889f2e84bcc3f00",
    ),
    (
        0x29C88D5,
        "488b0d7472d302488b460848634004488d540608488b01ff9098000000488d0de769d302b2014883c4305b5f5e415e415fe985e804fe",
    ),
    (0x15725C0, "e9ab58c70a"),
    (
        0xC1E7E70,
        "415741565657534883ec408b05770452f98b15310951f9654c8b042558000000498b14d03b82b4040000",
    ),
    (
        0x216E840,
        "415741565657534883ec408b0587eb59038b15619f5803654c8b042558000000498b14d03b82b4040000",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct HealthSnapshot {
    holder: u64,
    holder_bytes: Vec<u8>,
    begin: u64,
    end: u64,
    entries: Vec<u64>,
    record_heads: Vec<Vec<u8>>,
    player_health: Vec<u8>,
    mode: u8,
    game_date: u32,
    managers: Vec<(u64, u64, u64)>,
}
#[derive(Clone, Debug)]
pub(crate) struct InjuryPlan {
    session: Session,
    entity: Entity,
    before: HealthSnapshot,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct InjuryPreview {
    pub player_id: u32,
    pub player_name: String,
    pub injury_count: usize,
    pub can_apply: bool,
    pub experimental: bool,
    pub reversible: bool,
    pub note: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct InjuryReceipt {
    pub journal_id: String,
    pub player_id: u32,
    pub injuries_before: usize,
    pub injuries_after: usize,
    pub verified: bool,
    pub experimental: bool,
    pub reversible: bool,
}
impl InjuryPlan {
    pub(crate) fn summary(&self) -> InjuryPreview {
        InjuryPreview {
            player_id: self.entity.id,
            player_name: self.entity.name.clone(),
            injury_count: self.before.entries.len(),
            can_apply: !self.before.entries.is_empty(),
            experimental: true,
            reversible: false,
            note: "시험 기능: FM27 자체 부상 제거 함수를 한 번 호출합니다. 게임 저장 백업이 필요하며 에디터 되돌리기는 제공하지 않습니다.",
        }
    }
    fn current(&self) -> Result<HealthSnapshot> {
        snapshot(&self.session, &self.entity)
    }
    fn check_current(&self) -> Result<()> {
        ensure!(
            self.current()? == self.before,
            "선수의 부상·건강 상태가 미리보기 후 변경됐습니다. 새로 조회하세요"
        );
        Ok(())
    }
}
fn pointer(value: u64) -> bool {
    (0x10000..0x0000_8000_0000_0000).contains(&value) && value % 8 == 0
}
fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("static hex"))
        .collect()
}
fn exact_profile(s: &Session) -> Result<()> {
    ensure!(
        s.module_stamp == profiles::FM27.timestamp
            && s.module_size == profiles::FM27.image_size
            && s.profile == profiles::FM27.label,
        "부상 제거 시험은 검증된 FM27 빌드에서만 사용할 수 있습니다"
    );
    Ok(())
}
fn verify_code_with(
    s: &Session,
    read: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<()> {
    exact_profile(s)?;
    for &(rva, hex) in CODE {
        let bytes = hex_bytes(hex);
        ensure!(
            rva + bytes.len() as u64 <= s.module_size as u64
                && read(s.module_base + rva, bytes.len())? == bytes,
            "FM27 부상 제거 네이티브 코드가 검증본과 다릅니다. 호출하지 않았습니다"
        );
    }
    Ok(())
}
fn word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("bounded word"))
}
fn capture_with(
    s: &Session,
    e: &Entity,
    read: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<HealthSnapshot> {
    let holder = word(&read(e.base + 0xF8, 8)?, 0);
    let (holder_bytes, begin, end, entries, record_heads) = if holder == 0 {
        (vec![], 0, 0, vec![], vec![])
    } else {
        ensure!(pointer(holder), "부상 상태 객체 포인터가 잘못되었습니다");
        let bytes = read(holder, 0x68)?;
        let begin = word(&bytes, 0);
        let end = word(&bytes, 8);
        ensure!(
            (begin == 0 && end == 0)
                || (pointer(begin)
                    && end >= begin
                    && end % 8 == 0
                    && (end - begin) / 8 <= MAX_INJURIES as u64),
            "부상 목록 범위가 검증 한도를 벗어납니다"
        );
        let entries = if begin == end {
            vec![]
        } else {
            read(begin, (end - begin) as usize)?
                .chunks_exact(8)
                .map(|b| word(b, 0))
                .collect::<Vec<_>>()
        };
        ensure!(
            entries.iter().all(|&p| pointer(p)),
            "부상 목록에 유효하지 않은 항목이 있습니다"
        );
        ensure!(
            entries.iter().copied().collect::<HashSet<_>>().len() == entries.len(),
            "부상 목록에 중복 항목이 있습니다"
        );
        // Native helper consumes [record] before releasing the record. This is
        // an identity guard, not a claim to decode the complete injury schema.
        let heads = entries
            .iter()
            .map(|&p| read(p, 8))
            .collect::<Result<Vec<_>>>()?;
        (bytes, begin, end, entries, heads)
    };
    let mode = read(s.module_base + 0x56FF311, 1)?[0];
    let mut globals = vec![0x56F5D10, 0x56F5D08, 0x56FFB50];
    if mode.wrapping_sub(1) >= 2 {
        globals.push(0x5702330);
    }
    let mut managers = vec![];
    for rva in globals {
        let object = word(&read(s.module_base + rva, 8)?, 0);
        ensure!(pointer(object), "FM 건강 관리자가 아직 준비되지 않았습니다");
        let vtable = word(&read(object, 8)?, 0);
        // Direct manager receivers need not have a polymorphic vtable; retain
        // their first word as an identity guard without inventing a type.
        if rva == 0x56FFB50 || rva == 0x5702330 {
            ensure!(
                vtable >= s.module_base && vtable < s.module_base + s.module_size as u64,
                "FM 건강 관리자 가상 테이블이 다릅니다"
            );
            let slot = if rva == 0x56FFB50 { 0x98 } else { 0x2E0 };
            let target = word(&read(vtable + slot, 8)?, 0);
            ensure!(
                target >= s.module_base && target < s.module_base + s.module_size as u64,
                "FM 건강 관리자 호출 대상이 다릅니다"
            );
            managers.push((rva, object, target));
        } else {
            managers.push((rva, object, vtable));
        }
    }
    Ok(HealthSnapshot {
        holder,
        holder_bytes,
        begin,
        end,
        entries,
        record_heads,
        player_health: read(e.base + 0x258, 6)?,
        mode,
        game_date: u32::from_le_bytes(
            read(s.module_base + 0x56AD378, 4)?
                .try_into()
                .expect("bounded date"),
        ),
        managers,
    })
}
fn stable_with(
    s: &Session,
    e: &Entity,
    read: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<HealthSnapshot> {
    verify_code_with(s, read)?;
    let before = capture_with(s, e, read)?;
    ensure!(
        capture_with(s, e, read)? == before,
        "부상 조회 중 상태가 변경됐습니다. 잠시 후 새로 조회하세요"
    );
    Ok(before)
}
fn snapshot(s: &Session, e: &Entity) -> Result<HealthSnapshot> {
    exact_profile(s)?;
    let p = Process::open(s.pid, false)?;
    validate_with(&p, s)?;
    validate_person(&p, s, e)?;
    let (offset, kind) = type_info(&p, s.module_base, s.module_size, p.u64(e.address)?)?;
    strict_player_identity(e, offset, &kind)?;
    let result = stable_with(s, e, &mut |a, n| p.read(a, n))?;
    validate_with(&p, s)?;
    validate_person(&p, s, e)?;
    Ok(result)
}
fn strict_player_identity(e: &Entity, offset: u32, kind: &str) -> Result<()> {
    ensure!(
        e.kind == EntityKind::Player
            && offset == 0x288
            && kind == ".?AVACTUAL_PLAYER@db@@"
            && e.address.checked_sub(0x288) == Some(e.base),
        "부상 제거 시험은 검증된 ACTUAL_PLAYER 객체만 지원합니다"
    );
    Ok(())
}
pub(crate) fn preview(s: &Session, e: &Entity) -> Result<InjuryPlan> {
    Ok(InjuryPlan {
        session: s.clone(),
        entity: e.clone(),
        before: snapshot(s, e)?,
    })
}
pub(crate) fn apply(plan: &InjuryPlan) -> Result<InjuryReceipt> {
    ensure!(!plan.before.entries.is_empty(), "제거할 부상이 없습니다");
    plan.check_current()?;
    let journal = native_health::remove_injuries(
        &plan.session,
        plan.entity.id,
        plan.entity.base,
        &serde_json::to_value(&plan.before)?,
        || plan.check_current(),
    )?;
    let after = match plan.current() {
        Ok(after) => after,
        Err(error) => {
            native_health::verification_failed(&journal, &format!("{error:#}"))?;
            return Err(error.context(
                "네이티브 호출은 완료됐으나 사후 검증에 실패했습니다. 자동 재시도하지 마세요",
            ));
        }
    };
    let words = [
        u16::from_le_bytes(after.player_health[0..2].try_into().unwrap()),
        u16::from_le_bytes(after.player_health[2..4].try_into().unwrap()),
        u16::from_le_bytes(after.player_health[4..6].try_into().unwrap()),
    ];
    let expected_middle = if after.mode.wrapping_sub(1) < 2 {
        0xFF06
    } else {
        0xFE0C
    };
    let mut expected_holder = plan.before.holder_bytes.clone();
    if !expected_holder.is_empty() {
        expected_holder[8..16].copy_from_slice(&plan.before.begin.to_le_bytes());
    }
    let verified = after.entries.is_empty()
        && after.holder == plan.before.holder
        && after.holder_bytes == expected_holder
        && after.mode == plan.before.mode
        && after.game_date == plan.before.game_date
        && after.managers == plan.before.managers
        && words == [0x2710, expected_middle, 0x2710];
    if !verified {
        native_health::verification_failed(&journal, "Injury list or native post-state differs")?;
        bail!("네이티브 호출 후 부상·건강 상태 검증이 일치하지 않습니다. 자동 재시도하지 마세요");
    }
    native_health::verified(&journal)?;
    Ok(InjuryReceipt {
        journal_id: journal,
        player_id: plan.entity.id,
        injuries_before: plan.before.entries.len(),
        injuries_after: 0,
        verified: true,
        experimental: true,
        reversible: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        Session {
            pid: 0,
            created: 1,
            module_base: 0x180000000,
            module_size: profiles::FM27.image_size,
            module_stamp: profiles::FM27.timestamp,
            db: 0x100000,
            registry_begin: 0x200000,
            game_date: "2026-07-25".into(),
            profile: profiles::FM27.label.into(),
            own_club_id: None,
        }
    }
    #[test]
    fn code_guard_rejects_other_build_and_any_changed_prefix() {
        let s = session();
        let mut reads = 0;
        assert!(
            verify_code_with(&s, &mut |a, n| {
                reads += 1;
                let (_, h) = CODE.iter().find(|(r, _)| s.module_base + *r == a).unwrap();
                let b = hex_bytes(h);
                assert_eq!(b.len(), n);
                Ok(b)
            })
            .is_ok()
        );
        assert_eq!(reads, CODE.len());
        assert!(verify_code_with(&s, &mut |_, n| Ok(vec![0; n])).is_err());
        for &(changed, _) in CODE {
            assert!(
                verify_code_with(&s, &mut |a, n| {
                    let (rva, hex) = CODE
                        .iter()
                        .find(|(rva, _)| s.module_base + *rva == a)
                        .unwrap();
                    let mut bytes = hex_bytes(hex);
                    assert_eq!(bytes.len(), n);
                    if *rva == changed {
                        bytes[0] ^= 1;
                    }
                    Ok(bytes)
                })
                .is_err(),
                "every guarded boundary must fail closed independently"
            );
        }
        let mut other = s;
        other.module_stamp += 1;
        assert!(verify_code_with(&other, &mut |_, _| panic!("must reject before read")).is_err());
    }
    #[test]
    fn safe_preview_never_serializes_native_state() {
        let preview = InjuryPreview {
            player_id: 42,
            player_name: "test".into(),
            injury_count: 1,
            can_apply: true,
            experimental: true,
            reversible: false,
            note: "test",
        };
        let v = serde_json::to_value(preview).unwrap();
        assert_eq!(v["injury_count"], 1);
        assert_eq!(v["can_apply"], true);
        assert!(v.get("base").is_none() && v.get("holder").is_none());
    }
    fn entity() -> Entity {
        serde_json::from_value(serde_json::json!({"id":42,"kind":"Player","name":"fixture","english_name":"fixture","club":"fixture","club_id":null,"age":20,"ca":100,"pa":100,"role":"GK","group":"fixture","address":0x300288u64,"base":0x300000u64,"index":0})).unwrap()
    }
    #[test]
    fn health_identity_requires_actual_player_base_not_registry_subobject_or_hybrid() {
        let mut e = entity();
        assert!(strict_player_identity(&e, 0x288, ".?AVACTUAL_PLAYER@db@@").is_ok());
        assert!(strict_player_identity(&e, 0x380, ".?AVACTUAL_PLAYER@db@@").is_err());
        assert!(strict_player_identity(&e, 0x288, ".?AVVIRTUAL_PLAYER@db@@").is_err());
        e.base = e.address;
        assert!(strict_player_identity(&e, 0x288, ".?AVACTUAL_PLAYER@db@@").is_err());
        e = entity();
        e.kind = EntityKind::Staff;
        assert!(strict_player_identity(&e, 0x288, ".?AVACTUAL_PLAYER@db@@").is_err());
    }
    fn fixture_memory(s: &Session) -> HashMap<u64, u8> {
        fn put(m: &mut HashMap<u64, u8>, at: u64, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                m.insert(at + i as u64, b);
            }
        }
        let mut m = HashMap::new();
        for &(rva, hex) in CODE {
            put(&mut m, s.module_base + rva, &hex_bytes(hex));
        }
        put(&mut m, 0x3000F8, &0x400000u64.to_le_bytes());
        let mut holder = vec![0; 0x68];
        holder[..8].copy_from_slice(&0x500000u64.to_le_bytes());
        holder[8..16].copy_from_slice(&0x500008u64.to_le_bytes());
        put(&mut m, 0x400000, &holder);
        put(&mut m, 0x500000, &0x600000u64.to_le_bytes());
        put(&mut m, 0x600000, &0x610000u64.to_le_bytes());
        put(&mut m, 0x300258, &[1, 2, 3, 4, 5, 6]);
        put(&mut m, s.module_base + 0x56FF311, &[1]);
        put(&mut m, s.module_base + 0x56AD378, &[1, 2, 3, 4]);
        for (rva, obj) in [
            (0x56F5D10, 0x700000u64),
            (0x56F5D08, 0x700100),
            (0x56FFB50, 0x700200),
        ] {
            put(&mut m, s.module_base + rva, &obj.to_le_bytes());
            put(&mut m, obj, &(s.module_base + 0x1000).to_le_bytes());
        }
        put(
            &mut m,
            s.module_base + 0x1098,
            &(s.module_base + 0x2000).to_le_bytes(),
        );
        m
    }
    fn read_fixture(m: &HashMap<u64, u8>, a: u64, n: usize) -> Result<Vec<u8>> {
        (a..a + n as u64)
            .map(|a| m.get(&a).copied().context("unmapped fixture byte"))
            .collect()
    }
    #[test]
    fn bounded_health_snapshot_rejects_changed_headers_null_records_and_excessive_lists() {
        let s = session();
        let e = entity();
        let original = fixture_memory(&s);
        let stable = stable_with(&s, &e, &mut |a, n| read_fixture(&original, a, n)).unwrap();
        assert_eq!(stable.entries.len(), 1);
        let mut read_count = 0;
        assert!(
            stable_with(&s, &e, &mut |a, n| {
                let mut b = read_fixture(&original, a, n)?;
                if a == 0x400000 {
                    read_count += 1;
                    if read_count == 2 {
                        b[8..16].copy_from_slice(&0x500000u64.to_le_bytes());
                    }
                }
                Ok(b)
            })
            .is_err()
        );
        let mut invalid = original.clone();
        for i in 0..8 {
            invalid.insert(0x500000 + i, 0);
        }
        assert!(capture_with(&s, &e, &mut |a, n| read_fixture(&invalid, a, n)).is_err());
        let mut invalid = original;
        for (i, &b) in (0x500000u64 + (MAX_INJURIES as u64 + 1) * 8)
            .to_le_bytes()
            .iter()
            .enumerate()
        {
            invalid.insert(0x400008 + i as u64, b);
        }
        assert!(capture_with(&s, &e, &mut |a, n| read_fixture(&invalid, a, n)).is_err());
    }
}
