//! Build-scoped native registry adapter. Unknown or unverified layouts fail closed.
use crate::{
    memory::{Module, Process},
    model::*,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{HashMap, HashSet};
pub mod watch;
pub(crate) mod discovery;
#[cfg(feature = "fm27-health-experiment")]
pub(crate) mod health;
#[cfg(feature = "fm27-health-experiment")]
pub(crate) mod bans;
#[cfg(feature = "fm27-transfer-experiment")]
pub(crate) mod transfer_experiment;

pub mod profiles;
/// Legacy default used by old snapshots/tests, not a global connection grant.
pub const PROFILE: &str = profiles::FM26.label;

#[derive(Clone, Debug, serde::Serialize)]
pub struct DetectedBuild {
    pub timestamp: u32,
    pub image_size: u32,
    pub profile: Option<&'static profiles::BuildProfile>,
}
/// Header identity observed before later validation. This never grants access.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ObservedBuild {
    pub timestamp: u32,
    pub image_size: u32,
}
impl std::fmt::Display for ObservedBuild {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        write!(f,"관찰된 FM 빌드 {:08X}-{:08X}",self.timestamp,self.image_size)
    }
}
impl std::error::Error for ObservedBuild {}
fn read_pe_identity(p: &Process, base: u64) -> Result<(u32,u32)> {
    read_pe_identity_with(base,|address,size|p.read(address,size))
}
fn read_pe_identity_with(base:u64, mut read:impl FnMut(u64,usize)->Result<Vec<u8>>)->Result<(u32,u32)> {
    let dos=read(base,64)?;
    ensure!(dos.len()==64,"FM 모듈 DOS 헤더 크기가 다릅니다");
    ensure!(word16(&dos,0)==0x5A4D,"FM 모듈 DOS 헤더가 다릅니다");
    let offset=word32(&dos,0x3c) as u64;
    ensure!((64..0x1000).contains(&offset),"지원하지 않는 FM PE 헤더 위치입니다");
    let header=read(base+offset,84)?;
    ensure!(header.len()==84,"FM 모듈 PE 헤더 크기가 다릅니다");
    ensure!(word32(&header,0)==0x4550 && word16(&header,4)==0x8664 && word16(&header,24)==0x20B,
        "지원하지 않는 FM 모듈 형식입니다");
    Ok((word32(&header,8),word32(&header,80)))
}
/// Bounded PE-header/code reads only; never follows game objects before the
/// exact build and independently verified native signatures have passed.
pub fn detect_build(p: &Process,m: &Module)->Result<DetectedBuild> {
    detect_build_with(m,|address,size|p.read(address,size))
        .with_context(||discovery::module_context(&m.path))
}
fn detect_build_with(m:&Module,mut read:impl FnMut(u64,usize)->Result<Vec<u8>>)->Result<DetectedBuild> {
    let (timestamp,image_size)=read_pe_identity_with(m.base,&mut read)?;
    (|| {
        ensure!(image_size==m.size,"FM 모듈 이미지 크기가 헤더와 다릅니다");
        let profile=profiles::identify(timestamp,image_size);
        if let Some(profile)=profile {profile.verify_code_with(m.base,read)?;}
        Ok(DetectedBuild{timestamp,image_size,profile})
    })().with_context(||ObservedBuild{timestamp,image_size})
}
fn require_read_profile(build:&DetectedBuild)->Result<&'static profiles::BuildProfile> {
    let profile=build.profile.ok_or_else(||anyhow::anyhow!(profiles::UnsupportedBuild {
        timestamp:build.timestamp,image_size:build.image_size,profile:None }))?;
    profile.require_read_layout()?;
    Ok(profile)
}
pub fn fingerprint(p: &Process, m: &Module) -> Result<(u32,u32)> {
    let build=detect_build(p,m)?;
    require_read_profile(&build)?;
    Ok((build.timestamp,build.image_size))
}
pub fn registry(p: &Process, base: u64, db: u64) -> Result<(u64,u64)> {
    let (timestamp,image_size)=read_pe_identity(p,base)?;
    let build=DetectedBuild{timestamp,image_size,profile:profiles::identify(timestamp,image_size)};
    let profile=require_read_profile(&build)?;
    profile.verify_code_with(base,|address,size|p.read(address,size))?;
    registry_with_layout(p,base,db,profile.require_read_layout()?)
}
/// The registry accessor implementation is shared by unrelated database types.
/// Native type evidence must also match the intended person table.
fn is_person_registry_type(name:&str)->bool {
    name.contains("PERSON") || name.contains("PLAYER") || name.contains("OFFICIAL")
        || name.contains("SUPPORT_STAFF") || name.contains("AGENT")
        // FM27 RTTI: JOURNALIST -> ACTUAL_PERSON_BASE -> PERSON. Save
        // progression can move one into a sampled registry slot. Accept only
        // this verified name; unrelated/unknown records still fail closed.
        || name == ".?AVJOURNALIST@db@@"
}
fn registry_with_layout(p:&Process,base:u64,db:u64,layout:&profiles::ReadLayout)->Result<(u64,u64)> {
    registry_with_reader(base,db,layout,|address,size|p.read(address,size))
}
#[derive(Debug)]
pub(crate) struct EmptyRegistry;
impl std::fmt::Display for EmptyRegistry {
    fn fmt(&self, f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        f.write_str("세이브 인물 목록이 비어 있습니다. 세이브 로드가 완료됐는지 확인해 주세요")
    }
}
impl std::error::Error for EmptyRegistry {}
fn registry_header(raw:&[u8])->Result<Option<(u64,u64)>> {
    ensure!(raw.len()==16,"인물 레지스트리 헤더 크기 오류");
    let (begin,end)=(word64(raw,0),word64(raw,8));
    if begin==end && (begin==0 || pointer(begin)==begin) { return Ok(None); }
    let bytes=end.checked_sub(begin).context("인물 레지스트리 역방향 범위 오류")?;
    ensure!(pointer(begin)==begin && begin!=0 && pointer(end)==end && bytes>0
        && bytes<16_000_000 && bytes%8==0,"인물 레지스트리 범위 검증 실패");
    Ok(Some((begin,end)))
}
fn registry_with_reader(base:u64,db:u64,layout:&profiles::ReadLayout,
    mut read:impl FnMut(u64,usize)->Result<Vec<u8>>)->Result<(u64,u64)> {
    ensure!(pointer(db)==db && db!=0,"인물 데이터베이스 주소 검증 실패");
    let flag_bytes=read(base+layout.registry_mode_rva,1)?;
    ensure!(flag_bytes.len()==1,"인물 레지스트리 모드 크기 오류");
    let flag=flag_bytes[0];
    ensure!(flag<=1,"인물 레지스트리 모드 검증 실패");
    let mut guards=vec![(base+layout.registry_mode_rva,flag_bytes)];
    let direct=if flag==0 {
        let address=db+layout.person_vector_offset;
        let raw=read(address,16)?;
        let range=registry_header(&raw)?; // Never bypass a malformed inline table.
        guards.push((address,raw));
        range
    } else {None};
    let range=if let Some(range)=direct {range} else {
        if flag==0 && !layout.person_empty_inline_fallback {return Err(anyhow::anyhow!(EmptyRegistry));}
        // FM27 native accessor A71ED4: mode 0 uses +60/+68 when a requested
        // index exists, otherwise follows +80. Scope this correction to an
        // empty, valid inline header; mixed tables are not inferred here.
        let route_address=db+layout.person_indirect_vector_offset;
        let route=read(route_address,8)?;
        ensure!(route.len()==8,"인물 레지스트리 간접 주소 크기 오류");
        let vector=word64(&route,0);
        ensure!(pointer(vector)==vector && vector!=0,"인물 레지스트리 간접 주소 검증 실패");
        guards.push((route_address,route));
        let raw=read(vector,16)?;
        let range=registry_header(&raw)?.ok_or_else(||anyhow::anyhow!(EmptyRegistry))?;
        guards.push((vector,raw));
        range
    };
    for (address,expected) in guards {
        ensure!(read(address,expected.len())?==expected,"조회 중 인물 레지스트리 경로가 변경됐습니다");
    }
    Ok(range)
}
pub fn type_name(p: &Process, s: &Session, vtable: u64) -> String {
    type_info(p, s.module_base, s.module_size, vtable)
        .map(|x| x.1)
        .unwrap_or_default()
}
fn type_info(p: &Process, base: u64, size: u32, vt: u64) -> Result<(u32, String)> {
    let end = base + size as u64;
    ensure!((base + 8..end).contains(&vt), "객체 형식 범위 불일치");
    let locator = p.u64(vt - 8)?;
    ensure!(
        (base..end - 24).contains(&locator) && p.u32(locator)? == 1,
        "객체 RTTI 검증 실패"
    );
    ensure!(
        base + p.u32(locator + 20)? as u64 == locator,
        "객체 RTTI 기준점 불일치"
    );
    let desc = base + p.u32(locator + 12)? as u64;
    ensure!((base..end - 272).contains(&desc), "객체 설명자 범위 불일치");
    let name = p.string(desc + 16, 256);
    ensure!(name.starts_with(".?AV"), "객체 이름 검증 실패");
    Ok((p.u32(locator + 4)?, name))
}
pub fn indirect_string(p: &Process, address: u64) -> String {
    let ptr = p.ptr(address);
    if ptr == 0 {
        String::new()
    } else {
        p.string(ptr + 4, 192)
    }
}
pub fn nested_string(p: &Process, address: u64) -> String {
    let outer = p.ptr(address);
    if outer == 0 {
        return String::new();
    }
    let ptr = p.ptr(outer);
    if ptr == 0 {
        String::new()
    } else {
        p.string(ptr + 4, 192)
    }
}
pub fn person_name(p: &Process, person: u64) -> String {
    let name = nested_string(p, person + 0x60);
    if !name.is_empty() {
        name
    } else {
        format!(
            "{} {}",
            nested_string(p, person + 0x50),
            nested_string(p, person + 0x58)
        )
        .trim()
        .to_string()
    }
}
fn compose_person_names(first: &str, last: &str, common: &str) -> Result<(String, String)> {
    ensure!(
        [first, last, common].iter().all(|name| !name.chars().any(char::is_control)),
        "인물 이름에 제어문자가 있습니다"
    );
    let english_name = format!("{first} {last}").trim().to_owned();
    let name = if common.is_empty() { english_name.clone() } else { common.to_owned() };
    ensure!(!name.trim().is_empty(), "인물 이름이 비어 있습니다");
    Ok((name, english_name))
}
// Unlike the best-effort index loader, post-write refresh must distinguish an
// absent common name from an unreadable pointer/string. Never publish a fallback
// name after a failed read, and verify the exact name references/text again.
fn read_person_names_with(
    person: u64,
    read: impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<(String, String)> {
    let names = read_person_name_parts_with(person, read)?;
    compose_person_names(&names[0], &names[1], &names[2])
}
fn read_person_name_parts_with(
    person: u64,
    mut read: impl FnMut(u64, usize) -> Result<Vec<u8>>,
) -> Result<[String; 3]> {
    let mut guards = Vec::new();
    let mut names = Vec::new();
    for offset in [0x50, 0x58, 0x60] {
        let address = person + offset;
        let raw = read(address, 8)?;
        let record = u64::from_le_bytes(raw.as_slice().try_into().context("이름 참조 길이 오류")?);
        guards.push((address, raw));
        if record == 0 {
            names.push(String::new());
            continue;
        }
        ensure!(pointer(record) == record, "이름 레코드 포인터 검증 실패");
        let raw = read(record, 8)?;
        let text = u64::from_le_bytes(raw.as_slice().try_into().context("이름 문자열 참조 길이 오류")?);
        guards.push((record, raw));
        ensure!(text != 0 && pointer(text) == text, "이름 문자열 포인터 검증 실패");
        let raw = read(text + 4, 192)?;
        let end = raw.iter().position(|&byte| byte == 0).context("이름이 허용 길이를 넘습니다")?;
        names.push(std::str::from_utf8(&raw[..end]).context("이름 UTF-8 검증 실패")?.to_owned());
        guards.push((text + 4, raw[..=end].to_vec()));
    }
    compose_person_names(&names[0], &names[1], &names[2])?;
    for (address, expected) in guards {
        ensure!(read(address, expected.len())? == expected, "조회 중 이름이 변경됐습니다. 다시 연결하세요");
    }
    names.try_into().map_err(|_| anyhow::anyhow!("이름 구성 요소 개수 오류"))
}
/// Exact native fields for LNC overrides; never guess the split of a full name.
pub(crate) fn player_name_parts(session: &Session, entity: &Entity) -> Result<[String; 3]> {
    ensure!(entity.kind == EntityKind::Player, "선수만 한글화할 수 있습니다");
    let process = Process::open(session.pid, false)?;
    validate_with(&process, session)?;
    validate_person(&process, session, entity)?;
    let names = read_person_name_parts_with(entity.address, |address, size| process.read(address, size))?;
    validate_person(&process, session, entity)?;
    validate_with(&process, session)?;
    Ok(names)
}
pub(crate) fn refresh_person_names_with(
    index: &mut Index,
    expected: &Entity,
    read: impl FnOnce(&Session, &Entity) -> Result<(String, String)>,
) -> Result<Entity> {
    ensure!(matches!(expected.kind, EntityKind::Player | EntityKind::Staff | EntityKind::Person), "인물 이름만 갱신할 수 있습니다");
    let mut matches = index.entities.iter_mut().filter(|e| e.kind == expected.kind && e.id == expected.id);
    let target = matches.next().context("이름을 갱신할 인물이 검색 목록에 없습니다")?;
    ensure!(matches.next().is_none(), "이름을 갱신할 인물 ID가 중복되었습니다");
    ensure!(target.address == expected.address && target.base == expected.base && target.index == expected.index,
        "이름을 갱신할 인물의 연결 정보가 변경됐습니다");
    let (name, english_name) = read(&index.session, target)?;
    ensure!(!name.trim().is_empty() && !name.chars().chain(english_name.chars()).any(char::is_control),
        "다시 읽은 인물 이름이 유효하지 않습니다");
    // Both values become visible together only after all reads and guards pass.
    target.name = name;
    target.english_name = english_name;
    Ok(target.clone())
}
pub fn refresh_person_names(index: &mut Index, expected: &Entity) -> Result<Entity> {
    refresh_person_names_with(index, expected, |session, entity| {
        let process = Process::open(session.pid, false)?;
        validate_with(&process, session)?;
        validate_person(&process, session, entity)?;
        let names = read_person_names_with(entity.address, |address, size| process.read(address, size))?;
        validate_person(&process, session, entity)?;
        validate_with(&process, session)?;
        Ok(names)
    })
}
pub fn club_name(p: &Process, club: u64) -> String {
    let name = indirect_string(p, club + 0xC0);
    if name.is_empty() {
        indirect_string(p, club + 0xC8)
    } else {
        name
    }
}
fn club_for(p: &Process, person: u64) -> (u64, u64, u64) {
    let c = p.ptr(person + 0xA8);
    let t = if c != 0 { p.ptr(c + 0x10) } else { 0 };
    let cl = if t != 0 { p.ptr(t + 0x30) } else { 0 };
    (c, t, cl)
}

pub fn decode_date(raw: u32) -> Option<(u32, u32, u32)> {
    let y = raw >> 16;
    let mut d = raw & 511;
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    if !(1900..=2100).contains(&y) || d == 0 || d > if leap { 366 } else { 365 } {
        return None;
    }
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for (i, days) in months.iter().enumerate() {
        if d <= *days {
            return Some((y, i as u32 + 1, d));
        }
        d -= days;
    }
    None
}
fn date_text(d: (u32, u32, u32)) -> String {
    format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)
}
fn age(raw: u32, today: (u32, u32, u32)) -> Option<u32> {
    let b = decode_date(raw)?;
    let y = today.0.checked_sub(b.0)?;
    y.checked_sub(u32::from((today.1, today.2) < (b.1, b.2)))
        .filter(|&x| x < 130)
}
fn team_label(t: u8) -> String {
    match t {
        0 => "1군".into(),
        18 => "U-20".into(),
        12 => "U-18".into(),
        1 => "2군".into(),
        255 => "소속팀 미확인".into(),
        _ => format!("연령팀 {t}"),
    }
}
fn contract_group(has_team: bool, team_kind: u8) -> String {
    if !has_team {
        "무소속".into()
    } else {
        team_label(team_kind)
    }
}
// FM26 26.3.x evidence: FMSuperScout/plugin/Fields.cs PERO_GENDER,
// corroborated by the existing loaned_players.py and current player readers.
// This is a player's sex/category, never a staff member's employment category.
fn player_football_category(flags: Option<u8>) -> FootballCategory {
    match flags {
        Some(flags) if flags & 0x10 != 0 => FootballCategory::Women,
        Some(_) => FootballCategory::Men,
        None => FootballCategory::Unknown,
    }
}

