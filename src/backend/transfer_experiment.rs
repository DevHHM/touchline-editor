//! Opt-in, one-player, zero-amount native transfer. Never edits affiliation pointers.
use super::*;
use crate::{native_health, native_transfer};
use serde::Serialize;

const MANAGER: u64 = 0x56F1A18;
const MAX_TEAMS: usize = 64;
const MAX_ROSTER: usize = 2048;
const CODE: &[(u64, &str)] = &[
    (0x1A4BFC0, "4157415641554154565755534881ec880000004989d5"),
    (
        0xBAC8D0,
        "5541574156415541545657534881ecd8000000488dac2480000000",
    ),
    (
        0xC05A89,
        "c644244800c644244000c644243800c644243000c64424280048c744242000000000",
    ),
    (
        0xC05ABA,
        "488b0d57bfae04895c2420c64424400048c744243800000000c644243000c744242801006c074889fa4d89f04989f1e8e26dfaff",
    ),
    (0xBACE22, "bad0000000e894a25aff4989c64889c1e8e981a802"),
    (0xBACF8E, "c644242000488d55084c89e941b0014531c9e84b72f2ff"),
    (0xBAD09B, "4c8d45284c8d4df04c89e9e80545f9ff"),
    (0x1CAF3B7, "85db0f881a010000"),
    (0x1CAF403, "895e58"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Bytes {
    address: u64,
    bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Snapshot {
    person: u64,
    contract: u64,
    current_team: u64,
    current_club: u64,
    source_club: u64,
    source_id: u32,
    destination_club: u64,
    destination_id: u32,
    destination_team: u64,
    source_members: usize,
    destination_members: usize,
    manager: u64,
    game_date: u32,
    guards: Vec<Bytes>,
}
#[derive(Clone, Debug)]
pub(crate) struct TransferPlan {
    session: Session,
    player: Entity,
    destination: Entity,
    before: Snapshot,
    source_name: String,
}
#[derive(Serialize)]
pub(crate) struct TransferSummary {
    player_id: u32,
    player_name: String,
    source_club_id: u32,
    source_club_name: String,
    destination_club_id: u32,
    destination_club_name: String,
    fee: u32,
    can_apply: bool,
    experimental: bool,
    reversible: bool,
}
#[derive(Serialize)]
pub(crate) struct TransferReceipt {
    journal_id: String,
    player_id: u32,
    source_club_id: u32,
    destination_club_id: u32,
    fee: u32,
    source_members_after: usize,
    destination_members_after: usize,
    verified: bool,
    experimental: bool,
    reversible: bool,
}
fn ptr(v: u64) -> Result<u64> {
    ensure!(
        (0x10000..0x0000_8000_0000_0000).contains(&v) && v % 8 == 0,
        "이적 시험 객체 주소가 잘못됐습니다"
    );
    Ok(v)
}
fn adjustment_table(session: &Session, table: u64) -> Result<u64> {
    // MSVC vbtable entries are signed DWORD displacements, not object pointers.
    // The native caller reads dword [vbtable+4]; actual ACTUAL_PLAYER tables can
    // be 4 mod 8. Keep all heap/object pointers under the stricter ptr() guard.
    let module_end = session
        .module_base
        .checked_add(session.module_size as u64)
        .context("선수 가상기저 변위표 모듈 범위 오류")?;
    ensure!(
        table % 4 == 0
            && table >= session.module_base
            && table.checked_add(8).is_some_and(|end| end <= module_end),
        "선수 가상기저 변위표가 모듈의 DWORD 범위가 아닙니다"
    );
    Ok(table)
}
fn q(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn u(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn hex(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}
struct Capture<'a> {
    read: &'a dyn Fn(u64, usize) -> Result<Vec<u8>>,
    info: &'a dyn Fn(u64) -> Result<(u32, String)>,
    guards: Vec<Bytes>,
}
impl Capture<'_> {
    fn bytes(&mut self, a: u64, n: usize) -> Result<Vec<u8>> {
        ensure!(n > 0 && n <= MAX_ROSTER * 8, "이적 시험 읽기 범위 초과");
        let bytes = (self.read)(a, n)?;
        ensure!(bytes.len() == n, "이적 시험 읽기 크기 불일치");
        self.guards.push(Bytes {
            address: a,
            bytes: bytes.clone(),
        });
        Ok(bytes)
    }
    fn word(&mut self, a: u64) -> Result<u64> {
        Ok(q(&self.bytes(a, 8)?, 0))
    }
    fn object(&mut self, a: u64, name: &str) -> Result<u32> {
        ptr(a).with_context(|| format!("이적 시험 {name} 객체 주소 검증 실패"))?;
        let b = self.bytes(a, 16)?;
        let (offset, kind) = (self.info)(q(&b, 0))?;
        ensure!(
            offset == 0 && kind == name,
            "이적 시험 객체 형식 불일치: {name}"
        );
        let id = u(&b, 12);
        ensure!(id > 0 && id < 0x8000_0000, "이적 시험 객체 식별자 오류");
        Ok(id)
    }
    fn vector(&mut self, a: u64, max: usize) -> Result<Vec<u64>> {
        let b = self.bytes(a, 16)?;
        let begin = q(&b, 0);
        let end = q(&b, 8);
        let len = bounded_vector(begin, end, max).context("이적 시험 명단 벡터 검증 실패")?;
        if len == 0 {
            return Ok(vec![]);
        }
        Ok(self
            .bytes(begin, len * 8)?
            .chunks_exact(8)
            .map(|b| q(b, 0))
            .collect())
    }
    fn team(&mut self, t: u64, club: u64) -> Result<u8> {
        self.object(t, ".?AVTEAM@db@@")?;
        let b = self.bytes(t + 0x28, 16)?;
        ensure!(
            b[7] == 0 && q(&b, 8) == club,
            "일반 구단 TEAM 연결이 아닙니다"
        );
        Ok(b[0])
    }
    fn club_roster(&mut self, club: u64, player: u64) -> Result<(usize, Vec<u64>)> {
        let teams = self.vector(club + 0x18, MAX_TEAMS)?;
        ensure!(
            !teams.is_empty() && teams.iter().copied().collect::<HashSet<_>>().len() == teams.len(),
            "구단 TEAM 목록이 비었거나 중복됐습니다"
        );
        let mut count = 0;
        let mut senior = vec![];
        for team in teams {
            let kind = self.team(team, club)?;
            if kind == 0 {
                senior.push(team);
            }
            let mut unique = HashSet::new();
            for member in self.vector(team + 0x38, MAX_ROSTER)? {
                ptr(member).context("이적 시험 직접 선수 명단 항목 주소 검증 실패")?;
                let vt = self.word(member)?;
                let (offset, typ) = (self.info)(vt)?;
                // No scanning wrapper fields or treating opaque roster entries as people.
                ensure!(
                    typ == ".?AVACTUAL_PLAYER@db@@" && matches!(offset, 0 | 0x288),
                    "이적 시험은 직접 선수 포인터 명단만 지원합니다"
                );
                let base = member
                    .checked_sub(offset as u64)
                    .context("선수 명단 객체 오프셋 오류")?;
                ensure!(unique.insert(base), "선수 명단에 중복 선수가 있습니다");
                if base == player {
                    count += 1;
                }
            }
        }
        Ok((count, senior))
    }
    fn verify(&self) -> Result<()> {
        for g in &self.guards {
            ensure!(
                (self.read)(g.address, g.bytes.len())? == g.bytes,
                "이적 정보를 읽는 동안 게임 상태가 변경됐습니다"
            );
        }
        Ok(())
    }
}
fn bounded_vector(begin: u64, end: u64, max: usize) -> Result<usize> {
    if begin == 0 && end == 0 {
        return Ok(0);
    }
    ptr(begin)?;
    ensure!(
        end >= begin && end % 8 == 0 && (end - begin) / 8 <= max as u64,
        "이적 시험 벡터 범위 오류"
    );
    Ok(((end - begin) / 8) as usize)
}
fn profile(s: &Session) -> Result<()> {
    ensure!(
        s.profile == profiles::FM27.label
            && s.module_stamp == profiles::FM27.timestamp
            && s.module_size == profiles::FM27.image_size,
        "이적 시험은 검증된 FM27 빌드만 지원합니다"
    );
    Ok(())
}
fn capture_with(
    s: &Session,
    e: &Entity,
    destination: &Entity,
    source: Option<(u64, u32)>,
    read: &dyn Fn(u64, usize) -> Result<Vec<u8>>,
    info: &dyn Fn(u64) -> Result<(u32, String)>,
) -> Result<Snapshot> {
    profile(s)?;
    ensure!(
        e.kind == EntityKind::Player && destination.kind == EntityKind::Club,
        "선수와 목적 구단이 필요합니다"
    );
    let mut c = Capture {
        read,
        info,
        guards: vec![],
    };
    for &(rva, h) in CODE {
        ensure!(
            c.bytes(s.module_base + rva, h.len() / 2)? == hex(h),
            "FM27 이적 코드가 검증본과 다릅니다"
        );
    }
    let (off, typ) = info(c.word(e.address)?)?;
    ensure!(
        off == 0x288
            && typ == ".?AVACTUAL_PLAYER@db@@"
            && e.address.checked_sub(0x288) == Some(e.base),
        "일반 ACTUAL_PLAYER만 이적 시험을 지원합니다"
    );
    let head = c.bytes(e.address, 16)?;
    ensure!(u(&head, 12) == e.id, "선수 ID가 변경됐습니다");
    let ix = e.index.context("선수 레지스트리 인덱스가 없습니다")?;
    ensure!(
        u(&head, 8) == ix && c.word(s.registry_begin + ix as u64 * 8)? == e.address,
        "선수 레지스트리 연결이 변경됐습니다"
    );
    let vb = adjustment_table(
        s,
        c.word(e.base + 8)
            .context("선수 가상기저 변위표 포인터 읽기 실패")?,
    )?;
    let displacement = i32::from_le_bytes(c.bytes(vb + 4, 4)?.try_into().unwrap());
    let person = e.base as i128 + 8 + displacement as i128;
    ensure!(
        person == e.address as i128,
        "네이티브 Person 변환이 인물 주소와 다릅니다"
    );
    let contract = ptr(c.word(e.address + 0xA8)?).context("선수 주계약 객체 주소 검증 실패")?;
    let (offset, kind) = info(c.word(contract)?)?;
    ensure!(
        offset == 0 && kind == ".?AVFULL_CONTRACT@db@@",
        "기존 FULL_CONTRACT가 필요합니다"
    );
    let raw = c.bytes(contract, 0xC8)?;
    ensure!(q(&raw, 8) == e.address, "주계약의 선수 연결이 다릅니다");
    let current_team = ptr(q(&raw, 0x10)).context("현재 주계약 TEAM 주소 검증 실패")?;
    let loan_holder = c.word(e.address + 0xB0)?;
    if loan_holder != 0 {
        ptr(loan_holder).context("선수 임대/추가 계약 holder 주소 검증 실패")?;
        ensure!(
            c.word(loan_holder)? == 0,
            "임대/추가 계약이 있는 선수는 이번 시험에서 제외합니다"
        );
    }
    c.object(current_team, ".?AVTEAM@db@@")?;
    let current_club =
        ptr(c.word(current_team + 0x30)?).context("현재 TEAM의 원 구단 주소 검증 실패")?;
    let current_id = c.object(current_club, ".?AVCLUB@db@@")?;
    let (source_club, source_id) = source.unwrap_or((current_club, current_id));
    ensure!(
        c.object(source_club, ".?AVCLUB@db@@")? == source_id,
        "원 구단 식별자가 변경됐습니다"
    );
    let destination_id = c.object(destination.address, ".?AVCLUB@db@@")?;
    ensure!(
        destination_id == destination.id && destination.address != source_club,
        "다른 목적 구단을 지정해야 합니다"
    );
    let (source_members, _) = c.club_roster(source_club, e.base)?;
    let (destination_members, senior) = c.club_roster(destination.address, e.base)?;
    ensure!(
        senior.len() == 1,
        "목적 구단의 일반 1군 TEAM을 유일하게 확인할 수 없습니다"
    );
    let manager =
        ptr(c.word(s.module_base + MANAGER)?).context("네이티브 이적 관리자 주소 검증 실패")?;
    c.bytes(manager, 8)?;
    // Native lookup receiver M+0x18 reads its offer pointer vector at +8/+16.
    let manager_vector = c.bytes(manager + 0x20, 16)?;
    bounded_vector(q(&manager_vector, 0), q(&manager_vector, 8), 1_000_000)
        .context("이적 관리자 offer 벡터 검증 실패")?;
    let game_date = u(&c.bytes(s.module_base + 0x56AD378, 4)?, 0);
    ensure!(decode_date(game_date).is_some(), "게임 날짜 검증 실패");
    c.verify()?;
    Ok(Snapshot {
        person: e.address,
        contract,
        current_team,
        current_club,
        source_club,
        source_id,
        destination_club: destination.address,
        destination_id,
        destination_team: senior[0],
        source_members,
        destination_members,
        manager,
        game_date,
        guards: c.guards,
    })
}
fn capture(
    s: &Session,
    e: &Entity,
    destination: &Entity,
    source: Option<(u64, u32)>,
) -> Result<Snapshot> {
    let p = Process::open(s.pid, false)?;
    validate_with(&p, s)?;
    validate_person(&p, s, e)?;
    let result = capture_with(s, e, destination, source, &|a, n| p.read(a, n), &|v| {
        type_info(&p, s.module_base, s.module_size, v)
    })?;
    validate_with(&p, s)?;
    validate_person(&p, s, e)?;
    Ok(result)
}
impl TransferPlan {
    pub(crate) fn summary(&self) -> TransferSummary {
        TransferSummary {
            player_id: self.player.id,
            player_name: self.player.name.clone(),
            source_club_id: self.before.source_id,
            source_club_name: self.source_name.clone(),
            destination_club_id: self.destination.id,
            destination_club_name: self.destination.name.clone(),
            fee: 0,
            can_apply: true,
            experimental: true,
            reversible: false,
        }
    }
    fn current(&self) -> Result<Snapshot> {
        capture(
            &self.session,
            &self.player,
            &self.destination,
            Some((self.before.source_club, self.before.source_id)),
        )
    }
    fn check_current(&self) -> Result<()> {
        ensure!(
            self.current()? == self.before,
            "이적 미리보기 후 선수·계약·양 구단 명단이 변경됐습니다"
        );
        Ok(())
    }
}
pub(crate) fn preview(
    s: &Session,
    player: &Entity,
    destination_club: &Entity,
) -> Result<TransferPlan> {
    let before = capture(s, player, destination_club, None)?;
    validate_initial(&before)?;
    let source_name = club_name(&Process::open(s.pid, false)?, before.source_club);
    let plan = TransferPlan {
        session: s.clone(),
        player: player.clone(),
        destination: destination_club.clone(),
        before,
        source_name,
    };
    plan.check_current()?;
    Ok(plan)
}
fn validate_initial(before: &Snapshot) -> Result<()> {
    ensure!(
        before.current_club == before.source_club
            && before.source_members == 1
            && before.destination_members == 0,
        "원 구단에 정확히 한 번 소속된 선수만 이적 시험을 지원합니다"
    );
    Ok(())
}
pub(crate) fn apply(plan: &TransferPlan) -> Result<TransferReceipt> {
    plan.check_current()?;
    let journal = native_transfer::apply(
        &plan.session,
        plan.player.id,
        plan.player.base,
        plan.before.person,
        plan.before.manager,
        plan.before.destination_team,
        &serde_json::to_value(&plan.before)?,
        || plan.check_current(),
    )?;
    let after = match plan.current() {
        Ok(s) => s,
        Err(e) => {
            native_health::verification_failed(&journal, &format!("{e:#}"))?;
            return Err(e.context("이적 네이티브 호출 후 검증 실패. 자동 재시도하지 마세요"));
        }
    };
    let valid = post_state(&plan.before, &after);
    if !valid {
        native_health::verification_failed(
            &journal,
            "Active contract and both roster memberships did not certify completion",
        )?;
        bail!(
            "이적 완료를 계약과 양 구단 명단에서 확인하지 못했습니다. 자동 재시도하지 마세요 (기록 {journal})"
        );
    }
    native_health::verified(&journal)?;
    Ok(TransferReceipt {
        journal_id: journal,
        player_id: plan.player.id,
        source_club_id: plan.before.source_id,
        destination_club_id: plan.before.destination_id,
        fee: 0,
        source_members_after: after.source_members,
        destination_members_after: after.destination_members,
        verified: true,
        experimental: true,
        reversible: false,
    })
}
fn post_state(before: &Snapshot, after: &Snapshot) -> bool {
    after.current_club == before.destination_club
        && after.current_team == before.destination_team
        && after.source_members == 0
        && after.destination_members == 1
        && after.source_club == before.source_club
        && after.source_id == before.source_id
        && after.destination_club == before.destination_club
        && after.destination_id == before.destination_id
        && after.destination_team == before.destination_team
        && after.manager == before.manager
        && after.person == before.person
        && after.game_date == before.game_date
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_and_excessive_rosters_fail_before_target_reads() {
        assert_eq!(bounded_vector(0, 0, 10).unwrap(), 0);
        for (b, e) in [
            (0, 0x10000),
            (1, 1),
            (0x20000, 0x10000),
            (0x20000, 0x20001),
            (0x20000, 0x20058),
        ] {
            assert!(bounded_vector(b, e, 10).is_err());
        }
        assert_eq!(bounded_vector(0x20000, 0x20050, 10).unwrap(), 10);
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            person: 0x30288,
            contract: 0x40000,
            current_team: 0x50000,
            current_club: 0x60000,
            source_club: 0x60000,
            source_id: 1,
            destination_club: 0x70000,
            destination_id: 2,
            destination_team: 0x80000,
            source_members: 1,
            destination_members: 0,
            manager: 0x90000,
            game_date: 1,
            guards: vec![],
        }
    }
    #[test]
    fn completion_requires_both_rosters_and_active_contract_not_one_pointer() {
        let before = snapshot();
        let mut after = before.clone();
        assert!(!post_state(&before, &after));
        after.current_club = before.destination_club;
        after.current_team = before.destination_team;
        assert!(!post_state(&before, &after));
        after.destination_members = 1;
        assert!(!post_state(&before, &after));
        after.source_members = 0;
        assert!(post_state(&before, &after));
        after.destination_members = 2;
        assert!(!post_state(&before, &after));
        after.destination_members = 1;
        after.game_date += 1;
        assert!(!post_state(&before, &after));
    }
    #[test]
    fn retained_guard_change_and_partial_read_fail_closed() {
        let mut c = Capture {
            read: &|_, n| Ok(vec![0; n]),
            info: &|_| bail!("unused"),
            guards: vec![],
        };
        c.bytes(0x10000, 16).unwrap();
        assert!(c.verify().is_ok());
        c.guards[0].bytes[0] = 1;
        assert!(c.verify().is_err());
        let mut c = Capture {
            read: &|_, _| Ok(vec![0]),
            info: &|_| bail!("unused"),
            guards: vec![],
        };
        assert!(c.bytes(0x10000, 8).is_err());
    }
    fn fixture() -> (Session, Entity, Entity, HashMap<u64, u8>) {
        fn put(m: &mut HashMap<u64, u8>, a: u64, b: &[u8]) {
            for (i, &byte) in b.iter().enumerate() {
                m.insert(a + i as u64, byte);
            }
        }
        fn object(m: &mut HashMap<u64, u8>, a: u64, vt: u64, id: u32) {
            let mut b = [0; 16];
            b[..8].copy_from_slice(&vt.to_le_bytes());
            b[12..16].copy_from_slice(&id.to_le_bytes());
            put(m, a, &b);
        }
        let s = Session {
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
        };
        let entity = |id, kind, address, base| {
            serde_json::from_value::<Entity>(serde_json::json!({"id":id,"kind":kind,"name":"fixture","english_name":"fixture",
            "club":"fixture","club_id":null,"age":20,"ca":100,"pa":100,"role":"GK","group":"fixture","address":address,"base":base,"index":0})).unwrap()
        };
        let e = entity(42, "Player", 0x300288u64, 0x300000u64);
        let d = entity(2, "Club", 0x510000u64, 0x510000u64);
        let mut m = HashMap::new();
        for &(r, h) in CODE {
            put(&mut m, s.module_base + r, &hex(h));
        }
        object(&mut m, e.address, 0x10000, 42);
        put(&mut m, s.registry_begin, &e.address.to_le_bytes());
        put(&mut m, e.base, &0x11000u64.to_le_bytes());
        put(&mut m, e.base + 8, &(s.module_base + 0x2004).to_le_bytes());
        put(&mut m, s.module_base + 0x2008, &0x280i32.to_le_bytes());
        put(&mut m, e.address + 0xA8, &0x400000u64.to_le_bytes());
        put(&mut m, e.address + 0xB0, &0u64.to_le_bytes());
        put(&mut m, 0x400000, &[0; 0xC8]);
        put(&mut m, 0x400000, &0x13000u64.to_le_bytes());
        put(&mut m, 0x400008, &e.address.to_le_bytes());
        put(&mut m, 0x400010, &0x600000u64.to_le_bytes());
        for (club, id, array, team) in [
            (0x500000, 1, 0x800000u64, 0x600000u64),
            (0x510000, 2, 0x820000, 0x610000),
        ] {
            object(&mut m, club, 0x14000, id);
            put(
                &mut m,
                club + 0x18,
                &[array.to_le_bytes(), (array + 8).to_le_bytes()].concat(),
            );
            put(&mut m, array, &team.to_le_bytes());
            object(&mut m, team, 0x15000, id + 10);
            let mut b = [0; 16];
            b[8..16].copy_from_slice(&club.to_le_bytes());
            put(&mut m, team + 0x28, &b);
            let (begin, end) = if id == 1 {
                (0x810000u64, 0x810008u64)
            } else {
                (0, 0)
            };
            put(
                &mut m,
                team + 0x38,
                &[begin.to_le_bytes(), end.to_le_bytes()].concat(),
            );
        }
        put(&mut m, 0x810000, &e.address.to_le_bytes());
        put(&mut m, s.module_base + MANAGER, &0x700000u64.to_le_bytes());
        put(&mut m, 0x700000, &[0; 8]);
        put(&mut m, 0x700020, &[0; 16]);
        put(
            &mut m,
            s.module_base + 0x56AD378,
            &0x07EA00CEu32.to_le_bytes(),
        );
        (s, e, d, m)
    }
    fn fixture_read(m: &HashMap<u64, u8>, a: u64, n: usize) -> Result<Vec<u8>> {
        (a..a + n as u64)
            .map(|a| m.get(&a).copied().context("unmapped fixture byte"))
            .collect()
    }
    fn fixture_type(v: u64) -> Result<(u32, String)> {
        Ok(match v {
            0x10000 => (0x288, ".?AVACTUAL_PLAYER@db@@".into()),
            0x11000 => (0, ".?AVACTUAL_PLAYER@db@@".into()),
            0x13000 => (0, ".?AVFULL_CONTRACT@db@@".into()),
            0x14000 => (0, ".?AVCLUB@db@@".into()),
            0x15000 => (0, ".?AVTEAM@db@@".into()),
            _ => bail!("opaque fixture entry"),
        })
    }
    #[test]
    fn full_snapshot_accepts_direct_roster_and_rejects_wrappers_loans_backlinks_and_code_changes() {
        let (s, e, d, m) = fixture();
        let take = |m: &HashMap<u64, u8>| {
            capture_with(
                &s,
                &e,
                &d,
                None,
                &|a, n| fixture_read(m, a, n),
                &fixture_type,
            )
        };
        let before = take(&m).unwrap();
        validate_initial(&before).unwrap();
        assert_eq!(before.destination_team, 0x610000);
        assert_eq!(before.source_id, 1);
        for (a, b) in [
            (0x810000, 0x900000u64.to_le_bytes().to_vec()),
            (e.address + 0xB0, 0x900000u64.to_le_bytes().to_vec()),
            (0x400008, 0x900000u64.to_le_bytes().to_vec()),
            (s.module_base + CODE[0].0, vec![0]),
        ] {
            let mut changed = m.clone();
            for (i, v) in b.into_iter().enumerate() {
                changed.insert(a + i as u64, v);
            }
            assert!(take(&changed).is_err());
        }
        let mut empty = m;
        for (i, v) in 0x810000u64.to_le_bytes().into_iter().enumerate() {
            empty.insert(0x600040 + i as u64, v);
        }
        assert!(validate_initial(&take(&empty).unwrap()).is_err());
    }
    #[test]
    fn dword_adjustment_tables_allow_four_mod_eight_only_inside_module_without_loosening_objects() {
        let (s, _, _, _) = fixture();
        assert!(adjustment_table(&s, s.module_base + 0x2004).is_ok());
        assert!(adjustment_table(&s, s.module_base + 0x2002).is_err());
        assert!(adjustment_table(&s, s.module_base - 4).is_err());
        assert!(adjustment_table(&s, s.module_base + s.module_size as u64 - 4).is_err());
        assert!(adjustment_table(&s, u64::MAX - 3).is_err());
        assert!(
            ptr(s.module_base + 0x2004).is_err(),
            "object guard must retain eight-byte alignment"
        );
    }
}
