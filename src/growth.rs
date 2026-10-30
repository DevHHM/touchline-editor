//! Read-only FM26 growth reference, through the known facility ceiling only.
//! Evidence: reference/fm-native-lifecycle/youth-competition-fields-followup.json
//! and youth-development-decompile/team-followup. This is not a CA forecast.
use crate::{
    backend,
    memory::Process,
    model::{Entity, EntityKind, Session},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const FORMULA_VERSION: &str = "fm26-6a229e8a-growth-reference-v1";
pub const SCOPE: &str = "현재 팀·대회와 확인된 시설 보정까지 반영한 성장 참고치입니다. 최종 CA 예측이나 성장 한도가 아니며, 이후 국가·선수·훈련·확률·CA/PA 보정은 포함하지 않습니다. 현재 구단의 실제 선수단만 조회하며 외부 구단에 임대 중인 선수는 제외됩니다.";
const MAX_TEAMS: usize = 64;
const MAX_ROSTER: usize = 512;
const MAX_PLAYERS: usize = 4096;
const MAX_LINKS: usize = 256;

#[derive(Clone, Debug, Serialize)]
pub struct ClubGrowth {
    pub club_id: u32,
    pub formula_version: &'static str,
    pub scope: &'static str,
    pub game_date: String,
    pub items: Vec<GrowthItem>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct GrowthItem {
    pub player_id: u32,
    pub status: &'static str,
    pub reference_value: Option<i32>,
    pub base_reference_value: Option<i32>,
    pub current_ca: Option<u16>,
    pub potential_ca: Option<u16>,
    pub current_age: Option<u32>,
    pub person_index: Option<u32>,
    pub reference_gap: Option<i32>,
    pub reason: String,
    pub selected_team: Option<String>,
    pub actual_team: Option<String>,
    pub source_team_kind: Option<String>,
    pub team_reputation: Option<u16>,
    pub competition_reputation: Option<u16>,
    pub denominator: Option<u16>,
    pub age_reference: Option<u16>,
    pub native_facility_value: Option<i8>,
    pub facility_ceiling: Option<i32>,
    pub calculation_note: String,
}
impl GrowthItem {
    fn unknown(id: u32) -> Self {
        Self {
            player_id: id,
            status: "unknown",
            reference_value: None,
            base_reference_value: None,
            current_ca: None,
            potential_ca: None,
            current_age: None,
            person_index: None,
            reference_gap: None,
            reason: String::new(),
            selected_team: None,
            actual_team: None,
            source_team_kind: None,
            team_reputation: None,
            competition_reputation: None,
            denominator: None,
            age_reference: None,
            native_facility_value: None,
            facility_ceiling: None,
            calculation_note: SCOPE.into(),
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Inputs {
    ca: u16,
    pa: u16,
    age: u32,
    person_index: u32,
    team_reputation: u16,
    competition_reputation: u16,
    denominator: u16,
    facility: Option<i8>,
}
#[derive(Debug, PartialEq, Eq)]
struct Calculation {
    age_reference: u16,
    base: i32,
    value: i32,
    gap: i32,
    ceiling: Option<i32>,
}
fn calculate(x: Inputs) -> Result<Calculation> {
    ensure!(
        (1..=200).contains(&x.ca) && (1..=200).contains(&x.pa),
        "CA/PA 범위 미확인"
    );
    ensure!(
        x.ca <= x.pa,
        "현재 CA가 PA보다 높아 이 경로를 확정할 수 없습니다"
    );
    ensure!((14..=100).contains(&x.age), "나이 범위 미확인");
    ensure!(x.person_index < 2_000_000, "인물 인덱스 범위 미확인");
    ensure!(
        x.team_reputation <= 10000 && x.competition_reputation <= 200,
        "팀·대회 평판 범위 미확인"
    );
    ensure!(matches!(x.denominator, 80 | 105 | 115), "분모 미확인");
    let age_key = (x.age + x.person_index % 3).max(16);
    let half = x.pa / 2;
    let age_reference = match age_key {
        16 | 17 => half + half.min(50),
        18 => half + half.min(60),
        19 => half + half.min(70),
        20 => half + half.min(80),
        21 => half + half.min(90),
        _ => x.pa,
    };
    // The native CWD / IDIV r14w discards the adjacent high word read with
    // competition reputation. The validated numerator fits signed16.
    let numerator = x.team_reputation as i32 / 2 + 25 * x.competition_reputation as i32;
    let base = (numerator / x.denominator as i32).max(10) + age_reference as i32 / 2;
    let ceiling = if x.denominator == 115 {
        let facility = x.facility.context("시설 상한 입력 미확인")?;
        ensure!((0..15).contains(&facility), "시설 상한 입력 범위 미확인");
        Some(110 + facility as i32)
    } else {
        None
    };
    let value = ceiling.map_or(base, |cap| base.min(cap));
    Ok(Calculation {
        age_reference,
        base,
        value,
        gap: value - x.ca as i32,
        ceiling,
    })
}
fn pointer(a: u64) -> bool {
    (0x10000..0x7fff_ffff_ffff).contains(&a)
}
fn label(kind: u8) -> String {
    match kind {
        0 => "1군".into(),
        12 => "U-18".into(),
        18 => "U-20".into(),
        _ => format!("팀 종류 {kind}"),
    }
}

// A request-local read set is checked again before publishing any results.
// It never writes, suspends the process or reuses an index's old CA/PA/age.
struct Capture<'a> {
    read: &'a dyn Fn(u64, usize) -> Result<Vec<u8>>,
    session: &'a Session,
    bytes: HashMap<(u64, usize), Vec<u8>>,
}
impl Capture<'_> {
    fn bytes(&mut self, a: u64, n: usize) -> Result<Vec<u8>> {
        ensure!(
            pointer(a) && n <= MAX_PLAYERS * 8,
            "성장 조회 메모리 범위 오류"
        );
        if let Some(v) = self.bytes.get(&(a, n)) {
            return Ok(v.clone());
        }
        let v = (self.read)(a, n)?;
        ensure!(v.len() == n, "성장 조회 길이 오류");
        self.bytes.insert((a, n), v.clone());
        Ok(v)
    }
    fn u8(&mut self, a: u64) -> Result<u8> {
        Ok(self.bytes(a, 1)?[0])
    }
    fn i8(&mut self, a: u64) -> Result<i8> {
        Ok(self.u8(a)? as i8)
    }
    fn u16(&mut self, a: u64) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(a, 2)?.try_into().unwrap()))
    }
    fn u32(&mut self, a: u64) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(a, 4)?.try_into().unwrap()))
    }
    fn u64(&mut self, a: u64) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(a, 8)?.try_into().unwrap()))
    }
    fn ptr(&mut self, a: u64) -> Result<u64> {
        let p = self.u64(a)?;
        ensure!(p == 0 || pointer(p), "포인터 범위 미확인");
        Ok(p)
    }
    fn vector(&mut self, a: u64, max: usize) -> Result<Vec<u64>> {
        let h = self.bytes(a, 16)?;
        let b = u64::from_le_bytes(h[..8].try_into().unwrap());
        let e = u64::from_le_bytes(h[8..].try_into().unwrap());
        if b == e {
            ensure!(b == 0 || pointer(b), "빈 목록 포인터 오류");
            return Ok(vec![]);
        }
        ensure!(
            pointer(b) && pointer(e) && e > b && (e - b) % 8 == 0 && (e - b) / 8 <= max as u64,
            "선수단/제휴 목록 범위 미확인"
        );
        Ok(self
            .bytes(b, (e - b) as usize)?
            .chunks_exact(8)
            .map(|x| u64::from_le_bytes(x.try_into().unwrap()))
            .collect())
    }
    fn type_info(&mut self, a: u64) -> Result<(u32, String)> {
        let vt = self.u64(a)?;
        let base = self.session.module_base;
        let end = base + self.session.module_size as u64;
        ensure!((base + 8..end).contains(&vt), "객체 형식 미확인");
        let col = self.u64(vt - 8)?;
        ensure!((base..end - 24).contains(&col), "객체 RTTI 범위 오류");
        let raw = self.bytes(col, 24)?;
        let word = |i: usize| u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
        ensure!(
            word(0) == 1 && base + word(20) as u64 == col,
            "객체 RTTI 기준점 오류"
        );
        let desc = base + word(12) as u64;
        ensure!(
            (base..end - 272).contains(&desc),
            "객체 RTTI 이름 범위 오류"
        );
        let text = self.bytes(desc + 16, 256)?;
        let n = text
            .iter()
            .position(|&b| b == 0)
            .context("객체 이름 길이 오류")?;
        Ok((word(4), std::str::from_utf8(&text[..n])?.to_owned()))
    }
    fn is_type(&mut self, a: u64, name: &str) -> Result<()> {
        let (offset, found) = self.type_info(a)?;
        ensure!(offset == 0 && found == name, "객체 형식 미확인: {name}");
        let id = self.u32(a + 12)?;
        ensure!(id > 0 && id < 0x8000_0000, "객체 ID 범위 미확인: {name}");
        Ok(())
    }
    fn verify(&self) -> Result<()> {
        for (&(a, n), expected) in &self.bytes {
            ensure!(
                (self.read)(a, n)? == *expected,
                "조회 중 선수단 또는 성장 입력이 변경됐습니다. 다시 조회하세요."
            );
        }
        Ok(())
    }
    fn player(&mut self, candidate: u64, registry_end: u64) -> Result<(u64, u64, u32, u32)> {
        let (offset, name) = self.type_info(candidate)?;
        ensure!(
            name.contains("PLAYER") && !name.contains("VIRTUAL"),
            "선수 객체가 아닙니다"
        );
        let (person, base) = if offset == 0 {
            let vb = self.ptr(candidate + 8)?;
            let displacement = self.u32(vb + 4)? as i32;
            ensure!(
                matches!(displacement, 0x280 | 0x378),
                "선수 가상 기반 오프셋 미확인"
            );
            (candidate + 8 + displacement as u64, candidate)
        } else {
            ensure!(matches!(offset, 0x288 | 0x380), "선수 인물 오프셋 미확인");
            (
                candidate,
                candidate
                    .checked_sub(offset as u64)
                    .context("선수 기반 오류")?,
            )
        };
        let (po, pn) = self.type_info(person)?;
        ensure!(
            pn == name && matches!(po, 0x288 | 0x380) && person - po as u64 == base,
            "선수 인물 형식 불일치"
        );
        let (bo, bn) = self.type_info(base)?;
        ensure!(bo == 0 && bn == name, "선수 기반 형식 불일치");
        let raw = self.bytes(person + 8, 8)?;
        let ix = u32::from_le_bytes(raw[..4].try_into().unwrap());
        let id = u32::from_le_bytes(raw[4..].try_into().unwrap());
        let slot = self.session.registry_begin + ix as u64 * 8;
        ensure!(
            id > 0 && id < 0x8000_0000 && ix < 2_000_000 && slot < registry_end,
            "선수 ID/인덱스 범위 오류"
        );
        ensure!(self.u64(slot)? == person, "선수 레지스트리 불일치");
        Ok((person, base, id, ix))
    }
    fn resolve_player(&mut self, entry: u64, end: u64) -> Result<(u64, u64, u32, u32)> {
        if let Ok(p) = self.player(entry, end) {
            return Ok(p);
        }
        // The existing roster reader admits either player/person pointers or
        // a short holder. Every candidate must independently pass RTTI+registry.
        let holder = self.bytes(entry, 0x88)?;
        let mut found = None;
        for chunk in holder.chunks_exact(8) {
            let p = u64::from_le_bytes(chunk.try_into().unwrap());
            if !pointer(p) {
                continue;
            }
            if let Ok(v) = self.player(p, end) {
                if let Some(previous) = found {
                    ensure!(previous == v, "선수단 항목에 여러 인물이 있습니다");
                }
                found = Some(v);
            }
        }
        found.context("선수단 항목의 인물을 확인할 수 없습니다")
    }
    fn facility(&mut self, club: u64, include: bool, stack: &mut HashSet<u64>) -> Result<i8> {
        ensure!(
            stack.len() < 16 && stack.insert(club),
            "시설 공유 관계의 순환/깊이 미확인"
        );
        let result = self.facility_inner(club, include, stack);
        stack.remove(&club);
        result
    }
    fn facility_inner(&mut self, club: u64, include: bool, stack: &mut HashSet<u64>) -> Result<i8> {
        self.is_type(club, ".?AVCLUB@db@@")?;
        let links = self.vector(club + 0x118, MAX_LINKS)?;
        if self.u8(club + 0x163)? & 0x10 == 0 {
            for &link in &links {
                let child = self.ptr(link + 8)?;
                let kind = self.u8(link + 0x30)?;
                if child == club && matches!(kind, 4..=8 | 20) {
                    let parent = self.ptr(link)?;
                    if parent != 0 {
                        return self.facility(parent, true, stack);
                    }
                    break;
                }
            }
        }
        let data = self.ptr(club + 0x100)?;
        ensure!(data != 0, "시설 레코드 없음");
        let mut value = self.i8(data + 0x118)?;
        ensure!((0..=20).contains(&value), "시설 값 범위 미확인");
        if include {
            for link in links {
                let kind = self.u8(link + 0x30)?;
                if matches!(kind, 4..=8 | 20) {
                    continue;
                }
                let parent = self.ptr(link)?;
                let child = self.ptr(link + 8)?;
                if parent == 0 || child == 0 {
                    continue;
                }
                let flags = self.u32(link + 0x1C)?;
                let mut candidate = None;
                if child == club && flags & 0x400 != 0 {
                    candidate = Some(self.facility(parent, false, stack)? / 2)
                }
                if child == club && flags & 0x10000 != 0 {
                    candidate = Some(self.facility(parent, false, stack)?)
                }
                if parent == club && flags & 0x40000 != 0 {
                    candidate = Some(self.facility(child, false, stack)?)
                }
                if let Some(x) = candidate {
                    value = value.max(x)
                }
            }
        }
        Ok(value)
    }
}