/// No verified native club-category offset is assumed. Use every validated
/// contracted player, before roster views change display affiliations. Mixed,
/// missing, or unreadable evidence stays unknown instead of taking a majority.
fn club_football_categories(entities: &[Entity]) -> HashMap<u32, FootballCategory> {
    let mut evidence = HashMap::<u32, u8>::new();
    for e in entities.iter().filter(|e| e.kind == EntityKind::Player) {
        let Some(id) = e.club_id else { continue };
        let bit = match e.football_category {
            FootballCategory::Men => 1,
            FootballCategory::Women => 2,
            FootballCategory::Unknown => 4,
        };
        *evidence.entry(id).or_default() |= bit;
    }
    evidence.into_iter().map(|(id, flags)| {
        (id, match flags {
            1 => FootballCategory::Men,
            2 => FootballCategory::Women,
            _ => FootballCategory::Unknown,
        })
    }).collect()
}

fn assign_club_football_categories(
    entities: &mut [Entity],
    categories: &HashMap<u32, FootballCategory>,
) {
    for e in entities.iter_mut().filter(|e| {
        matches!(e.kind, EntityKind::Club | EntityKind::Staff | EntityKind::Person)
    }) {
        e.football_category = e.club_id
            .and_then(|id| categories.get(&id).copied())
            .unwrap_or_default();
    }
}
pub fn job_label(n: u8) -> String {
    match n {
        1 => "선수",
        2 => "코치",
        3 => "선수 겸 코치",
        4 => "회장",
        6 => "이사",
        8 => "운영 이사",
        10 => "풋볼 디렉터",
        12 => "물리치료사",
        14 => "스카우트",
        16 => "감독",
        17 => "선수 겸 감독",
        20 => "수석 코치",
        21 => "선수 겸 수석 코치",
        22 => "세트피스 코치",
        24 => "단장",
        26 => "피트니스 코치",
        27 => "선수 겸 피트니스 코치",
        34 => "골키퍼 코치",
        35 => "선수 겸 골키퍼 코치",
        36 => "수석 분석가",
        38 => "구단 의사",
        40 => "수석 스포츠 과학자",
        42 => "분석가",
        44 => "수석 스카우트",
        45 => "선수 겸 수석 스카우트",
        46 => "의사",
        48 => "스포츠 과학자",
        49 => "선수 겸 유소년 코치",
        50 => "수석 물리치료사",
        52 => "U-19 감독",
        54 => "1군 코치",
        58 => "영입 분석가",
        60 => "전력 분석가",
        62 => "수석 전력 분석가",
        64 => "수석 유소년 육성가",
        65 => "선수 겸 유소년 육성가",
        66 => "구단주",
        70 => "대표",
        86 => "임대 매니저",
        88 => "테크니컬 디렉터",
        144 => "감독 대행",
        _ => return format!("직무 코드 {n}"),
    }
    .into()
}
fn player_roles(positions: &[u8]) -> String {
    const POS: [&str; 15] = [
        "GK", "SW", "DL", "DC", "DR", "DM", "ML", "MC", "MR", "AML", "AMC", "AMR", "ST", "WBL",
        "WBR",
    ];
    positions
        .iter()
        .zip(POS)
        .filter(|(v, _)| **v >= 15)
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn word16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset+2].try_into().unwrap())
}
fn word32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn word64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn pointer(p: u64) -> u64 {
    if (0x10000..0x7fff_ffff_ffff).contains(&p) {
        p
    } else {
        0
    }
}
/// Name objects are interned across thousands of people. Cache only within this read pass.
fn cached_name(p: &Process, outer: u64, cache: &mut HashMap<u64, String>) -> String {
    if pointer(outer) == 0 {
        return String::new();
    }
    cache
        .entry(outer)
        .or_insert_with(|| {
            let inner = p.ptr(outer);
            if inner > 0 {
                p.string(inner + 4, 192)
            } else {
                String::new()
            }
        })
        .clone()
}

// Person+0x68 is the verified primary NATION link shared by players and staff
// (relations::snapshot). Reuse countries only within one index pass; never read
// a full relation/profile or open a process handle for every search-row nation.
fn cached_nationality_with(
    address: u64,
    cache: &mut HashMap<u64, Option<(u32, String)>>,
    read: impl FnOnce(u64) -> Result<(u32, String)>,
) -> Option<(u32, String)> {
    if pointer(address) == 0 {
        return None;
    }
    cache
        .entry(address)
        .or_insert_with(|| {
            let (id, name) = read(address).ok()?;
            (id > 0 && !name.trim().is_empty() && !name.chars().any(char::is_control))
                .then_some((id, name))
        })
        .clone()
}
fn cached_nationality(
    p: &Process,
    s: &Session,
    address: u64,
    cache: &mut HashMap<u64, Option<(u32, String)>>,
) -> Option<(u32, String)> {
    cached_nationality_with(address, cache, |nation| {
        let raw = p.read(nation, 16)?;
        ensure!(
            type_name(p, s, word64(&raw, 0)) == ".?AVNATION@db@@",
            "기본 국적 객체 형식 오류"
        );
        Ok((word32(&raw, 12), indirect_string(p, nation + 0x20)))
    })
}

