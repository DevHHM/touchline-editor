//! Experimental official FM27 all-bans path. Addresses stay inside the plan.
use super::*;
use crate::{native_bans, native_health};
use serde::Serialize;
const MAX_RECORDS: usize = 128;
const CODE: &[(u64, &str)] = &[
    (
        0x15FB170,
        "415741565657534883ec20488bb1f80000004885f6745a4889cf4c8b76184c8b7e204d39fe751aeb3f0f1f80000000004889d9e808afb5014983c6084d39fe7418498b1e4885db74ef488b4b084885c974dee87936b700ebd7488b461848894620488bb7f8000000488b06483b4608740c4883c4205b5f5e415e415fc3488b4618483b462075ea48837e300075e348837e380075dc48837e480075d548837e500075ce48837e580075c748837e600075c048837e400075b948c787f8000000000000004889f1e8c51064024889f14883c4205b5f5e415e415fe902e10300",
    ),
    (0x31560B0, "e91bc8c710"),
    (0x1639350, "e97bceae0c"),
    (0x3C3C300, "e9fbc3d112"),
    (
        0x216E840,
        "415741565657534883ec408b0587eb59038b15619f5803654c8b042558000000498b14d03b82b4040000",
    ),
    (
        0x29CC23A,
        "4c89e1e82eefc2fe488b0d0739d302498b44240848634004498d540408488b01ff9098000000488d0d7930d302b201e822af04fe",
    ),
    (0xA17190, "4883ec3880797f0175200fb64130fec83c037216"),
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Snapshot {
    holder: u64,
    holder_bytes: Vec<u8>,
    bans: Vec<u64>,
    ban_heads: Vec<Vec<u8>>,
    injuries: Vec<u64>,
    injury_heads: Vec<Vec<u8>>,
    health: Vec<u8>,
    mode: u8,
    date: u32,
    adjustment_table: u64,
    adjustment: i32,
    interface: u64,
    interface_head: Vec<u8>,
    manager: u64,
    manager_vtable: u64,
    manager_target: u64,
    manager_prefix: Vec<u8>,
}
#[derive(Clone, Debug)]
pub(crate) struct BanPlan {
    session: Session,
    entity: Entity,
    before: Snapshot,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct BanPreview {
    pub player_id: u32,
    pub player_name: String,
    pub ban_count: usize,
    pub can_apply: bool,
    pub experimental: bool,
    pub reversible: bool,
    pub note: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct BanReceipt {
    pub journal_id: String,
    pub player_id: u32,
    pub bans_before: usize,
    pub bans_after: usize,
    pub verified: bool,
    pub experimental: bool,
    pub reversible: bool,
}
impl BanPlan {
    pub(crate) fn summary(&self) -> BanPreview {
        BanPreview {
            player_id: self.entity.id,
            player_name: self.entity.name.clone(),
            ban_count: self.before.bans.len(),
            can_apply: !self.before.bans.is_empty(),
            experimental: true,
            reversible: false,
            note: "시험 기능: FM27 자체 전체 출장 정지 제거와 후속 알림을 실행합니다. 저장 백업이 필요하며 에디터 되돌리기는 제공하지 않습니다.",
        }
    }
    fn current(&self) -> Result<Snapshot> {
        snapshot(&self.session, &self.entity)
    }
    fn check(&self) -> Result<()> {
        ensure!(
            self.current()? == self.before,
            "출장 정지·건강 상태가 미리보기 후 변경됐습니다. 다시 조회하세요"
        );
        Ok(())
    }
}
fn ptr(p: u64) -> bool {
    (0x10000..0x0000_8000_0000_0000).contains(&p) && p % 8 == 0
}
fn word(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().expect("bounded word"))
}
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("static hex"))
        .collect()
}
fn profile(s: &Session) -> Result<()> {
    ensure!(
        s.module_stamp == profiles::FM27.timestamp
            && s.module_size == profiles::FM27.image_size
            && s.profile == profiles::FM27.label,
        "검증된 FM27 출장 정지 시험 빌드가 아닙니다"
    );
    Ok(())
}
fn code_with(s: &Session, r: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>) -> Result<()> {
    profile(s)?;
    for &(a, h) in CODE {
        let b = hex(h);
        ensure!(
            a + b.len() as u64 <= s.module_size as u64 && r(s.module_base + a, b.len())? == b,
            "FM27 출장 정지 네이티브 코드가 검증본과 다릅니다. 호출하지 않았습니다"
        );
    }
    Ok(())
}
fn list(
    r: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>,
    h: &[u8],
    off: usize,
    head: usize,
) -> Result<(Vec<u64>, Vec<Vec<u8>>)> {
    let b = word(h, off);
    let e = word(h, off + 8);
    ensure!(
        (b == 0 && e == 0) || (ptr(b) && e >= b && e % 8 == 0 && (e - b) / 8 <= MAX_RECORDS as u64),
        "건강 목록 범위 오류"
    );
    let entries = if b == e {
        vec![]
    } else {
        r(b, (e - b) as usize)?
            .chunks_exact(8)
            .map(|x| word(x, 0))
            .collect::<Vec<_>>()
    };
    ensure!(
        entries.iter().all(|&a| ptr(a))
            && entries.iter().copied().collect::<HashSet<_>>().len() == entries.len(),
        "건강 목록에 잘못되거나 중복된 항목이 있습니다"
    );
    let heads = entries
        .iter()
        .map(|&a| r(a, head))
        .collect::<Result<Vec<_>>>()?;
    Ok((entries, heads))
}
fn capture(
    s: &Session,
    e: &Entity,
    r: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<Snapshot> {
    let holder = word(&r(e.base + 0xF8, 8)?, 0);
    let (holder_bytes, bans, ban_heads, injuries, injury_heads) = if holder == 0 {
        (vec![], vec![], vec![], vec![], vec![])
    } else {
        ensure!(ptr(holder), "건강 상태 객체 포인터 오류");
        let h = r(holder, 0x68)?;
        let (b, bh) = list(r, &h, 0x18, 16)?;
        let (i, ih) = list(r, &h, 0, 8)?;
        (h, b, bh, i, ih)
    };
    let adjustment_table = word(&r(e.base + 8, 8)?, 0);
    ensure!(
        adjustment_table >= s.module_base
            && adjustment_table + 8 < s.module_base + s.module_size as u64,
        "선수 인터페이스 조정 테이블 오류"
    );
    let adjustment = i32::from_le_bytes(
        r(adjustment_table + 4, 4)?
            .try_into()
            .expect("bounded adjustment"),
    );
    ensure!(
        (-4096..=4096).contains(&adjustment),
        "선수 인터페이스 조정 범위 오류"
    );
    let interface = e
        .base
        .checked_add_signed(adjustment as i64 + 8)
        .context("선수 인터페이스 오버플로")?;
    ensure!(ptr(interface), "선수 인터페이스 포인터 오류");
    let manager = word(&r(s.module_base + 0x56FFB50, 8)?, 0);
    ensure!(ptr(manager), "출장 정지 알림 관리자가 준비되지 않았습니다");
    let manager_vtable = word(&r(manager, 8)?, 0);
    ensure!(
        manager_vtable >= s.module_base
            && manager_vtable + 0xA0 < s.module_base + s.module_size as u64,
        "출장 정지 관리자 가상 테이블 오류"
    );
    let manager_target = word(&r(manager_vtable + 0x98, 8)?, 0);
    ensure!(
        manager_target >= s.module_base && manager_target < s.module_base + s.module_size as u64,
        "출장 정지 관리자 호출 대상 오류"
    );
    // Force a bounded read of the live call target before starting a worker.
    let manager_prefix = r(manager_target, 16)?;
    Ok(Snapshot {
        holder,
        holder_bytes,
        bans,
        ban_heads,
        injuries,
        injury_heads,
        health: r(e.base + 0x258, 6)?,
        mode: r(s.module_base + 0x56FF311, 1)?[0],
        date: u32::from_le_bytes(
            r(s.module_base + 0x56AD378, 4)?
                .try_into()
                .expect("bounded date"),
        ),
        adjustment_table,
        adjustment,
        interface,
        interface_head: r(interface, 16)?,
        manager,
        manager_vtable,
        manager_target,
        manager_prefix,
    })
}
fn stable(
    s: &Session,
    e: &Entity,
    r: &mut impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<Snapshot> {
    code_with(s, r)?;
    let a = capture(s, e, r)?;
    ensure!(
        capture(s, e, r)? == a,
        "출장 정지 조회 중 건강 상태가 변경됐습니다"
    );
    Ok(a)
}
fn snapshot(s: &Session, e: &Entity) -> Result<Snapshot> {
    profile(s)?;
    let p = Process::open(s.pid, false)?;
    validate_with(&p, s)?;
    validate_person(&p, s, e)?;
    let (off, kind) = type_info(&p, s.module_base, s.module_size, p.u64(e.address)?)?;
    ensure!(
        e.kind == EntityKind::Player
            && off == 0x288
            && kind == ".?AVACTUAL_PLAYER@db@@"
            && e.address.checked_sub(0x288) == Some(e.base),
        "출장 정지 시험은 검증된 ACTUAL_PLAYER 객체만 지원합니다"
    );
    let state = stable(s, e, &mut |a, n| p.read(a, n))?;
    validate_with(&p, s)?;
    validate_person(&p, s, e)?;
    Ok(state)
}
fn can_release_holder(s: &Snapshot) -> bool {
    !s.holder_bytes.is_empty()
        && s.injuries.is_empty()
        && [0x30, 0x38, 0x40, 0x48, 0x50, 0x58, 0x60]
            .iter()
            .all(|&i| word(&s.holder_bytes, i) == 0)
}
fn post_matches(before: &Snapshot, after: &Snapshot) -> bool {
    if !after.bans.is_empty()
        || !after.ban_heads.is_empty()
        || before.injuries != after.injuries
        || before.injury_heads != after.injury_heads
        || before.health != after.health
        || before.mode != after.mode
        || before.date != after.date
        || before.adjustment_table != after.adjustment_table
        || before.adjustment != after.adjustment
        || before.interface != after.interface
        || before.interface_head != after.interface_head
        || before.manager != after.manager
        || before.manager_vtable != after.manager_vtable
        || before.manager_target != after.manager_target
        || before.manager_prefix != after.manager_prefix
    {
        return false;
    }
    if can_release_holder(before) {
        return after.holder == 0 && after.holder_bytes.is_empty();
    }
    let mut h = before.holder_bytes.clone();
    if !h.is_empty() {
        let begin = word(&h, 0x18).to_le_bytes();
        h[0x20..0x28].copy_from_slice(&begin);
    }
    before.holder == after.holder && h == after.holder_bytes
}
pub(crate) fn preview(s: &Session, e: &Entity) -> Result<BanPlan> {
    Ok(BanPlan {
        session: s.clone(),
        entity: e.clone(),
        before: snapshot(s, e)?,
    })
}
pub(crate) fn apply(plan: &BanPlan) -> Result<BanReceipt> {
    ensure!(!plan.before.bans.is_empty(), "제거할 출장 정지가 없습니다");
    plan.check()?;
    let journal = native_bans::remove_bans(
        &plan.session,
        plan.entity.id,
        plan.entity.base,
        &serde_json::to_value(&plan.before)?,
        || plan.check(),
    )?;
    let after = match plan.current() {
        Ok(a) => a,
        Err(e) => {
            native_health::verification_failed(&journal, &format!("{e:#}"))?;
            return Err(e.context(
                "네이티브 호출은 완료됐으나 사후 검증이 실패했습니다. 자동 재시도하지 마세요",
            ));
        }
    };
    if !post_matches(&plan.before, &after) {
        native_health::verification_failed(
            &journal,
            "Ban removal or preservation post-state differs",
        )?;
        bail!("출장 정지 제거 후 검증이 일치하지 않습니다. 자동 재시도하지 마세요");
    }
    native_health::verified(&journal)?;
    Ok(BanReceipt {
        journal_id: journal,
        player_id: plan.entity.id,
        bans_before: plan.before.bans.len(),
        bans_after: 0,
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
            db: 0,
            registry_begin: 0,
            game_date: "fixture".into(),
            profile: profiles::FM27.label.into(),
            own_club_id: None,
        }
    }
    #[test]
    fn every_native_ban_boundary_rejects_independent_mutation() {
        let s = session();
        for &(changed, _) in CODE {
            assert!(
                code_with(&s, &mut |a, n| {
                    let (r, h) = CODE.iter().find(|(r, _)| s.module_base + *r == a).unwrap();
                    let mut b = hex(h);
                    assert_eq!(n, b.len());
                    if *r == changed {
                        b[0] ^= 1;
                    }
                    Ok(b)
                })
                .is_err()
            );
        }
        let mut other = s;
        other.module_stamp += 1;
        assert!(code_with(&other, &mut |_, _| panic!("reject first")).is_err());
    }
    fn state() -> Snapshot {
        let mut h = vec![0; 0x68];
        h[0x18..0x20].copy_from_slice(&0x500000u64.to_le_bytes());
        h[0x20..0x28].copy_from_slice(&0x500008u64.to_le_bytes());
        Snapshot {
            holder: 0x400000,
            holder_bytes: h,
            bans: vec![0x600000],
            ban_heads: vec![vec![0; 16]],
            injuries: vec![],
            injury_heads: vec![],
            health: vec![1; 6],
            mode: 1,
            date: 10,
            adjustment_table: 0x180001000,
            adjustment: 0x280,
            interface: 0x300288,
            interface_head: vec![0; 16],
            manager: 0x700000,
            manager_vtable: 0x180002000,
            manager_target: 0x180003000,
            manager_prefix: vec![0; 16],
        }
    }
    #[test]
    fn postcheck_allows_native_holder_release_only_when_eligible_and_preserves_injuries() {
        let before = state();
        let mut after = before.clone();
        after.bans.clear();
        after.ban_heads.clear();
        after.holder = 0;
        after.holder_bytes.clear();
        assert!(post_matches(&before, &after));
        let mut held = before.clone();
        held.injuries = vec![0x610000];
        held.injury_heads = vec![vec![9; 8]];
        held.holder_bytes[0x30] = 1;
        let mut after = held.clone();
        after.bans.clear();
        after.ban_heads.clear();
        after.ban_heads.clear();
        after.holder_bytes[0x20..0x28].copy_from_slice(&0x500000u64.to_le_bytes());
        assert!(post_matches(&held, &after));
        after.health[0] ^= 1;
        assert!(!post_matches(&held, &after));
        after = held.clone();
        after.bans.clear();
        after.injuries.clear();
        assert!(!post_matches(&held, &after));
    }
    #[test]
    fn ban_summary_contains_count_not_native_addresses() {
        let p = BanPreview {
            player_id: 1,
            player_name: "test".into(),
            ban_count: 1,
            can_apply: true,
            experimental: true,
            reversible: false,
            note: "test",
        };
        let j = serde_json::to_value(p).unwrap();
        assert_eq!(j["ban_count"], 1);
        assert!(j.get("holder").is_none() && j.get("address").is_none());
    }
}