fn read_age(raw: u32, today: (u32, u32, u32)) -> Result<u32> {
    let dob = backend::decode_date(raw).context("생년월일 미확인")?;
    ensure!(dob != (1900, 1, 1) && dob <= today, "생년월일 미확인");
    let age = today.0 - dob.0 - u32::from((today.1, today.2) < (dob.1, dob.2));
    ensure!(
        (14..=100).contains(&age),
        "지원하는 선수 나이 범위가 아닙니다"
    );
    Ok(age)
}
fn denominator(
    c: &mut Capture<'_>,
    club: u64,
    fallback: bool,
    first_rep: u16,
    gk: u8,
) -> Result<(u16, Option<i8>)> {
    let nation = c.ptr(club + 0xD8)?;
    if nation == 0 {
        return Ok((105, None));
    }
    c.is_type(nation, ".?AVNATION@db@@")?;
    let category = c.u8(club + 0xB9)?;
    ensure!(category <= 1, "구단 축구 구분 미확인");
    let record = c.ptr(nation + 0x108 + category as u64 * 8)?;
    if record == 0 {
        return Ok((105, None));
    }
    if c.i8(record + 0x875)? >= 3 {
        let f = c.facility(club, true, &mut HashSet::new())?;
        return Ok((if f < 15 { 115 } else { 105 }, Some(f)));
    }
    if !fallback {
        return Ok((105, None));
    }
    if first_rep >= 6500 || gk >= 15 {
        return Ok((80, None));
    }
    anyhow::bail!("특수 국가·리그 분모 조건은 아직 확인하지 못했습니다")
}