pub fn connect(progress: impl Fn(String)) -> Result<Index> {
    progress("실행 중인 Football Manager와 빌드를 확인하고 있습니다".into());
    let candidates = discovery::candidates()?;
    ensure!(
        !candidates.is_empty(),
        "{}", discovery::LOCAL_GAME_GUIDANCE
    );
    let mut reasons = Vec::new();
    for (pid, name) in candidates {
        match connect_pid(pid, &progress) {
            Ok(index) => return Ok(index),
            Err(e) => reasons.push(format!("{name} / PID {pid}: {e:#}")),
        }
    }
    bail!("FM 연결 실패\n{}", reasons.join("\n"))
}
/// Build only the process selected by the stable identity probe. Connection
/// recovery must not silently switch to a different running FM instance.
pub(crate) fn connect_pid(pid: u32, progress: &impl Fn(String)) -> Result<Index> {
    let p = Process::open(pid, false)?;
    let m = p
        .modules()?
        .into_iter()
        .find(|m| m.name.eq_ignore_ascii_case("game_plugin.dll"))
        .context("게임 데이터 모듈이 로드되지 않았습니다")?;
    let build=detect_build(&p,&m)?;
    let profile=require_read_profile(&build)
        .with_context(||discovery::module_context(&m.path))?; // No object reads before profile validation.
    let layout=profile.require_read_layout()?;
    let (stamp,size)=(build.timestamp,build.image_size);
    let db = p.u64(m.base + layout.database_rva)?;
    let (rb, re) = registry_with_layout(&p, m.base, db,layout)?;
    let count=(re-rb)/8;
    let mut sampled_people=0;
    for ix in [0,count/4,count/2,count*3/4,count-1] {
        let address=p.u64(rb+ix*8)?;
        if address==0 {continue;}
        let ident=p.read(address,16)?;
        let (_,kind)=type_info(&p,m.base,size,word64(&ident,0))?;
        ensure!(word32(&ident,8) as u64==ix && is_person_registry_type(&kind),
            "인물 레지스트리 표본 검증 실패: {kind}");
        sampled_people+=1;
    }
    ensure!(sampled_people>0,"인물 레지스트리에 유효한 표본이 없습니다");
    let today = decode_date(p.u32(m.base + layout.date_rva)?).context("게임 날짜 검증 실패")?;
    let session = Session {
        pid,
        created: p.created()?,
        module_base: m.base,
        module_size: size,
        module_stamp: stamp,
        db,
        registry_begin: rb,
        game_date: date_text(today),
        profile: profile.label.into(),
        own_club_id: None,
    };
    let registry_bytes = p.read(rb, (re - rb) as usize)?;
    let people = registry_bytes
        .chunks_exact(8)
        .map(|x| u64::from_le_bytes(x.try_into().unwrap()))
        .collect::<Vec<_>>();
    let mut kinds: HashMap<u64, Option<(u32, String)>> = HashMap::new();
    let mut clubs: HashMap<u64, (u32, String)> = HashMap::new();
    let mut teams: HashMap<u64, (u8, u64)> = HashMap::new();
    let mut names: HashMap<u64, String> = HashMap::new();
    let mut nationalities: HashMap<u64, Option<(u32, String)>> = HashMap::new();
    let mut rows = Vec::with_capacity(people.len());
    let mut human_clubs = HashSet::new();
    let mut errors = 0;
    for (index, &person) in people.iter().enumerate() {
        if index % 25_000 == 0 {
            progress(format!("인물 목록 읽는 중 · {index} / {}", people.len()));
        }
        if person == 0 {
            continue;
        }
        let outcome = (|| -> Result<Option<Entity>> {
            // One bounded read covers identity, three name references, DOB and contract.
            let h = p.read(person, 0xB8)?;
            let vt = word64(&h, 0);
            let ix = word32(&h, 8);
            let id = word32(&h, 12);
            if ix as usize != index || id == 0 || id >= 0x8000_0000 {
                return Ok(None);
            }
            let info = kinds
                .entry(vt)
                .or_insert_with(|| type_info(&p, m.base, m.size, vt).ok());
            let Some((offset, name)) = info else {
                return Ok(None);
            };
            let (kind, base) = if (*offset == 0x288 || *offset == 0x380)
                && name.contains("PLAYER")
                && !name.contains("VIRTUAL")
            {
                (EntityKind::Player, person - *offset as u64)
            } else if name == ".?AVACTUAL_NON_PLAYER@db@@" && *offset == 0x100 {
                (EntityKind::Staff, person - 0x100)
            } else if name.contains("PERSON") || name.contains("NON_PLAYER") {
                (EntityKind::Person, person)
            } else {
                return Ok(None);
            };
            let (ca, pa, player_role, role_hint) = if kind == EntityKind::Player {
                // Positions through CA/PA are contiguous inside the verified player block.
                let data = p.read(base + 0x150, 0x118)?;
                let a = u16::from_le_bytes(data[0x114..0x116].try_into().unwrap());
                let b = u16::from_le_bytes(data[0x116..0x118].try_into().unwrap());
                ensure!(
                    (1..=200).contains(&a) && (1..=200).contains(&b),
                    "잘못된 선수 능력치"
                );
                (Some(a), Some(b), player_roles(&data[..15]), 0)
            } else if kind == EntityKind::Staff {
                let data = p.read(base + 0xDA, 5)?;
                let a = u16::from_le_bytes(data[..2].try_into().unwrap());
                let b = u16::from_le_bytes(data[2..4].try_into().unwrap());
                if a <= 200 && b <= 200 {
                    (Some(a), Some(b), String::new(), data[4])
                } else {
                    (None, None, String::new(), data[4])
                }
            } else {
                (None, None, String::new(), 0)
            };
            let c = pointer(word64(&h, 0xA8));
            let contract = if c > 0 {
                p.read(c + 0x10, 0x17).ok()
            } else {
                None
            };
            let t = contract
                .as_ref()
                .map(|b| pointer(word64(b, 0)))
                .unwrap_or(0);
            let job = contract.as_ref().map(|b| b[0x16]).unwrap_or(role_hint);
            let (team_kind, cl) = if t > 0 {
                *teams.entry(t).or_insert_with(|| {
                    p.read(t + 0x28, 0x10)
                        .ok()
                        .map(|b| (b[0], pointer(word64(&b, 8))))
                        .unwrap_or((255, 0))
                })
            } else {
                (255, 0)
            };
            if cl != 0 && !clubs.contains_key(&cl) {
                let cuid = p.u32(cl + 12)?;
                let cname = club_name(&p, cl);
                if cuid > 0 && !cname.is_empty() {
                    clubs.insert(cl, (cuid, cname));
                }
            }
            let (club_id, club) = clubs
                .get(&cl)
                .map(|(i, n)| (Some(*i), n.clone()))
                .unwrap_or((None, String::new()));
            if name == ".?AVHUMAN_NON_PLAYER@db@@" {
                if let Some(i) = club_id {
                    human_clubs.insert(i);
                }
            }
            let first = cached_name(&p, word64(&h, 0x50), &mut names);
            let second = cached_name(&p, word64(&h, 0x58), &mut names);
            let english_name = format!("{first} {second}").trim().to_string();
            let common = cached_name(&p, word64(&h, 0x60), &mut names);
            let person_name = if common.is_empty() {
                english_name.clone()
            } else {
                common
            };
            if person_name.is_empty() || person_name.chars().any(char::is_control) {
                return Ok(None);
            }
            let role = if kind == EntityKind::Player {
                player_role
            } else if kind == EntityKind::Staff {
                job_label(job)
            } else if name.contains("HUMAN") {
                "사용자 감독".into()
            } else {
                "기타 인물".into()
            };
            // Player groups are resolved from roster vectors when opening a
            // club. Staff contract teams already identify their assignment.
            let group = contract_group(t != 0, team_kind);
            let nationality = cached_nationality(&p, &session, word64(&h, 0x68), &mut nationalities);
            let (nationality_id, nationality) = nationality
                .map(|(id, name)| (Some(id), name))
                .unwrap_or((None, String::new()));
            Ok(Some(Entity {
                id,
                kind,
                name: person_name,
                english_name,
                nationality,
                nationality_id,
                club,
                club_id,
                age: age(word32(&h, 0x88), today),
                ca,
                pa,
                role,
                group,
                football_category: if kind == EntityKind::Player {
                    player_football_category(h.get(0x19).copied())
                } else {
                    FootballCategory::Unknown
                },
                address: person,
                base,
                index: Some(ix),
            }))
        })();
        match outcome {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => {}
            Err(_) => errors += 1,
        }
    }
    let football_categories = club_football_categories(&rows);
    let mut index = Index {
        session,
        entities: rows,
        club_leagues: Vec::new(),
        warnings: Vec::new(),
    };
    if human_clubs.len() == 1 {
        index.session.own_club_id = human_clubs.iter().next().copied();
    } else if human_clubs.len() > 1 {
        index
            .warnings
            .push("고용된 사용자 감독이 여러 명이라 내 구단을 자동 선택하지 않았습니다".into());
    } else {
        index.warnings.push(
            "현재 고용된 사용자 감독을 찾지 못했습니다. 전체 목록에서 구단을 검색하세요".into(),
        );
    }
    for (address, (id, name)) in &clubs {
        index.entities.push(Entity {
            nationality: String::new(),
            nationality_id: None,
            id: *id,
            kind: EntityKind::Club,
            name: name.clone(),
            english_name: name.clone(),
            club: name.clone(),
            club_id: Some(*id),
            age: None,
            ca: None,
            pa: None,
            role: "구단".into(),
            group: String::new(),
            football_category: FootballCategory::Unknown,
            address: *address,
            base: *address,
            index: None,
        });
    }
    assign_club_football_categories(&mut index.entities, &football_categories);
    progress("내 구단 선수단과 도시 정보를 정리하고 있습니다".into());
    if let Some(id) = index.session.own_club_id {
        if let Some((&cl, _)) = clubs.iter().find(|(_, c)| c.0 == id) {
            mark_squad(&p, &mut index, cl, id);
        }
    }
    crate::extras::append_clubs_and_cities(&p, &mut index)?;
    progress("구단의 실제 소속 리그를 정리하고 있습니다".into());
    crate::leagues::populate(&p, &mut index);
    // Clubs appended from the wider database have no player evidence in this
    // index, so their default category remains unknown.
    validate_with(&p, &index.session)?;
    if errors > 0 {
        index.warnings.push(format!(
            "검증되지 않거나 읽는 동안 변경된 인물 {errors}개를 제외했습니다"
        ));
    }
    index
        .warnings
        .push("고용 의사·이적 의향은 이 목록의 수치로 확정할 수 없습니다".into());
    let unknown_clubs = index.entities.iter().filter(|e| {
        e.kind == EntityKind::Club && e.football_category == FootballCategory::Unknown
    }).count();
    let unknown_staff = index.entities.iter().filter(|e| {
        e.kind == EntityKind::Staff && e.football_category == FootballCategory::Unknown
    }).count();
    index.warnings.push(format!(
        "구단·스태프의 남녀 축구 구분은 검증된 소속 선수 전체가 일치할 때만 표시합니다. 선수 정보가 없거나 혼합된 구단 및 무소속 스태프는 미확인입니다 (구단 {unknown_clubs}개 · 스태프 {unknown_staff}명)"
    ));
    progress(format!("연결 완료 · {}개 레코드", index.entities.len()));
    Ok(index)
}
fn vector(p: &Process, a: u64, max: u64) -> Result<Vec<u64>> {
    let b = p.u64(a)?;
    let e = p.u64(a + 8)?;
    ensure!(e >= b && e - b <= max && (e - b) % 8 == 0, "벡터 범위 오류");
    if b == e {
        return Ok(Vec::new());
    }
    Ok(p.read(b, (e - b) as usize)?
        .chunks_exact(8)
        .map(|x| u64::from_le_bytes(x.try_into().unwrap()))
        .collect())
}

#[derive(Clone, Debug)]
enum StaffAffiliation {
    Unemployed,
    Employed { club_id:u32, club:String, team_kind:u8, job:u8, category:FootballCategory },
    Unknown,
}
fn apply_staff_affiliation(entity:&mut Entity, affiliation:&StaffAffiliation) {
    match affiliation {
        StaffAffiliation::Unemployed => {
            entity.club_id=None;entity.club.clear();entity.group="무소속".into();entity.role="무직".into();
            entity.football_category=FootballCategory::Unknown;
        }
        StaffAffiliation::Employed {club_id,club,team_kind,job,category} => {
            entity.club_id=Some(*club_id);entity.club=club.clone();entity.group=contract_group(true,*team_kind);
            // A zero job is missing role evidence. Keep it empty so negative
            // search comparisons cannot turn an unknown job into a match.
            entity.role=if *job==0 {String::new()} else {job_label(*job)};entity.football_category=*category;
        }
        StaffAffiliation::Unknown => {
            entity.club_id=None;entity.club.clear();entity.group=STAFF_AFFILIATION_UNKNOWN.into();
            entity.role="직무 미확인".into();entity.football_category=FootballCategory::Unknown;
        }
    }
}
fn require_staff_affiliation_profile(session:&Session)->Result<()> {
    ensure!(profiles::profile_for_session(session).is_some_and(|p|p.read_verified &&
        (p.id==profiles::FM26.id || p.id==profiles::FM27.id)),"스태프 소속을 읽을 빌드가 검증되지 않았습니다");
    Ok(())
}
/// A cache belongs to one observation phase, never to a session or UI view.
/// Aligned reads coalesce the many tiny relationship reads. Unreadable pages
/// fall back to the exact requested range, so a nearby inaccessible byte does
/// not turn an otherwise readable relationship into Unknown.
struct StaffReadPages<'a> {
    read:&'a dyn Fn(u64,usize)->Result<Vec<u8>>,
    pages:std::cell::RefCell<HashMap<u64,Option<Vec<u8>>>>,
}
impl<'a> StaffReadPages<'a> {
    const PAGE:usize=4096;
    const MAX_PAGES:usize=8192; // 32 MiB, including failed-page entries.
    fn new(read:&'a dyn Fn(u64,usize)->Result<Vec<u8>>)->Self {Self {read,pages:Default::default()}}
    fn clear(&self) {self.pages.borrow_mut().clear();}
    fn read(&self,address:u64,size:usize)->Result<Vec<u8>> {
        ensure!(size<=Self::PAGE&&address.checked_add(size as u64).is_some(),"스태프 소속 읽기 범위 오류");
        let mut bytes=Vec::with_capacity(size);let mut cursor=address;
        while bytes.len()<size {
            let page=cursor&!(Self::PAGE as u64-1);let offset=(cursor-page) as usize;
            let take=(size-bytes.len()).min(Self::PAGE-offset);
            if !self.pages.borrow().contains_key(&page) {
                let value=(self.read)(page,Self::PAGE).ok().filter(|v|v.len()==Self::PAGE);
                let mut pages=self.pages.borrow_mut();if pages.len()>=Self::MAX_PAGES {pages.clear();}
                pages.insert(page,value);
            }
            let pages=self.pages.borrow();
            let Some(Some(raw))=pages.get(&page) else {
                drop(pages);return (self.read)(address,size);
            };
            bytes.extend_from_slice(&raw[offset..offset+take]);cursor+=take as u64;
        }
        Ok(bytes)
    }
}
/// Only identity and primary-contract links; no staff attributes or native role-list claims.
struct StaffAffiliationCapture<'a> {
    session:&'a Session,
    registry_end:u64,
    read:&'a dyn Fn(u64,usize)->Result<Vec<u8>>,
    info:&'a dyn Fn(u64)->Result<(u32,String)>,
    name:&'a dyn Fn(u64)->String,
    club_metadata:HashMap<u64,(u32,String,FootballCategory)>,
    types:HashMap<u64,(u32,String)>,
    teams:HashMap<u64,(u8,u64)>,
    clubs:HashMap<u64,(u32,String,FootballCategory)>,
    guards:Vec<Guard>,
    guarded_values:HashMap<(u64,usize),usize>,
}
impl StaffAffiliationCapture<'_> {
    fn guarded(&mut self,address:u64,size:usize)->Result<Vec<u8>> {
        if let Some(i)=self.guarded_values.get(&(address,size)) {return Ok(self.guards[*i].expected.clone());}
        let raw=(self.read)(address,size)?;ensure!(raw.len()==size,"스태프 소속 읽기 길이 불일치");
        self.guarded_values.insert((address,size),self.guards.len());
        self.guards.push(Guard {address,expected:raw.clone()});Ok(raw)
    }
    fn ptr(value:u64)->Result<u64> {ensure!(pointer(value)==value&&value!=0&&value%8==0,"스태프 소속 포인터 미확인");Ok(value)}
    fn typed(&mut self,vtable:u64,offset:u32,names:&[&str])->Result<()> {
        if !self.types.contains_key(&vtable) {self.types.insert(vtable,(self.info)(vtable)?);}
        let (actual,name)=&self.types[&vtable];
        ensure!(*actual==offset&&names.contains(&name.as_str()),"스태프 소속 객체 유형 미확인");Ok(())
    }
    fn object(&mut self,address:u64,name:&str)->Result<u32> {
        Self::ptr(address)?;let raw=self.guarded(address,16)?;self.typed(word64(&raw,0),0,&[name])?;
        let uid=word32(&raw,12);ensure!(uid>0&&uid<0x8000_0000,"스태프 소속 객체 ID 미확인");Ok(uid)
    }
    fn staff(&mut self,entity:&Entity)->Result<StaffAffiliation> {
        ensure!(entity.kind==EntityKind::Staff,"스태프 대상이 아닙니다");
        let index=entity.index.context("스태프 인물 목록 신원 미확인")?;
        let slot=self.session.registry_begin.checked_add(index as u64*8).context("스태프 목록 범위 오류")?;
        ensure!(slot<self.registry_end,"스태프 목록 범위가 변경됐습니다");
        ensure!(word64(&self.guarded(slot,8)?,0)==entity.address,"스태프 인물 목록 연결이 변경됐습니다");
        let raw=self.guarded(entity.address,16)?;
        ensure!(word32(&raw,8)==index&&word32(&raw,12)==entity.id&&entity.address.checked_sub(0x100)==Some(entity.base),"스태프 신원이 변경됐습니다");
        self.typed(word64(&raw,0),0x100,&[".?AVACTUAL_NON_PLAYER@db@@"])?;
        let contract=word64(&self.guarded(entity.address+0xA8,8)?,0);
        if contract==0 {return Ok(StaffAffiliation::Unemployed);}
        Self::ptr(contract)?;
        let vtable=word64(&self.guarded(contract,8)?,0);
        self.typed(vtable,0,&[".?AVFULL_CONTRACT@db@@",".?AVBASIC_CONTRACT@db@@"])?;
        ensure!(word64(&self.guarded(contract+8,8)?,0)==entity.address,"현재 계약의 인물 연결이 다릅니다");
        let team=Self::ptr(word64(&self.guarded(contract+0x10,8)?,0))?;
        let job=self.guarded(contract+0x26,1)?[0];
        if !self.teams.contains_key(&team) {
            self.object(team,".?AVTEAM@db@@")?;
            let kind=self.guarded(team+0x28,1)?[0];
            let club=Self::ptr(word64(&self.guarded(team+0x30,8)?,0))?;
            self.teams.insert(team,(kind,club));
        }
        let (team_kind,club)=self.teams[&team];
        if !self.clubs.contains_key(&club) {
            let uid=self.object(club,".?AVCLUB@db@@")?;
            let (name,category)=self.club_metadata.get(&club).filter(|(id,_,_)|*id==uid)
                .map(|(_,name,category)|(name.clone(),*category)).unwrap_or_else(||((self.name)(club),FootballCategory::Unknown));
            self.clubs.insert(club,(uid,if name.is_empty(){format!("구단 {uid}")}else{name},category));
        }
        let (club_id,club,category)=self.clubs[&club].clone();
        Ok(StaffAffiliation::Employed {club_id,club,team_kind,job,category})
    }
    fn verify(&self)->Result<()> {
        for guard in &self.guards {ensure!((self.read)(guard.address,guard.expected.len())?==guard.expected,"스태프 소속을 읽는 동안 정보가 변경됐습니다");}
        Ok(())
    }
}
fn capture_staff_affiliations_with(
    index:&Index,registry_end:u64,read:&dyn Fn(u64,usize)->Result<Vec<u8>>,
    info:&dyn Fn(u64)->Result<(u32,String)>,name:&dyn Fn(u64)->String,end:&dyn Fn()->Result<u64>,
)->Result<Vec<(usize,StaffAffiliation)>> {
    require_staff_affiliation_profile(&index.session)?;
    let pages=StaffReadPages::new(read);let read_cached=|address,size|pages.read(address,size);
    let mut capture=StaffAffiliationCapture {session:&index.session,registry_end,read:&read_cached,info,name,
        club_metadata:index.entities.iter().filter(|e|e.kind==EntityKind::Club)
            .map(|e|(e.address,(e.id,e.name.clone(),e.football_category))).collect(),
        types:HashMap::new(),teams:HashMap::new(),clubs:HashMap::new(),guards:vec![],guarded_values:HashMap::new()};
    let rows=index.entities.iter().enumerate().filter(|(_,e)|e.kind==EntityKind::Staff)
        .map(|(i,e)|(i,capture.staff(e).unwrap_or(StaffAffiliation::Unknown))).collect();
    // Every guarded byte must be observed again. Reusing initial pages here
    // would make torn-read validation silently compare a snapshot to itself.
    pages.clear();
    capture.verify()?;ensure!(end()?==registry_end,"스태프 인물 목록 범위가 변경됐습니다");Ok(rows)
}
fn apply_staff_affiliations(index:&mut Index,rows:Vec<(usize,StaffAffiliation)>) {
    let unknown=rows.iter().filter(|(_,a)|matches!(a,StaffAffiliation::Unknown)).count();
    for (i,affiliation) in rows {apply_staff_affiliation(&mut index.entities[i],&affiliation);}
    index.warnings.retain(|w|!w.starts_with("스태프 소속:"));
    if unknown>0 {index.warnings.push(format!("스태프 소속: {unknown}명의 현재 계약을 확인하지 못해 소속·직무를 미확인으로 표시합니다. 무직으로 간주하지 않습니다."));}
}
pub fn refresh_staff_affiliations(index:&mut Index)->Result<()> {
    require_staff_affiliation_profile(&index.session)?;
    let p=Process::open(index.session.pid,false)?;validate_with(&p,&index.session)?;
    let (_,end)=registry(&p,index.session.module_base,index.session.db)?;
    let rows=capture_staff_affiliations_with(index,end,&|a,n|p.read(a,n),
        &|vt|type_info(&p,index.session.module_base,index.session.module_size,vt),&|club|club_name(&p,club),
        &||Ok(registry(&p,index.session.module_base,index.session.db)?.1))?;
    validate_with(&p,&index.session)?;apply_staff_affiliations(index,rows);Ok(())
}
fn staff_detail_affiliation(p:&Process,session:&Session,entity:&Entity)->Result<(StaffAffiliation,Vec<Guard>)> {
    require_staff_affiliation_profile(session)?;
    let (_,registry_end)=registry(p,session.module_base,session.db)?;
    let read=|a,n|p.read(a,n);let info=|vt|type_info(p,session.module_base,session.module_size,vt);let name=|club|club_name(p,club);
    let mut capture=StaffAffiliationCapture {session,registry_end,read:&read,info:&info,name:&name,
        club_metadata:HashMap::new(),types:HashMap::new(),teams:HashMap::new(),clubs:HashMap::new(),
        guards:vec![],guarded_values:HashMap::new()};
    let mut affiliation=capture.staff(entity).unwrap_or(StaffAffiliation::Unknown);capture.verify()?;
    ensure!(registry(p,session.module_base,session.db)?.1==registry_end,"스태프 인물 목록 범위가 변경됐습니다");
    if let StaffAffiliation::Employed {club_id,category,..}=&mut affiliation {
        if entity.club_id==Some(*club_id) {*category=entity.football_category;}
    }
    Ok((affiliation,capture.guards))
}
fn mark_squad(p: &Process, index: &mut Index, club: u64, clubid: u32) {
    let cname = club_name(p, club);
    // A previous club view may have shown this player as an incoming loan.
    // Recover the actual employer before applying this club's roster so view
    // order cannot hide the parent's outgoing-loan row or lose the loan badge.
    for e in index.entities.iter_mut().filter(|e| {
        e.kind == EntityKind::Player
            && (e.club_id == Some(clubid) || e.group.ends_with(" · 임대 영입"))
    }) {
        let (_, team, employer) = club_for(p, e.address);
        if employer == 0 || (employer != club && e.club_id != Some(clubid)) {
            continue;
        }
        let Ok(employer_id) = p.u32(employer + 12) else {
            continue;
        };
        if employer_id == 0 || employer_id >= 0x8000_0000 {
            continue;
        }
        restore_player_employer(
            e,
            employer_id,
            &club_name(p, employer),
            p.u8(team + 0x28).unwrap_or(255),
        );
    }
    let mut lookup = HashMap::new();
    for (i, e) in index
        .entities
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == EntityKind::Player)
    {
        lookup.insert(e.address, i);
        lookup.insert(e.base, i);
    }
    // Contract team establishes the employer, not the active squad. Start afresh
    // and use only explicit roster memberships when assigning the player's group.
    for e in index
        .entities
        .iter_mut()
        .filter(|e| e.kind == EntityKind::Player && e.club_id == Some(clubid))
    {
        e.group = "소속팀 미확인".into();
    }
    let mut memberships: HashMap<usize, u8> = HashMap::new();
    for t in vector(p, club + 0x18, 1024).unwrap_or_default() {
        if p.ptr(t + 0x30) != club {
            continue;
        }
        let kind = p.u8(t + 0x28).unwrap_or(255);
        for entry in vector(p, t + 0x38, 16000).unwrap_or_default() {
            let item = lookup.get(&entry).copied().or_else(|| {
                (0..=0x80)
                    .step_by(8)
                    .find_map(|d| lookup.get(&p.ptr(entry + d)).copied())
            });
            if let Some(i) = item {
                memberships
                    .entry(i)
                    .and_modify(|old| *old = preferred_squad(*old, kind))
                    .or_insert(kind);
            }
        }
    }
    assign_memberships(&mut index.entities, &memberships, clubid, &cname);
    for e in index
        .entities
        .iter_mut()
        .filter(|e| e.kind == EntityKind::Player && e.club_id == Some(clubid))
    {
        let holder = p.ptr(e.address + 0xB0);
        let loan = if holder > 0 { p.ptr(holder) } else { 0 };
        if loan > 0
            && type_name(p, &index.session, p.ptr(loan)) == ".?AVLOAN_CONTRACT@db@@"
            && p.ptr(loan + 8) == e.address
        {
            let t = p.ptr(loan + 0x10);
            let c = if t > 0 { p.ptr(t + 0x30) } else { 0 };
            if c > 0 && c != club {
                e.group = format!("임대 중 · {}", club_name(p, c));
            }
        }
    }
}
fn restore_player_employer(e: &mut Entity, clubid: u32, cname: &str, team_kind: u8) {
    e.club_id = Some(clubid);
    e.club = cname.into();
    e.group = contract_group(true, team_kind);
}
fn assign_memberships(
    entities: &mut [Entity],
    memberships: &HashMap<usize, u8>,
    clubid: u32,
    cname: &str,
) {
    for (&i, &kind) in memberships {
        let e = &mut entities[i];
        e.group = team_label(kind);
        if e.club_id != Some(clubid) {
            e.club_id = Some(clubid);
            e.club = cname.into();
            e.group.push_str(" · 임대 영입");
        }
    }
}
/// Re-read the managed club's player roster and staff assignments.
pub fn refresh_squad(index: &mut Index) -> Result<()> {
    let id = index
        .session
        .own_club_id
        .context("내 구단이 식별되지 않았습니다")?;
    refresh_club_squad(index, id)?;
    refresh_staff_affiliations(index)
}