fn fill_player(
    c: &mut Capture<'_>,
    item: &mut GrowthItem,
    person: u64,
    base: u64,
    ix: u32,
    club: u64,
    teams: &HashSet<u64>,
    first: u64,
    today: (u32, u32, u32),
) -> Result<()> {
    item.person_index = Some(ix);
    let ca = c.u16(base + 0x264)?;
    let pa = c.u16(base + 0x266)?;
    ensure!(
        (1..=200).contains(&ca) && (1..=200).contains(&pa),
        "CA/PA 범위 미확인"
    );
    item.current_ca = Some(ca);
    item.potential_ca = Some(pa);
    let age = read_age(c.u32(person + 0x88)?, today)?;
    item.current_age = Some(age);
    let actual = c.ptr(base + 0x130)?;
    ensure!(
        teams.contains(&actual),
        "실제 소속팀이 현재 구단 선수단과 일치하지 않습니다"
    );
    c.is_type(actual, ".?AVTEAM@db@@")?;
    ensure!(
        c.ptr(actual + 0x30)? == club && c.u8(actual + 0x2F)? == 0,
        "실제 소속 구단 미확인"
    );
    let kind = c.u8(actual + 0x28)?;
    item.actual_team = Some(label(kind));
    item.source_team_kind = Some(label(kind));
    ensure!(
        matches!(kind, 0 | 12 | 18),
        "이 팀 종류의 네이티브 선택 조건은 아직 확인하지 못했습니다"
    );
    let mut direct = false;
    if kind != 0 {
        let competition = c.ptr(actual + 0x50)?;
        if competition != 0 {
            c.is_type(competition, ".?AVCOMP@db@@")?;
            let nation = c.ptr(competition + 0x60)?;
            if nation != 0 {
                c.is_type(nation, ".?AVNATION@db@@")?;
            }
            let typ = c.u8(competition + 0xF8)?;
            // Native admits only these three byte values for this branch.
            // Every other byte takes the verified fallback; unknown enum
            // labels do not make that predicate or fallback uncertain.
            direct = nation != 0 && matches!(typ, 0 | 1 | 14);
        }
    }
    // Verified nation squad-configuration helper never treats U18/U20 as
    // type1; neither is in 0x4003E008. Otherwise native selects vector[0].
    let selected = if direct { actual } else { first };
    c.is_type(selected, ".?AVTEAM@db@@")?;
    ensure!(
        c.ptr(selected + 0x30)? == club && c.u8(selected + 0x2F)? == 0,
        "선택 팀 소속 미확인"
    );
    let selected_kind = c.u8(selected + 0x28)?;
    if !direct {
        ensure!(selected_kind == 0, "구단 첫 팀이 1군이 아닙니다")
    }
    item.selected_team = Some(label(selected_kind));
    let rep = c.u16(selected + 0xA8)?;
    ensure!(rep <= 10000, "팀 평판 범위 미확인");
    item.team_reputation = Some(rep);
    let comp = c.ptr(selected + 0x50)?;
    // Null competition is a verified native zero contribution, unlike a read failure.
    let comp_rep = if comp == 0 {
        0
    } else {
        c.is_type(comp, ".?AVCOMP@db@@")?;
        let r = c.u16(comp + 0x188)?;
        ensure!(r <= 200, "대회 평판 범위 미확인");
        r
    };
    item.competition_reputation = Some(comp_rep);
    let first_rep = c.u16(first + 0xA8)?;
    ensure!(first_rep <= 10000, "1군 평판 범위 미확인");
    let gk = c.u8(base + 0x150)?;
    ensure!(gk <= 20, "GK 숙련도 범위 미확인");
    let (d, f) = denominator(c, club, !direct, first_rep, gk)?;
    item.denominator = Some(d);
    item.native_facility_value = f;
    let result = calculate(Inputs {
        ca,
        pa,
        age,
        person_index: ix,
        team_reputation: rep,
        competition_reputation: comp_rep,
        denominator: d,
        facility: f,
    })?;
    item.age_reference = Some(result.age_reference);
    item.base_reference_value = Some(result.base);
    item.facility_ceiling = result.ceiling;
    item.reference_value = Some(result.value);
    item.reference_gap = Some(result.gap);
    item.status = "calculated";
    item.calculation_note = format!(
        "{} → {} 입력. 나이와 인물 인덱스에 따른 참고값을 사용하며, 확인된 시설 상한까지 반영했습니다. {}",
        label(kind),
        label(selected_kind),
        SCOPE
    );
    Ok(())
}