/// Resolve a club roster on demand instead of treating a player's contract
/// team as its active team. Works for club-scoped searches beyond the own club.
pub fn refresh_club_squad(index: &mut Index, id: u32) -> Result<()> {
    let p = Process::open(index.session.pid, false)?;
    validate_with(&p, &index.session)?;
    index.session.game_date = date_text(
        decode_date(p.u32(index.session.module_base + profiles::profile_for_session(&index.session).context("세션 빌드가 검증되지 않았습니다")?.require_read_layout()?.date_rva)?)
            .context("게임 날짜 검증 실패")?,
    );
    let club = index
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Club && e.id == id)
        .context("구단 레코드가 없습니다")?
        .address;
    ensure!(
        p.u32(club + 12)? == id && type_name(&p, &index.session, p.u64(club)?) == ".?AVCLUB@db@@",
        "구단이 변경됐습니다"
    );
    mark_squad(&p, index, club, id);
    validate_with(&p, &index.session)
}
fn preferred_squad(a: u8, b: u8) -> u8 {
    // The senior squad wins only when the player actually appears in its roster.
    let priority = |t| match t {
        0 => 0,
        18 => 1,
        12 => 2,
        _ => 3,
    };
    if priority(b) < priority(a) { b } else { a }
}
pub fn validate_with(p: &Process, s: &Session) -> Result<()> {
    ensure!(
        p.pid == s.pid && p.created()? == s.created,
        "FM이 재시작됐습니다. 다시 연결하세요"
    );
    let m = p
        .modules()?
        .into_iter()
        .find(|m| m.name.eq_ignore_ascii_case("game_plugin.dll"))
        .context("FM 데이터 모듈이 없습니다")?;
    let build=detect_build(p,&m)?;
    let profile=require_read_profile(&build)?;
    let layout=profile.require_read_layout()?;
    let (stamp,size)=(build.timestamp,build.image_size);
    ensure!(
        m.base == s.module_base
            && stamp == s.module_stamp
            && size == s.module_size
            && s.profile == profile.label,
        "FM 모듈/빌드가 변경됐습니다. 다시 연결하세요"
    );
    ensure!(
        p.u64(m.base + layout.database_rva)? == s.db,
        "세이브가 변경됐습니다. 다시 연결하세요"
    );
    let (begin, _) = registry_with_layout(p, m.base, s.db,layout)?;
    ensure!(
        begin == s.registry_begin,
        "인물 목록이 다시 로드됐습니다. 다시 연결하세요"
    );
    Ok(())
}
pub fn validate_session(s: &Session) -> Result<()> {
    validate_with(&Process::open(s.pid, false)?, s)
}
fn validate_person(p: &Process, s: &Session, e: &Entity) -> Result<u32> {
    let ix = e.index.context("인물 레지스트리 인덱스가 없습니다")?;
    let (_, end) = registry(&p, s.module_base, s.db)?;
    ensure!(
        s.registry_begin + ix as u64 * 8 < end,
        "인물 목록 범위가 변경됐습니다"
    );
    ensure!(
        p.u64(s.registry_begin + ix as u64 * 8)? == e.address
            && p.u32(e.address + 8)? == ix
            && p.u32(e.address + 12)? == e.id,
        "선택한 인물이 변경됐습니다. 다시 조회하세요"
    );
    let (offset, typ) = type_info(p, s.module_base, s.module_size, p.u64(e.address)?)?;
    match e.kind {
        EntityKind::Player => ensure!(
            (offset == 0x288 || offset == 0x380)
                && e.address - offset as u64 == e.base
                && typ.contains("PLAYER")
                && !typ.contains("VIRTUAL"),
            "선수 객체 검증 실패"
        ),
        EntityKind::Staff => ensure!(
            offset == 0x100 && e.address - 0x100 == e.base && typ == ".?AVACTUAL_NON_PLAYER@db@@",
            "스태프 객체 검증 실패"
        ),
        EntityKind::Person => ensure!(
            e.base == e.address && (typ.contains("PERSON") || typ.contains("NON_PLAYER")),
            "인물 객체 검증 실패"
        ),
        _ => {}
    }
    Ok(ix)
}
pub fn detail(s: &Session, e: &Entity) -> Result<Detail> {
    let p = Process::open(s.pid, false)?;
    validate_with(&p, s)?;
    if matches!(e.kind, EntityKind::Club | EntityKind::City) {
        let mut detail = crate::extras::detail(&p, s, e)?;
        crate::compatibility::constrain_detail(s, &mut detail);
        return Ok(detail);
    }
    let ix = validate_person(&p, s, e)?;
    let mut guards = vec![
        Guard {
            address: s.registry_begin + ix as u64 * 8,
            expected: e.address.to_le_bytes().to_vec(),
        },
        Guard {
            address: e.address,
            expected: p.read(e.address, 16)?,
        },
    ];
    if e.base != e.address {
        guards.push(Guard {
            address: e.base,
            expected: p.read(e.base, 8)?,
        });
    }
    // Staff affiliations use the strict primary-contract reader below; the
    // permissive display helper must not turn a failed read into unemployment.
    let (c, t, cl) = if e.kind==EntityKind::Staff {(0,0,0)}else{club_for(&p, e.address)};
    if e.kind!=EntityKind::Staff {guards.push(Guard {
        address: e.address + 0xA8,
        expected: p.read(e.address + 0xA8, 8)?,
    });}
    if c > 0 {
        guards.push(Guard {
            address: c + 0x10,
            expected: p.read(c + 0x10, 8)?,
        });
    }
    if t > 0 {
        guards.push(Guard {
            address: t + 0x30,
            expected: p.read(t + 0x30, 8)?,
        });
    }
    if cl > 0 {
        guards.push(Guard {
            address: cl,
            expected: p.read(cl, 16)?,
        });
    }
    let mut d = Detail {
        entity: e.clone(),
        fields: Vec::new(),
        guards,
        notes: vec!["이름·국적·관계는 전용 편집에서 대상과 변경 내용을 검토합니다".into()],
    };
    if e.kind==EntityKind::Staff {
        let (affiliation,guards)=staff_detail_affiliation(&p,s,e)?;
        apply_staff_affiliation(&mut d.entity,&affiliation);
        for guard in guards {if !d.guards.contains(&guard) {d.guards.push(guard);}}
        if matches!(affiliation,StaffAffiliation::Unknown) {
            d.notes.push("현재 주계약의 소속·직무를 확인하지 못했습니다. 무직을 뜻하지 않습니다.".into());
        }
    }
    let legacy = crate::compatibility::legacy_layout(s);
    if !legacy && e.kind == EntityKind::Person {
        d.entity.age = None;
        d.notes = vec!["FM27 기타 인물은 기본 정보만 조회합니다. 하위 인물 유형별 성격·생년월일 구조가 검증될 때까지 편집하지 않습니다".into()];
        validate_with(&p, s)?;
        return Ok(d);
    }
    crate::fields::populate(&p, &mut d, legacy)?;
    crate::extended::append(&p, s, &mut d);
    if legacy { if let Err(error) = crate::relations::append(&p, s, &mut d) {
        d.notes.push(format!("관계 상세를 읽을 수 없습니다: {error:#}"));
    } } else {
        d.notes.push("FM27: 능력치·성격·포지션을 우선 지원합니다. 관계·커리어·계약의 추가 구조는 검증 중입니다".into());
    }
    if legacy && e.kind == EntityKind::Player {
        crate::traits::append(&p, &mut d)?;
    }
    if let Ok(today) = crate::extended::parse_date_text(&s.game_date) {
        d.entity.age = age(p.u32(e.address + 0x88)?, today);
    }
    if let Some(field) = d.fields.iter().find(|f| f.key == "ca") {
        d.entity.ca = Some(field.value as u16);
    }
    if let Some(field) = d.fields.iter().find(|f| f.key == "pa") {
        d.entity.pa = Some(field.value as u16);
    }
    validate_with(&p, s)?;
    for g in &d.guards {
        ensure!(
            p.read(g.address, g.expected.len())? == g.expected,
            "조회 중 인물이 변경됐습니다. 다시 선택하세요"
        );
    }
    crate::compatibility::constrain_detail(s, &mut d);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put_staff_memory(memory:&mut HashMap<u64,u8>,address:u64,bytes:&[u8]) {
        for (i,b) in bytes.iter().enumerate() {memory.insert(address+i as u64,*b);}
    }
    fn staff_affiliation_fixture(profile:&profiles::BuildProfile)->(Index,HashMap<u64,u8>) {
        let mut staff=category_row(EntityKind::Staff,Some(11),FootballCategory::Men);
        staff.id=7;staff.name="Synthetic coach".into();staff.address=0x20100;staff.base=0x20000;staff.index=Some(0);
        staff.club="A".into();staff.role="코치".into();staff.group="1군".into();staff.ca=Some(100);staff.pa=Some(150);
        let mut player=staff.clone();player.kind=EntityKind::Player;player.id=8;player.address=0xDEAD000;player.base=0xDEAD000;
        let mut clubs=vec![];
        for (id,address,name,category) in [(11,0x50000,"A",FootballCategory::Men),(22,0x51000,"B",FootballCategory::Women)] {
            let mut club=category_row(EntityKind::Club,Some(id),category);club.id=id;club.name=name.into();club.address=address;club.base=address;clubs.push(club);
        }
        let index=Index {session:Session {pid:0,created:0,module_base:0,module_size:profile.image_size,module_stamp:profile.timestamp,
            db:0,registry_begin:0x10000,game_date:"2026-11-01".into(),profile:profile.label.into(),own_club_id:Some(11)},
            entities:[vec![staff,player],clubs].concat(),club_leagues:vec![],warnings:vec![]};
        let mut memory=HashMap::new();
        for (address,size) in [(0x20100,16),(0x30000,0x28),(0x40000,0x38),(0x41000,0x38),(0x50000,16),(0x51000,16)] {put_staff_memory(&mut memory,address,&vec![0;size]);}
        for (address,value) in [(0x10000,0x20100u64),(0x20100,0x60000),(0x201A8,0x30000),(0x30000,0x61000),
            (0x30008,0x20100),(0x30010,0x40000),(0x40000,0x62000),(0x40030,0x50000),
            (0x41000,0x62000),(0x41030,0x51000),(0x50000,0x63000),(0x51000,0x63000)] {put_staff_memory(&mut memory,address,&value.to_le_bytes());}
        for (address,value) in [(0x20108,0u32),(0x2010C,7),(0x4000C,101),(0x4100C,102),(0x5000C,11),(0x5100C,22)] {put_staff_memory(&mut memory,address,&value.to_le_bytes());}
        put_staff_memory(&mut memory,0x30026,&[2]);put_staff_memory(&mut memory,0x41028,&[18]);
        (index,memory)
    }
    fn staff_fixture_info(vtable:u64)->Result<(u32,String)> {
        Ok(match vtable {0x60000=>(0x100,".?AVACTUAL_NON_PLAYER@db@@".into()),0x61000=>(0,".?AVFULL_CONTRACT@db@@".into()),
            0x62000=>(0,".?AVTEAM@db@@".into()),0x63000=>(0,".?AVCLUB@db@@".into()),
            0x64000=>(0,".?AVBASIC_CONTRACT@db@@".into()),_=>bail!("synthetic unknown RTTI")})
    }
    fn refresh_staff_fixture(index:&mut Index,memory:&HashMap<u64,u8>)->Result<()> {
        let rows=capture_staff_affiliations_with(index,0x10008,&|a,n|read_name_memory(memory,a,n),&staff_fixture_info,
            &|_|panic!("indexed club names must be reused"),&||Ok(0x10008))?;
        apply_staff_affiliations(index,rows);Ok(())
    }
    fn fill_staff_fixture_pages(memory:&mut HashMap<u64,u8>) {
        let pages=memory.keys().map(|a|*a&!4095).collect::<std::collections::HashSet<_>>();
        for page in pages {for offset in 0..4096 {memory.entry(page+offset).or_insert(0);}}
    }
    #[test]
    fn staff_affiliation_page_reads_coalesce_large_metadata_pass_and_still_observe_fresh_guards() {
        use std::cell::Cell;
        let (mut index,mut memory)=staff_affiliation_fixture(&profiles::FM27);let template=index.entities[0].clone();
        index.entities.retain(|e|e.kind!=EntityKind::Staff);
        for n in 0..64u32 {
            let mut e=template.clone();e.id=100+n;e.index=Some(n);e.base=0x80000+n as u64*0x200;e.address=e.base+0x100;
            let contract=0xA0000+n as u64*0x30;
            for (a,value) in [(0x10000+n as u64*8,e.address),(e.address,0x60000),(e.address+0xA8,contract),
                (contract,0x61000),(contract+8,e.address),(contract+0x10,0x40000)] {put_staff_memory(&mut memory,a,&value.to_le_bytes());}
            put_staff_memory(&mut memory,e.address+8,&n.to_le_bytes());put_staff_memory(&mut memory,e.address+12,&e.id.to_le_bytes());put_staff_memory(&mut memory,contract+0x26,&[2]);
            index.entities.push(e);
        }
        fill_staff_fixture_pages(&mut memory);let calls=Cell::new(0);let bytes=Cell::new(0);
        let rows=capture_staff_affiliations_with(&index,0x10200,&|a,n| {calls.set(calls.get()+1);bytes.set(bytes.get()+n);read_name_memory(&memory,a,n)},
            &staff_fixture_info,&|_|String::new(),&||Ok(0x10200)).unwrap();
        assert_eq!(rows.len(),64);assert!(rows.iter().all(|(_,a)|matches!(a,StaffAffiliation::Employed {club_id:11,..})));
        // The narrow reader would need >900 RPM calls for these same people.
        // Each page is fetched once in each independent observation phase.
        assert!(calls.get()<=30,"{} underlying reads",calls.get());assert_eq!(bytes.get(),calls.get()*4096);
        // A mutation visible only when a page is fetched a second time must be
        // caught; cached bytes from capture must not satisfy final validation.
        let job_page_reads=Cell::new(0);
        let result=capture_staff_affiliations_with(&index,0x10200,&|a,n| {
            let mut raw=read_name_memory(&memory,a,n)?;
            if a==0xA0000 {job_page_reads.set(job_page_reads.get()+1);if job_page_reads.get()>1 {raw[0x26]=14;}}
            Ok(raw)
        },&staff_fixture_info,&|_|String::new(),&||Ok(0x10200));
        assert!(result.is_err());assert_eq!(job_page_reads.get(),2);
    }
    #[test]
    fn staff_page_reads_cross_boundaries_fall_back_exactly_and_bound_retained_memory() {
        use std::cell::Cell;
        let calls=Cell::new(0);let read=|a,n| {calls.set(calls.get()+1);Ok(vec![if a==0x20000{1}else{2};n])};
        let pages=StaffReadPages::new(&read);
        assert_eq!(pages.read(0x20FFC,8).unwrap(),[1,1,1,1,2,2,2,2]);
        pages.read(0x20010,8).unwrap();assert_eq!(calls.get(),2);
        pages.clear();pages.read(0x20010,8).unwrap();assert_eq!(calls.get(),3);
        // Failed page fetches are remembered within a phase, but readable exact
        // ranges still succeed even if surrounding bytes cannot be read.
        let failures=Cell::new(0);let exact=Cell::new(0);
        let sparse=|_,n| {if n==4096 {failures.set(failures.get()+1);bail!("synthetic unreadable neighbouring bytes");}exact.set(exact.get()+1);Ok(vec![7;n])};
        let sparse_pages=StaffReadPages::new(&sparse);
        for _ in 0..3 {assert_eq!(sparse_pages.read(0x20010,8).unwrap(),vec![7;8]);}
        assert_eq!((failures.get(),exact.get()),(1,3));sparse_pages.clear();sparse_pages.read(0x20010,8).unwrap();assert_eq!(failures.get(),2);
        // Populate failure entries without allocating 32 MiB just for this
        // bound test; successful and failed entries use the same admission cap.
        pages.clear();for n in 0..StaffReadPages::MAX_PAGES {pages.pages.borrow_mut().insert(0x30000+n as u64*4096,None);}
        pages.read(0x10000,8).unwrap();assert_eq!(pages.pages.borrow().len(),1);
    }
    #[test]
    fn staff_affiliation_move_and_club_search_order_use_current_primary_contract() {
        for profile in [&profiles::FM26,&profiles::FM27] {for order in [[11,22],[22,11]] {
            let (mut index,mut memory)=staff_affiliation_fixture(profile);
            let unaffected=serde_json::to_value(&index.entities[1]).unwrap();
            put_staff_memory(&mut memory,0x30010,&0x41000u64.to_le_bytes());put_staff_memory(&mut memory,0x30026,&[14]);
            for club in order {
                refresh_staff_fixture(&mut index,&memory).unwrap();
                let rows=index.entities.iter().filter(|e|e.kind==EntityKind::Staff&&e.club_id==Some(club)).collect::<Vec<_>>();
                assert_eq!(rows.len(),usize::from(club==22));
                if let Some(e)=rows.first() {assert_eq!((e.club.as_str(),e.role.as_str(),e.group.as_str()),("B","스카우트","U-20"));assert_eq!(e.football_category,FootballCategory::Women);assert_eq!((e.ca,e.pa),(Some(100),Some(150)));}
            }
            assert_eq!(serde_json::to_value(&index.entities[1]).unwrap(),unaffected);
        }}
    }
    #[test]
    fn staff_affiliation_job_change_and_known_unemployment_clear_old_metadata() {
        let (mut index,mut memory)=staff_affiliation_fixture(&profiles::FM27);
        put_staff_memory(&mut memory,0x30026,&[20]);refresh_staff_fixture(&mut index,&memory).unwrap();
        assert_eq!(index.entities[0].role,"수석 코치");
        put_staff_memory(&mut memory,0x30026,&[0]);refresh_staff_fixture(&mut index,&memory).unwrap();
        assert_eq!(index.entities[0].employment(),Some(true));assert!(index.entities[0].role.is_empty());
        put_staff_memory(&mut memory,0x201A8,&0u64.to_le_bytes());refresh_staff_fixture(&mut index,&memory).unwrap();
        let e=&index.entities[0];assert_eq!(e.employment(),Some(false));assert!(e.club.is_empty());assert_eq!(e.group,"무소속");assert_eq!(e.role,"무직");assert_eq!(e.football_category,FootballCategory::Unknown);
    }
    #[test]
    fn staff_affiliation_bad_identity_contract_owner_and_links_are_unknown_not_unemployed() {
        for (address,bytes) in [(0x10000,0x20200u64.to_le_bytes().to_vec()),(0x2010C,9u32.to_le_bytes().to_vec()),
            (0x201A8,0x90000u64.to_le_bytes().to_vec()),(0x30000,0xDEAD0u64.to_le_bytes().to_vec()),
            (0x30008,0x20200u64.to_le_bytes().to_vec()),(0x30010,0u64.to_le_bytes().to_vec()),(0x40030,0u64.to_le_bytes().to_vec())] {
            let (mut index,mut memory)=staff_affiliation_fixture(&profiles::FM27);put_staff_memory(&mut memory,address,&bytes);
            refresh_staff_fixture(&mut index,&memory).unwrap();let e=&index.entities[0];
            assert_eq!(e.employment(),None);assert_eq!(e.club_id,None);assert!(e.club.is_empty());assert_eq!(e.role,"직무 미확인");
            assert!(index.warnings.iter().any(|w|w.starts_with("스태프 소속:")));
        }
    }
    #[test]
    fn staff_affiliation_torn_job_or_registry_end_does_not_publish_partial_updates() {
        use std::cell::Cell;
        for shrink in [false,true] {
            let (mut index,memory)=staff_affiliation_fixture(&profiles::FM27);let before=serde_json::to_value(&index).unwrap();let reads=Cell::new(0);
            let rows=capture_staff_affiliations_with(&index,0x10008,&|a,n| {
                if a==0x30026 {reads.set(reads.get()+1);if !shrink&&reads.get()>1 {return Ok(vec![14]);}}
                read_name_memory(&memory,a,n)
            },&staff_fixture_info,&|_|String::new(),&||Ok(if shrink{0x10000}else{0x10008}));
            assert!(rows.is_err());assert_eq!(serde_json::to_value(&index).unwrap(),before);
            // Confirm an unsupported profile cannot invoke any metadata read.
            index.session.module_stamp+=1;
            assert!(capture_staff_affiliations_with(&index,0x10008,&|_,_|panic!("profile gate must run first"),&staff_fixture_info,&|_|String::new(),&||Ok(0x10008)).is_err());
        }
    }
    #[test]
    fn staff_affiliation_guards_are_deterministic_and_exclude_editable_staff_attributes() {
        let (index,memory)=staff_affiliation_fixture(&profiles::FM27);let read=|a,n|read_name_memory(&memory,a,n);let name=|_|String::new();
        let mut baseline=None;
        for _ in 0..2 {
            let mut capture=StaffAffiliationCapture {session:&index.session,registry_end:0x10008,read:&read,info:&staff_fixture_info,name:&name,
                club_metadata:HashMap::new(),types:HashMap::new(),teams:HashMap::new(),clubs:HashMap::new(),guards:vec![],guarded_values:HashMap::new()};
            capture.staff(&index.entities[0]).unwrap();capture.verify().unwrap();
            for g in &capture.guards {assert!(!(g.address<0x200DE && g.address+g.expected.len() as u64>0x200DA));assert!(!(g.address<0x20054&&g.address+g.expected.len() as u64>0x20010));}
            if let Some(old)=&baseline {assert_eq!(&capture.guards,old);} else {baseline=Some(capture.guards);}
        }
    }
    #[test]
    fn person_registry_accepts_fm27_person_rtti_families_including_journalists() {
        // Concrete PERSON-derived types from the verified FM27 game_plugin.dll
        // RTTI. A journalist at any connection/watch anchor must not make an
        // otherwise valid save unconnectable.
        for name in [
            "HUMAN_NON_PLAYER", "CLIENT_PERSON", "PERSON", "BASIC_PERSON",
            "ACTUAL_NON_PLAYER", "ACTUAL_PERSON", "ACTUAL_PERSON_BASE",
            "RETIRED_PERSON_NAME_STRINGS", "RETIRED_PERSON_NAME_POINTERS",
            "RETIRED_PERSON_REMOVED_FROM_GAME", "VIRTUAL_PLAYER", "PLAYER",
            "ACTUAL_PLAYER", "AGENT", "JOURNALIST", "OFFICIAL",
            "ACTUAL_PLAYER_AND_NON_PLAYER", "SUPPORT_STAFF",
        ] {
            let native_name=format!(".?AV{name}@db@@");
            assert!(is_person_registry_type(&native_name), "rejected {native_name}");
        }
    }
    #[test]
    fn journalist_registry_fix_does_not_accept_unrelated_native_records() {
        for native_name in [
            "", ".?AVJOURNALIST@unverified@@", ".?AVJOURNALIST_EXTRA@db@@",
            ".?AVREMOVED_RECORD@db@@", ".?AVFUTURE_REGEN@db@@",
            ".?AVVIRTUAL_MANAGER@db@@", ".?AVCLUB@db@@",
        ] {
            assert!(!is_person_registry_type(native_name), "accepted {native_name}");
        }
    }
    #[test]
    fn unknown_store_build_reads_only_pe_headers_and_never_gets_a_layout() {
        let module=Module {name:"game_plugin.dll".into(),base:0x180000000,size:0x12345000,
            path:r"D:\XboxGames\Football Manager 27\Content\game_plugin.dll".into()};
        let mut reads=Vec::new();
        let build=detect_build_with(&module,|address,size| {
            reads.push((address,size));
            let mut bytes=vec![0;size];
            match (address-module.base,size) {
                (0,64)=>{
                    bytes[0..2].copy_from_slice(&0x5A4Du16.to_le_bytes());
                    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
                },
                (0x80,84)=>{
                    bytes[0..4].copy_from_slice(&0x4550u32.to_le_bytes());
                    bytes[4..6].copy_from_slice(&0x8664u16.to_le_bytes());
                    bytes[8..12].copy_from_slice(&0x12345678u32.to_le_bytes());
                    bytes[24..26].copy_from_slice(&0x20Bu16.to_le_bytes());
                    bytes[80..84].copy_from_slice(&module.size.to_le_bytes());
                },
                _=>panic!("unknown build must not read code or game objects"),
            }
            Ok(bytes)
        }).unwrap();
        assert!(build.profile.is_none());
        assert_eq!(reads,vec![(module.base,64),(module.base+0x80,84)]);
        let error=require_read_profile(&build).unwrap_err()
            .context(discovery::module_context(&module.path));
        assert!(error.downcast_ref::<profiles::UnsupportedBuild>().is_some());
        let reason=format!("{error:#}");
        assert!(reason.contains("12345678/12345000") && reason.contains("PC Game Pass"));
        assert!(!reason.contains(&module.path));
    }

    fn registry_fixture(profile:&profiles::BuildProfile, mode:u8, direct:(u64,u64), indirect:(u64,u64))->HashMap<u64,Vec<u8>> {
        let layout=profile.require_read_layout().unwrap();
        let header=|(b,e):(u64,u64)|[b.to_le_bytes(),e.to_le_bytes()].concat();
        HashMap::from([
            (0x180000000+layout.registry_mode_rva,vec![mode]),
            (0x20000+layout.person_vector_offset,header(direct)),
            (0x20000+layout.person_indirect_vector_offset,0x30000u64.to_le_bytes().to_vec()),
            (0x30000,header(indirect)),
        ])
    }
    fn fixture_registry(profile:&profiles::BuildProfile,memory:&HashMap<u64,Vec<u8>>)->Result<(u64,u64)> {
        registry_with_reader(0x180000000,0x20000,profile.require_read_layout().unwrap(),|address,size| {
            let bytes=memory.get(&address).with_context(||format!("unexpected read {address:x}"))?;
            ensure!(bytes.len()==size,"fixture read size mismatch");Ok(bytes.clone())
        })
    }
    #[test]
    fn registry_routes_mode_zero_and_mode_one_without_changing_nonempty_inline() {
        let direct=(0x40000,0x40080);let indirect=(0x50000,0x50100);
        for profile in [&profiles::FM26,&profiles::FM27] {
            assert_eq!(fixture_registry(profile,&registry_fixture(profile,0,direct,indirect)).unwrap(),direct);
            assert_eq!(fixture_registry(profile,&registry_fixture(profile,1,direct,indirect)).unwrap(),indirect);
        }
    }
    #[test]
    fn fm27_empty_inline_uses_native_indirect_route_but_fm26_remains_unchanged() {
        let indirect=(0x50000,0x50100);
        for direct in [(0,0),(0x40000,0x40000)] {
            assert_eq!(fixture_registry(&profiles::FM27,&registry_fixture(&profiles::FM27,0,direct,indirect)).unwrap(),indirect);
            let error=fixture_registry(&profiles::FM26,&registry_fixture(&profiles::FM26,0,direct,indirect)).unwrap_err();
            assert!(error.downcast_ref::<EmptyRegistry>().is_some());
        }
    }
    #[test]
    fn registry_fallback_never_accepts_malformed_inline_or_indirect() {
        for direct in [(1,1),(0x50000,0x40000),(0x40000,0x40003),(0,0x40000),(0x40000,0x2000000)] {
            let error=fixture_registry(&profiles::FM27,&registry_fixture(&profiles::FM27,0,direct,(0x50000,0x50100))).unwrap_err();
            assert!(error.downcast_ref::<EmptyRegistry>().is_none());
        }
        let mut memory=registry_fixture(&profiles::FM27,0,(0,0),(0x50000,0x50100));
        memory.insert(0x20080,vec![0;8]);
        assert!(fixture_registry(&profiles::FM27,&memory).unwrap_err().to_string().contains("간접 주소 검증 실패"));
        for invalid in [(0x50000,0x40000),(1,1),(0x50000,0x50007)] {
            assert!(fixture_registry(&profiles::FM27,&registry_fixture(&profiles::FM27,0,(0,0),invalid)).is_err());
        }
        assert!(fixture_registry(&profiles::FM27,&registry_fixture(&profiles::FM27,2,(0,0),(0x50000,0x50100))).is_err());
        let error=fixture_registry(&profiles::FM27,&registry_fixture(&profiles::FM27,0,(0,0),(0,0))).unwrap_err();
        assert!(error.downcast_ref::<EmptyRegistry>().is_some());
    }
    #[test]
    fn registry_rejects_a_route_changed_during_read() {
        let layout=profiles::FM27.require_read_layout().unwrap();
        for changed in [0x180000000+layout.registry_mode_rva,0x20060,0x20080,0x30000] {
            let memory=registry_fixture(&profiles::FM27,0,(0,0),(0x50000,0x50100));
            let mut counts=HashMap::<u64,usize>::new();
            let result=registry_with_reader(0x180000000,0x20000,layout,|address,size| {
                let count=counts.entry(address).or_default();*count+=1;
                let mut bytes=memory.get(&address).unwrap().clone();assert_eq!(bytes.len(),size);
                if address==changed && *count>1 {bytes[0]^=1;}
                Ok(bytes)
            });
            assert!(result.unwrap_err().to_string().contains("경로가 변경"));
        }
    }
    #[test]
    fn nationality_index_cache_reads_each_country_once_including_unreadable_links() {
        let mut cache = HashMap::new();
        let mut reads = 0;
        for _ in 0..200 {
            let country = cached_nationality_with(0x20000, &mut cache, |_| {
                reads += 1;
                Ok((101, "대한민국".into()))
            });
            assert_eq!(country, Some((101, "대한민국".into())));
        }
        assert_eq!(reads, 1);
        assert_eq!(cached_nationality_with(0x30000, &mut cache, |_| Ok((102, "이탈리아".into()))), Some((102, "이탈리아".into())));
        assert_eq!(cached_nationality_with(0x40000, &mut cache, |_| anyhow::bail!("unreadable nation")), None);
        assert_eq!(cached_nationality_with(0x40000, &mut cache, |_| panic!("failed links must also be cached")), None);
        assert_eq!(cache.len(), 3);
    }
    #[test]
    fn nationality_index_cache_keeps_missing_and_malformed_countries_unknown() {
        let mut cache = HashMap::new();
        for address in [0, 1, u64::MAX] {
            assert_eq!(cached_nationality_with(address, &mut cache, |_| panic!("invalid pointer must not be read")), None);
        }
        assert!(cache.is_empty());
        for (i, value) in [(0, "대한민국"), (101, ""), (101, "  "), (101, "대한민국\n")].into_iter().enumerate() {
            assert_eq!(cached_nationality_with(0x20000 + i as u64 * 0x1000, &mut cache, |_| Ok((value.0, value.1.into()))), None);
        }
    }
    fn name_memory(first: &str, last: &str, common: Option<&str>) -> HashMap<u64, u8> {
        let mut memory = HashMap::new();
        for (i, name) in [Some(first), Some(last), common].into_iter().enumerate() {
            let record = 0x20000 + i as u64 * 0x1000;
            let text = record + 0x100;
            for (offset, byte) in name.map_or(0, |_| record).to_le_bytes().into_iter().enumerate() {
                memory.insert(0x10050 + i as u64 * 8 + offset as u64, byte);
            }
            if let Some(name) = name {
                for (offset, byte) in text.to_le_bytes().into_iter().enumerate() {
                    memory.insert(record + offset as u64, byte);
                }
                for offset in 0..192 {
                    memory.insert(text + 4 + offset as u64, name.as_bytes().get(offset).copied().unwrap_or(0));
                }
            }
        }
        memory
    }
    fn read_name_memory(memory: &HashMap<u64, u8>, address: u64, size: usize) -> Result<Vec<u8>> {
        (0..size).map(|offset| memory.get(&(address + offset as u64)).copied().context("synthetic unreadable name")).collect()
    }
    #[test]
    fn refreshed_names_keep_common_name_and_rebuild_english_name_for_each_field() {
        for (first, last, common, expected) in [
            ("New", "Last", Some("별명"), ("별명", "New Last")),
            ("First", "New", Some("별명"), ("별명", "First New")),
            ("First", "Last", Some("새 별명"), ("새 별명", "First Last")),
            ("First", "Last", None, ("First Last", "First Last")),
            ("First", "Last", Some(""), ("First Last", "First Last")),
        ] {
            let memory = name_memory(first, last, common);
            let actual = read_person_names_with(0x10000, |address, size| read_name_memory(&memory, address, size)).unwrap();
            assert_eq!(actual, (expected.0.into(), expected.1.into()));
        }
    }
    #[test]
    fn name_refresh_rejects_unreadable_malformed_and_unterminated_names_without_fallback() {
        let mut memory = name_memory("First", "Last", Some("Common"));
        memory.remove(&0x22104);
        assert!(read_person_names_with(0x10000, |a, n| read_name_memory(&memory, a, n)).is_err());
        for bad in [vec![0xff, 0], vec![b'\n', 0], vec![b'x'; 192]] {
            let mut memory = name_memory("First", "Last", Some("Common"));
            for (offset, byte) in bad.into_iter().enumerate() { memory.insert(0x22104 + offset as u64, byte); }
            assert!(read_person_names_with(0x10000, |a, n| read_name_memory(&memory, a, n)).is_err());
        }
        assert!(compose_person_names("", "", "").is_err());
    }
    #[test]
    fn name_refresh_rejects_a_reference_or_string_changed_during_read() {
        for changed_address in [0x10050, 0x20104] {
            let memory = name_memory("First", "Last", Some("Common"));
            let mut visits = 0;
            let error = read_person_names_with(0x10000, |address, size| {
                let mut bytes = read_name_memory(&memory, address, size)?;
                if address == changed_address {
                    visits += 1;
                    if visits == 2 { bytes[0] ^= 1; }
                }
                Ok(bytes)
            }).unwrap_err();
            assert!(error.to_string().contains("조회 중 이름이 변경"));
        }
    }
    #[test]
    fn index_name_refresh_is_atomic_and_checks_identity_before_reading() {
        let (mut index, _, _) = crate::relations::fixtures::data();
        let original = index.entities[0].clone();
        let before = serde_json::to_vec(&index).unwrap();
        assert!(refresh_person_names_with(&mut index, &original, |_, _| bail!("read failed")).is_err());
        assert_eq!(serde_json::to_vec(&index).unwrap(), before);
        let mut wrong = original.clone(); wrong.address += 8;
        assert!(refresh_person_names_with(&mut index, &wrong, |_, _| panic!("must not read changed identity")).is_err());
        assert_eq!(serde_json::to_vec(&index).unwrap(), before);
        let updated = refresh_person_names_with(&mut index, &original, |_, _| Ok(("새 이름".into(), "New Name".into()))).unwrap();
        assert_eq!(updated.name, "새 이름");
        assert_eq!(updated.english_name, "New Name");
        let mut expected = original.clone(); expected.name = updated.name; expected.english_name = updated.english_name;
        assert_eq!(serde_json::to_vec(&index.entities[0]).unwrap(), serde_json::to_vec(&expected).unwrap());
        index.entities.push(original.clone());
        assert!(refresh_person_names_with(&mut index, &original, |_, _| panic!("must not read duplicate identity")).is_err());
    }
    #[test]
    fn dates() {
        assert_eq!(decode_date((2027 << 16) | 306), Some((2027, 11, 2)));
        assert_eq!(decode_date((2028 << 16) | 60), Some((2028, 2, 29)));
        assert_eq!(decode_date((2027 << 16) | 366), None);
    }
    #[test]
    fn explicit_youth_membership_overrides_contract_team() {
        let mut rows = vec![Entity {
            nationality: String::new(),
            nationality_id: None,
            id: 1,
            kind: EntityKind::Player,
            name: String::new(),
            english_name: String::new(),
            club: "Lecce".into(),
            club_id: Some(1141),
            age: Some(18),
            ca: None,
            pa: None,
            role: String::new(),
            group: "1군".into(),
            football_category: FootballCategory::Unknown,
            address: 1,
            base: 1,
            index: Some(1),
        }];
        assign_memberships(&mut rows, &HashMap::from([(0, 18)]), 1141, "Lecce");
        assert_eq!(rows[0].group, "U-20");
        assert_eq!(preferred_squad(18, 0), 0);
        assert_eq!(preferred_squad(0, 18), 0);
        assert_eq!(preferred_squad(12, 18), 18);
    }
    #[test]
    fn staff_assignment_uses_team_not_age_or_job() {
        // The same coach job can exist in each squad; youth-development staff
        // can also belong to the senior team. Only the assigned team decides.
        assert_eq!(contract_group(true, 0), "1군");
        assert_eq!(contract_group(true, 18), "U-20");
        assert_eq!(contract_group(true, 12), "U-18");
        assert_eq!(contract_group(true, 255), "소속팀 미확인");
        assert_eq!(contract_group(false, 0), "무소속");
    }
    #[test]
    fn other_club_youth_and_incoming_loan_use_actual_roster() {
        let player = Entity {
            nationality: String::new(),
            nationality_id: None,
            id: 1,
            kind: EntityKind::Player,
            name: String::new(),
            english_name: String::new(),
            club: "Other club".into(),
            club_id: Some(200),
            // An adult may appear in an age-group roster. Never replace the
            // explicit membership with an age-derived first-team assignment.
            age: Some(30),
            ca: None,
            pa: None,
            role: String::new(),
            group: "1군".into(),
            football_category: FootballCategory::Unknown,
            address: 1,
            base: 1,
            index: Some(1),
        };
        let mut rows = vec![player.clone(), player];
        rows[1].club_id = Some(300);
        assign_memberships(&mut rows, &HashMap::from([(0, 12), (1, 18)]), 200, "Other club");
        assert_eq!(rows[0].group, "U-18");
        assert_eq!(rows[0].club_id, Some(200));
        assert_eq!(rows[1].group, "U-20 · 임대 영입");
        assert_eq!(rows[1].club_id, Some(200));
        assert_eq!(rows[1].club, "Other club");

        // Reopening the borrower keeps the incoming-loan marker because the
        // contract employer is restored before applying the roster again.
        restore_player_employer(&mut rows[1], 300, "Parent club", 0);
        assign_memberships(&mut rows, &HashMap::from([(1, 18)]), 200, "Other club");
        assert_eq!(rows[1].group, "U-20 · 임대 영입");

        // Reopening the parent recovers the owned row, even after a borrower
        // view changed the display affiliation. The loan check can then label
        // it as outgoing without a roster membership in the parent's teams.
        restore_player_employer(&mut rows[1], 300, "Parent club", 0);
        assign_memberships(&mut rows, &HashMap::new(), 300, "Parent club");
        assert_eq!(rows[1].club_id, Some(300));
        assert_eq!(rows[1].club, "Parent club");
    }
    fn category_row(kind: EntityKind, club_id: Option<u32>, category: FootballCategory) -> Entity {
        Entity {
            nationality: String::new(),
            nationality_id: None,
            id: club_id.unwrap_or(1), kind, name: String::new(), english_name: String::new(),
            club: String::new(), club_id, age: None, ca: None, pa: None,
            role: String::new(), group: String::new(), football_category: category,
            address: 1, base: 1, index: Some(1),
        }
    }
    #[test]
    fn player_category_reads_only_the_verified_flag_and_never_defaults_a_failed_read_to_men() {
        assert_eq!(player_football_category(Some(0x02)), FootballCategory::Men);
        assert_eq!(player_football_category(Some(0x12)), FootballCategory::Women);
        assert_eq!(player_football_category(Some(0x0A)), FootballCategory::Men);
        assert_eq!(player_football_category(Some(0x92)), FootballCategory::Women);
        assert_eq!(player_football_category(None), FootballCategory::Unknown);
    }
    #[test]
    fn club_category_requires_consistent_evidence_from_every_validated_player() {
        use FootballCategory::*;
        let rows = vec![
            category_row(EntityKind::Player, Some(1), Men),
            category_row(EntityKind::Player, Some(1), Men),
            category_row(EntityKind::Player, Some(2), Women),
            category_row(EntityKind::Player, Some(2), Women),
            category_row(EntityKind::Player, Some(3), Men),
            category_row(EntityKind::Player, Some(3), Women),
            category_row(EntityKind::Player, Some(4), Men),
            category_row(EntityKind::Player, Some(4), Unknown),
            category_row(EntityKind::Staff, Some(5), Men),
            category_row(EntityKind::Club, Some(6), Unknown),
        ];
        let categories = club_football_categories(&rows);
        assert_eq!(categories.get(&1), Some(&Men));
        assert_eq!(categories.get(&2), Some(&Women));
        assert_eq!(categories.get(&3), Some(&Unknown));
        assert_eq!(categories.get(&4), Some(&Unknown));
        assert_eq!(categories.get(&5), None);
        assert_eq!(categories.get(&6), None);
    }
    #[test]
    fn staff_category_follows_employment_not_the_staff_members_sex() {
        use FootballCategory::*;
        let mut rows = vec![
            category_row(EntityKind::Player, Some(2), Women),
            // Even a prior male value must not override the women's employer.
            category_row(EntityKind::Staff, Some(2), Men),
            category_row(EntityKind::Person, Some(2), Men),
            category_row(EntityKind::Club, Some(2), Unknown),
            category_row(EntityKind::Staff, None, Women),
            category_row(EntityKind::Staff, Some(5), Men),
            category_row(EntityKind::City, None, Unknown),
        ];
        let categories = club_football_categories(&rows);
        assign_club_football_categories(&mut rows, &categories);
        assert!(rows[..4].iter().all(|e| e.football_category == Women));
        assert!(rows[4..].iter().all(|e| e.football_category == Unknown));
    }
}