fn capture_with(
    s: &Session,
    club: &Entity,
    read: &dyn Fn(u64, usize) -> Result<Vec<u8>>,
    registry_end: u64,
) -> Result<ClubGrowth> {
    let mut c = Capture {
        read,
        session: s,
        bytes: HashMap::new(),
    };
    ensure!(
        club.kind == EntityKind::Club && club.id > 0,
        "구단을 선택하세요"
    );
    c.is_type(club.address, ".?AVCLUB@db@@")?;
    ensure!(
        c.u32(club.address + 12)? == club.id,
        "구단 ID가 변경됐습니다"
    );
    let mode = c.u8(s.module_base + 0x4E45258)?;
    ensure!(
        !matches!(mode, 1 | 2),
        "대체 날짜 모드의 성장 참고치는 아직 지원하지 않습니다"
    );
    let today =
        backend::decode_date(c.u32(s.module_base + 0x4DF3C18)?).context("게임 날짜 미확인")?;
    ensure!(today != (1900, 1, 1), "게임 날짜 미확인");
    let team_vec = c.vector(club.address + 0x18, MAX_TEAMS)?;
    let first = *team_vec.first().context("구단 선수단이 없습니다")?;
    let teams: HashSet<u64> = team_vec.iter().copied().collect();
    ensure!(
        teams.len() == team_vec.len(),
        "구단 팀 목록이 중복되었습니다"
    );
    let mut entries = HashSet::new();
    for team in team_vec {
        c.is_type(team, ".?AVTEAM@db@@")?;
        ensure!(c.ptr(team + 0x30)? == club.address, "팀 소속 구단 불일치");
        for entry in c.vector(team + 0x38, MAX_ROSTER)? {
            ensure!(pointer(entry), "선수단 항목 포인터 미확인");
            entries.insert(entry);
            ensure!(
                entries.len() <= MAX_PLAYERS,
                "구단 선수 수가 허용 범위를 넘습니다"
            );
        }
    }
    let mut items = Vec::new();
    let mut identities = HashMap::new();
    let mut unresolved = 0;
    for entry in entries {
        let Ok((person, base, id, ix)) = c.resolve_player(entry, registry_end) else {
            unresolved += 1;
            continue;
        };
        if let Some(previous) = identities.insert(id, person) {
            ensure!(previous == person, "선수 ID 중복");
            continue;
        }
        let mut item = GrowthItem::unknown(id);
        if let Err(e) = fill_player(
            &mut c,
            &mut item,
            person,
            base,
            ix,
            club.address,
            &teams,
            first,
            today,
        ) {
            item.reason = e.to_string()
        }
        items.push(item);
    }
    items.sort_by_key(|x| x.player_id);
    c.verify()?;
    let warnings = if unresolved > 0 {
        vec![format!(
            "선수단 항목 {unresolved}개의 인물 신원을 확인하지 못했습니다. 해당 선수의 참고치는 표시하지 않습니다."
        )]
    } else {
        vec![]
    };
    Ok(ClubGrowth {
        club_id: club.id,
        formula_version: FORMULA_VERSION,
        scope: SCOPE,
        game_date: format!("{:04}-{:02}-{:02}", today.0, today.1, today.2),
        items,
        warnings,
    })
}
pub fn club_growth(s: &Session, club: &Entity) -> Result<ClubGrowth> {
    crate::compatibility::require_legacy_feature(s, "유소년 성장 참고치")?;
    let p = Process::open(s.pid, false)?;
    backend::validate_with(&p, s)?;
    let (_, end) = backend::registry(&p, s.module_base, s.db)?;
    let result = capture_with(s, club, &|a, n| p.read(a, n), end)?;
    backend::validate_with(&p, s)?;
    ensure!(
        backend::registry(&p, s.module_base, s.db)?.1 == end,
        "인물 목록이 조회 중 변경됐습니다"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FootballCategory;
    use std::cell::Cell;
    fn input() -> Inputs {
        Inputs {
            ca: 160,
            pa: 199,
            age: 22,
            person_index: 0,
            team_reputation: 7001,
            competition_reputation: 155,
            denominator: 105,
            facility: None,
        }
    }
    #[test]
    fn growth_age_bands_use_index_and_integer_halves() {
        for (age, index, expected) in [
            (16, 0, 149),
            (16, 1, 149),
            (16, 2, 159),
            (19, 0, 169),
            (20, 0, 179),
            (21, 0, 189),
            (20, 2, 199),
            (30, 0, 199),
        ] {
            let result = calculate(Inputs {
                age,
                person_index: index,
                ..input()
            })
            .unwrap();
            assert_eq!(result.age_reference, expected);
        }
    }
    #[test]
    fn growth_reference_matches_native_integer_example_and_facility_ceiling() {
        let result = calculate(input()).unwrap();
        assert_eq!(
            result,
            Calculation {
                age_reference: 199,
                base: 169,
                value: 169,
                gap: 9,
                ceiling: None
            }
        );
        let result = calculate(Inputs {
            denominator: 115,
            facility: Some(14),
            ..input()
        })
        .unwrap();
        assert_eq!(result.base, 163);
        assert_eq!(result.value, 124);
        assert_eq!(result.gap, -36);
        assert_eq!(result.ceiling, Some(124));
    }
    #[test]
    fn growth_reference_is_not_clamped_to_pa() {
        let r = calculate(Inputs {
            ca: 30,
            pa: 40,
            team_reputation: 10000,
            competition_reputation: 200,
            denominator: 80,
            ..input()
        })
        .unwrap();
        assert_eq!(r.value, 145);
        assert!(r.value > 40);
    }
    #[test]
    fn growth_unknown_inputs_never_become_zero_defaults() {
        for x in [
            Inputs { pa: 0, ..input() },
            Inputs { age: 13, ..input() },
            Inputs {
                team_reputation: 10001,
                ..input()
            },
            Inputs {
                competition_reputation: 201,
                ..input()
            },
            Inputs {
                denominator: 115,
                facility: None,
                ..input()
            },
            Inputs {
                denominator: 115,
                facility: Some(15),
                ..input()
            },
        ] {
            assert!(calculate(x).is_err())
        }
    }
    #[test]
    fn growth_age_checks_birthdays_and_sentinel() {
        let date = |year: u32, day: u32| (year << 16) | day;
        assert_eq!(read_age(date(2008, 60), (2028, 2, 28)).unwrap(), 19);
        assert_eq!(read_age(date(2008, 60), (2028, 2, 29)).unwrap(), 20);
        assert!(read_age(date(1900, 1), (2028, 2, 29)).is_err());
    }
    struct Fixture {
        bytes: HashMap<u64, u8>,
        session: Session,
        club: Entity,
        next_type: u64,
    }
    const CLUB: u64 = 0x10000;
    const FIRST: u64 = 0x20000;
    const PLAYER: u64 = 0x30000;
    const PERSON: u64 = PLAYER + 0x288;
    const COMP: u64 = 0x50000;
    const NATION: u64 = 0x60000;
    const RECORD: u64 = 0x70000;
    const REGISTRY: u64 = 0x80000;
    const YOUTH: u64 = 0xB0000;
    impl Fixture {
        fn put(&mut self, a: u64, v: &[u8]) {
            for (i, &b) in v.iter().enumerate() {
                self.bytes.insert(a + i as u64, b);
            }
        }
        fn u8(&mut self, a: u64, v: u8) {
            self.put(a, &[v]);
        }
        fn u16(&mut self, a: u64, v: u16) {
            self.put(a, &v.to_le_bytes());
        }
        fn u32(&mut self, a: u64, v: u32) {
            self.put(a, &v.to_le_bytes());
        }
        fn u64(&mut self, a: u64, v: u64) {
            self.put(a, &v.to_le_bytes());
        }
        fn typ(&mut self, a: u64, name: &str, offset: u32) {
            let vt = self.session.module_base + self.next_type;
            self.next_type += 0x1000;
            let col = vt + 0x100;
            let desc = vt + 0x200;
            self.u64(a, vt);
            self.u64(vt - 8, col);
            let mut h = [0u8; 24];
            h[..4].copy_from_slice(&1u32.to_le_bytes());
            h[4..8].copy_from_slice(&offset.to_le_bytes());
            h[12..16].copy_from_slice(&((desc - self.session.module_base) as u32).to_le_bytes());
            h[20..24].copy_from_slice(&((col - self.session.module_base) as u32).to_le_bytes());
            self.put(col, &h);
            let mut text = [0u8; 256];
            text[..name.len()].copy_from_slice(name.as_bytes());
            self.put(desc + 16, &text);
            if [
                ".?AVCLUB@db@@",
                ".?AVTEAM@db@@",
                ".?AVCOMP@db@@",
                ".?AVNATION@db@@",
            ]
            .contains(&name)
            {
                self.u32(a + 12, a as u32);
            }
        }
        fn vector(&mut self, address: u64, buffer: u64, entries: &[u64]) {
            self.u64(address, buffer);
            self.u64(address + 8, buffer + entries.len() as u64 * 8);
            for (i, e) in entries.iter().enumerate() {
                self.u64(buffer + i as u64 * 8, *e)
            }
        }
        fn read(&self, a: u64, n: usize) -> Result<Vec<u8>> {
            (0..n)
                .map(|i| {
                    self.bytes
                        .get(&(a + i as u64))
                        .copied()
                        .with_context(|| format!("synthetic missing {a:X}+{i}"))
                })
                .collect()
        }
        fn run(&self) -> Result<ClubGrowth> {
            capture_with(
                &self.session,
                &self.club,
                &|a, n| self.read(a, n),
                REGISTRY + 8,
            )
        }
        fn nation(&mut self, flag: u8, facility: u8) {
            self.typ(NATION, ".?AVNATION@db@@", 0);
            self.u64(CLUB + 0xD8, NATION);
            self.u8(CLUB + 0xB9, 0);
            self.u64(NATION + 0x108, RECORD);
            self.u8(RECORD + 0x875, flag);
            self.u64(CLUB + 0x100, 0xD0000);
            self.u8(0xD0000 + 0x118, facility);
            self.u8(CLUB + 0x163, 0x10);
            self.vector(CLUB + 0x118, 0, &[]);
        }
        fn youth(&mut self, comp_type: u8) {
            self.typ(YOUTH, ".?AVTEAM@db@@", 0);
            self.u8(YOUTH + 0x28, 18);
            self.u8(YOUTH + 0x2F, 0);
            self.u64(YOUTH + 0x30, CLUB);
            self.vector(CLUB + 0x18, 0x90000, &[FIRST, YOUTH]);
            self.vector(FIRST + 0x38, 0, &[]);
            self.vector(YOUTH + 0x38, 0xA0000, &[PLAYER]);
            self.u64(PLAYER + 0x130, YOUTH);
            self.u16(YOUTH + 0xA8, 2000);
            self.u64(YOUTH + 0x50, COMP);
            self.u8(COMP + 0xF8, comp_type);
            self.u64(COMP + 0x60, NATION);
        }
    }
    fn fixture() -> Fixture {
        let session = Session {
            pid: 1,
            created: 1,
            module_base: 0x180000000,
            module_size: 0x1EB41000,
            module_stamp: 0x6A229E8A,
            db: 0xF0000,
            registry_begin: REGISTRY,
            game_date: "stale".into(),
            profile: backend::PROFILE.into(),
            own_club_id: Some(1141),
        };
        let club = Entity {
            nationality: String::new(),
            nationality_id: None,
            id: 1141,
            kind: EntityKind::Club,
            name: "Test".into(),
            english_name: String::new(),
            club: String::new(),
            club_id: None,
            age: None,
            ca: None,
            pa: None,
            role: String::new(),
            group: String::new(),
            football_category: FootballCategory::Men,
            address: CLUB,
            base: CLUB,
            index: None,
        };
        let mut f = Fixture {
            bytes: HashMap::new(),
            session,
            club,
            next_type: 0x10000,
        };
        f.typ(CLUB, ".?AVCLUB@db@@", 0);
        f.u32(CLUB + 12, 1141);
        f.u64(CLUB + 0xD8, 0);
        f.typ(FIRST, ".?AVTEAM@db@@", 0);
        f.u8(FIRST + 0x28, 0);
        f.u8(FIRST + 0x2F, 0);
        f.u64(FIRST + 0x30, CLUB);
        f.u16(FIRST + 0xA8, 7001);
        f.u64(FIRST + 0x50, COMP);
        f.typ(COMP, ".?AVCOMP@db@@", 0);
        f.u16(COMP + 0x188, 155);
        f.typ(PLAYER, ".?AVACTUAL_PLAYER@db@@", 0);
        f.typ(PERSON, ".?AVACTUAL_PLAYER@db@@", 0x288);
        f.u64(PLAYER + 8, 0xC0000);
        f.u32(0xC0004, 0x280);
        f.u32(PERSON + 8, 0);
        f.u32(PERSON + 12, 5001);
        f.u64(REGISTRY, PERSON);
        f.u32(PERSON + 0x88, (2006 << 16) | 1);
        f.u16(PLAYER + 0x264, 160);
        f.u16(PLAYER + 0x266, 199);
        f.u8(PLAYER + 0x150, 1);
        f.u64(PLAYER + 0x130, FIRST);
        f.vector(CLUB + 0x18, 0x90000, &[FIRST]);
        f.vector(FIRST + 0x38, 0xA0000, &[PLAYER]);
        f.u8(f.session.module_base + 0x4E45258, 0);
        f.u32(f.session.module_base + 0x4DF3C18, (2028 << 16) | 33);
        f
    }
    #[test]
    fn growth_fresh_roster_values_and_identity_drive_result() {
        let mut f = fixture();
        let first = f.run().unwrap();
        assert_eq!(first.game_date, "2028-02-02");
        let row = &first.items[0];
        assert_eq!(row.status, "calculated");
        assert_eq!(row.reference_value, Some(169));
        assert_eq!(row.current_ca, Some(160));
        f.u16(PLAYER + 0x264, 180);
        f.u32(PERSON + 12, 9999);
        let changed = f.run().unwrap();
        assert_eq!(changed.items[0].player_id, 9999);
        assert_eq!(changed.items[0].reference_value, row.reference_value);
        assert_eq!(changed.items[0].reference_gap, Some(-11)); // UID never changes age selector.
    }
    #[test]
    fn growth_roster_guard_rejects_external_change_and_bad_identity() {
        let mut f = fixture();
        let reads = Cell::new(0usize);
        let read = |a, n| {
            let mut v = f.read(a, n)?;
            if a == PLAYER + 0x264 {
                let count = reads.get();
                reads.set(count + 1);
                if count > 0 {
                    v[0] ^= 1
                }
            }
            Ok(v)
        };
        assert!(
            capture_with(&f.session, &f.club, &read, REGISTRY + 8)
                .unwrap_err()
                .to_string()
                .contains("조회 중")
        );
        f.u64(REGISTRY, PERSON + 8);
        let result = f.run().unwrap();
        assert!(result.items.is_empty());
        assert!(!result.warnings.is_empty());
    }
    #[test]
    fn growth_missing_read_is_unknown_and_roster_bounds_fail_closed() {
        let mut f = fixture();
        f.bytes.remove(&(COMP + 0x188));
        let row = f.run().unwrap().items.remove(0);
        assert_eq!(row.status, "unknown");
        assert_eq!(row.reference_value, None);
        assert_eq!(row.competition_reputation, None);
        assert!(!row.reason.is_empty());
        f.u64(FIRST + 0x40, 0xA0000 + (MAX_ROSTER as u64 + 1) * 8);
        assert!(f.run().is_err());
    }
    #[test]
    fn growth_team_selection_and_safe_denominator_branches() {
        let mut f = fixture();
        f.nation(2, 14);
        f.youth(0);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.selected_team.as_deref(), Some("U-20"));
        assert_eq!(r.denominator, Some(105));
        f.u8(COMP + 0xF8, 2);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.selected_team.as_deref(), Some("1군"));
        assert_eq!(r.denominator, Some(80));
        f.u16(FIRST + 0xA8, 6400);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.status, "unknown");
        assert_eq!(r.denominator, None);
        f.u8(PLAYER + 0x150, 15);
        assert_eq!(f.run().unwrap().items[0].denominator, Some(80));
        f.u8(RECORD + 0x875, 3);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.denominator, Some(115));
        assert_eq!(r.reference_value, Some(124));
        f.u8(0xD0000 + 0x118, 15);
        assert_eq!(f.run().unwrap().items[0].denominator, Some(105));
    }
    #[test]
    fn growth_high_competition_type_bytes_use_first_team_fallback() {
        for kind in [12, 18] {
            for typ in [31, 64, 253, 255] {
                let mut f = fixture();
                f.nation(2, 14);
                f.youth(typ);
                f.u8(YOUTH + 0x28, kind);
                let row = f.run().unwrap().items.remove(0);
                assert_eq!(
                    row.status, "calculated",
                    "kind={kind}, type={typ}: {}",
                    row.reason
                );
                assert_eq!(row.actual_team, Some(label(kind)));
                assert_eq!(row.selected_team.as_deref(), Some("1군"));
                assert_eq!(row.team_reputation, Some(7001));
                assert_eq!(row.denominator, Some(80));
                assert_eq!(row.reference_value, Some(191));
            }
        }
    }
    #[test]
    fn growth_loan_or_stale_membership_does_not_use_the_parent_club_environment() {
        let mut f = fixture();
        f.u64(PLAYER + 0x130, 0xF1000); // Another club's actual team, despite a stale parent roster entry.
        let row = f.run().unwrap().items.remove(0);
        assert_eq!(row.status, "unknown");
        assert_eq!(row.reference_value, None);
        assert_eq!(row.team_reputation, None);
        assert_eq!(row.denominator, None);
        assert!(row.reason.contains("실제 소속팀"));
        f.u64(PLAYER + 0x130, FIRST);
        f.u64(FIRST + 0x30, 0xF2000);
        assert!(f.run().is_err());
    }
    #[test]
    fn growth_wire_contract_distinguishes_absent_competition_from_failed_read() {
        let mut f = fixture();
        f.u64(FIRST + 0x50, 0);
        let dto = serde_json::to_value(f.run().unwrap()).unwrap();
        let row = &dto["items"][0];
        assert_eq!(row["status"], "calculated");
        assert_eq!(row["competition_reputation"], 0);
        assert_eq!(row["reference_value"], 132);
        assert_eq!(row["reference_gap"], -28);
        assert!(row["facility_ceiling"].is_null());
        assert_eq!(dto["formula_version"], FORMULA_VERSION);
        f.bytes.remove(&(FIRST + 0x50));
        let dto = serde_json::to_value(f.run().unwrap()).unwrap();
        assert_eq!(dto["items"][0]["status"], "unknown");
        assert!(dto["items"][0]["reference_value"].is_null());
    }
    #[test]
    fn growth_facility_sharing_order_parent_traversal_and_cycles() {
        let mut f = fixture();
        f.nation(3, 10);
        let parent = 0xE0000;
        let link = 0xE1000;
        f.typ(parent, ".?AVCLUB@db@@", 0);
        f.u8(parent + 0x163, 0x10);
        f.vector(parent + 0x118, 0, &[]);
        f.u64(parent + 0x100, 0xE2000);
        f.u8(0xE2118, 20);
        f.vector(CLUB + 0x118, 0xE3000, &[link]);
        f.u64(link, parent);
        f.u64(link + 8, CLUB);
        f.u8(link + 0x30, 0);
        f.u32(link + 0x1C, 0x400);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.native_facility_value, Some(10));
        assert_eq!(r.denominator, Some(115));
        f.u32(link + 0x1C, 0x10400);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.native_facility_value, Some(20));
        assert_eq!(r.denominator, Some(105));
        f.u8(link + 0x30, 4);
        assert_eq!(f.run().unwrap().items[0].native_facility_value, Some(10)); // excluded from sharing
        f.u8(CLUB + 0x163, 0);
        assert_eq!(f.run().unwrap().items[0].native_facility_value, Some(20)); // parent traversal
        f.u64(link, CLUB);
        let r = f.run().unwrap().items.remove(0);
        assert_eq!(r.status, "unknown");
        assert!(r.reason.contains("순환"));
    }
}
