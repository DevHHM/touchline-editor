//! Club navigation from verified first-team competition links, never name guesses.
//! FM26 evidence: growth::fill_player (TEAM+50, COMP+60),
//! relations::career_competition_name (COMP+40/48), extras::first_team.
//! FM27 independently checked native TEAM tdvi / COMP nati, cnam getters:
//! reference/fm27-compatibility/league-navigation.json; exact accessor bytes
//! are also checked by backend::profiles before any live object reads.
//! Every indexed club survives missing/ambiguous links in an explicit group.
use crate::{backend, memory::Process, model::*};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, HashMap, HashSet};

const UNCLASSIFIED: &str = "기타 / 리그 미확인";
const UNKNOWN_NATION: &str = "국가 미확인";

pub struct LeagueGroup {
    pub key: String,
    pub id: Option<u32>,
    pub name: String,
    pub nation_id: Option<u32>,
    pub nation: String,
    pub football_category: FootballCategory,
    pub clubs: Vec<Entity>,
}
pub struct LeagueCatalog {
    pub groups: Vec<LeagueGroup>,
    pub total_clubs: usize,
    pub known_clubs: usize,
    pub warnings: Vec<String>,
}

fn clean(value: &str) -> Option<String> {
    (!value.trim().is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control))
        .then(|| value.trim().to_owned())
}
fn positive(value: Option<u32>) -> Option<u32> { value.filter(|id| *id > 0 && *id < 0x8000_0000) }

/// Snapshot-compatible semantic grouping. IDs, not translated names, define a league.
/// Duplicated/conflicting snapshot metadata fails to an unknown group, not a guess.
pub fn catalog(index: &Index) -> LeagueCatalog {
    let mut memberships: HashMap<u32, Option<&ClubLeague>> = HashMap::new();
    for link in &index.club_leagues {
        memberships.entry(link.club_id).and_modify(|old| {
            if old.is_some_and(|existing| existing != link) { *old = None; }
        }).or_insert(Some(link));
    }
    let mut groups = BTreeMap::<String, LeagueGroup>::new();
    let mut seen = HashSet::new();
    let mut known_clubs = 0;
    for club in index.entities.iter().filter(|row| row.kind == EntityKind::Club) {
        if !seen.insert(club.id) { continue; }
        let link = memberships.get(&club.id).and_then(|link| *link);
        let league = link.and_then(|link| Some((positive(link.league_id)?, clean(&link.league)?)));
        let nation = link.and_then(|link| Some((positive(link.nation_id)?, clean(&link.nation)?)));
        let (id, name) = league.map(|(id, name)| (Some(id), name)).unwrap_or((None, UNCLASSIFIED.into()));
        let (nation_id, nation) = nation.map(|(id, name)| (Some(id), name)).unwrap_or((None, UNKNOWN_NATION.into()));
        if id.is_some() { known_clubs += 1; }
        // Nation and category are part of the identity: same-name divisions,
        // women's leagues and unclassified clubs must never silently merge.
        let key = format!("{}:{}:{}", id.map(|id| format!("league-{id}")).unwrap_or("unclassified".into()),
            nation_id.map(|id| id.to_string()).unwrap_or("unknown".into()), club.football_category.key());
        let group = groups.entry(key.clone()).or_insert_with(|| LeagueGroup {
            key, id, name: name.clone(), nation_id, nation: nation.clone(), football_category: club.football_category, clubs: vec![],
        });
        // Stable labels if a snapshot has equivalent identities with varied translations.
        if name < group.name { group.name = name; }
        if nation < group.nation { group.nation = nation; }
        group.clubs.push(club.clone());
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    for group in &mut groups {
        group.clubs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    }
    groups.sort_by(|a, b| a.nation_id.is_none().cmp(&b.nation_id.is_none()).then(a.nation.cmp(&b.nation))
        .then(a.id.is_none().cmp(&b.id.is_none())).then(a.name.cmp(&b.name)).then(a.key.cmp(&b.key)));
    let total_clubs = seen.len();
    let mut warnings = vec!["현재 연결에서 확인한 구단의 1군 소속 대회 기준입니다. 순위표가 아니며, 소속이 바뀐 뒤에는 새로고침하세요.".into()];
    warnings.extend(index.warnings.iter().filter(|warning| warning.starts_with("리그 탐색:")).cloned());
    if total_clubs > known_clubs {
        warnings.push(format!("{}개 구단은 리그 연결을 확인하지 못했거나 소속 리그가 없어 ‘기타 / 리그 미확인’에 보존했습니다. 이전 스냅샷에는 리그 정보가 없을 수 있습니다.", total_clubs-known_clubs));
    }
    LeagueCatalog { groups, total_clubs, known_clubs, warnings }
}

fn pointer(value: u64) -> bool { (0x10000..0x0000_8000_0000_0000).contains(&value) && value % 8 == 0 }
fn q(raw: &[u8], offset: usize) -> u64 { u64::from_le_bytes(raw[offset..offset+8].try_into().unwrap()) }
fn d(raw: &[u8], offset: usize) -> u32 { u32::from_le_bytes(raw[offset..offset+4].try_into().unwrap()) }

struct Capture<'a> {
    read: &'a dyn Fn(u64, usize) -> Result<Vec<u8>>,
    type_name: &'a dyn Fn(u64) -> String,
    types: HashMap<u64, String>,
    competitions: HashMap<u64, Option<ClubLeague>>,
    nations: HashMap<u64, Option<(u32, String)>>,
    guards: Vec<(u64, Vec<u8>)>,
}
impl Capture<'_> {
    fn bytes(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        let raw = (self.read)(address, size)?;
        ensure!(raw.len() == size, "리그 정보 길이 검증 실패");
        Ok(raw)
    }
    fn guarded(&mut self, address: u64, size: usize) -> Result<Vec<u8>> {
        let raw = self.bytes(address, size)?;
        self.guards.push((address, raw.clone()));
        Ok(raw)
    }
    fn object(&mut self, address: u64, expected: &str) -> Result<u32> {
        ensure!(pointer(address), "리그 정보 포인터 검증 실패");
        let raw = self.guarded(address, 16)?;
        let vtable = q(&raw, 0);
        let name = self.types.entry(vtable).or_insert_with(|| (self.type_name)(vtable));
        ensure!(name == expected, "리그 정보 객체 형식 불일치");
        let id = d(&raw, 12);
        ensure!(positive(Some(id)).is_some(), "리그 정보 ID 검증 실패");
        Ok(id)
    }
    fn text(&mut self, address: u64, offsets: &[u64]) -> Result<String> {
        for offset in offsets {
            let ptr = q(&self.guarded(address + offset, 8)?, 0);
            if ptr == 0 { continue; }
            ensure!(pointer(ptr), "리그 이름 주소 검증 실패");
            let raw = self.bytes(ptr, 196)?;
            let end = raw[4..].iter().position(|&byte| byte == 0).context("리그 이름 길이 검증 실패")?;
            let name = std::str::from_utf8(&raw[4..4+end]).context("리그 이름 UTF-8 검증 실패")?;
            ensure!(!name.chars().any(char::is_control), "리그 이름 제어문자 검증 실패");
            self.guards.push((ptr, raw[..4+end+1].to_vec()));
            if let Some(name) = clean(name) { return Ok(name); }
        }
        anyhow::bail!("리그 이름을 확인할 수 없습니다")
    }
    fn nation(&mut self, address: u64) -> Option<(u32, String)> {
        if address == 0 { return None; }
        if let Some(value) = self.nations.get(&address) { return value.clone(); }
        let value = (|| -> Result<_> {
            let id = self.object(address, ".?AVNATION@db@@")?;
            Ok((id, self.text(address, &[0x20])?))
        })().ok();
        self.nations.insert(address, value.clone());
        value
    }
    fn competition(&mut self, address: u64) -> Option<ClubLeague> {
        if address == 0 { return None; }
        if let Some(value) = self.competitions.get(&address) { return value.clone(); }
        let value = (|| -> Result<_> {
            let id = self.object(address, ".?AVCOMP@db@@")?;
            let name = self.text(address, &[0x40, 0x48])?;
            let nation_ptr = q(&self.guarded(address + 0x60, 8)?, 0);
            let nation = self.nation(nation_ptr);
            Ok(ClubLeague { club_id: 0, league_id: Some(id), league: name,
                nation_id: nation.as_ref().map(|(id, _)| *id), nation: nation.map(|(_, name)| name).unwrap_or_default() })
        })().ok();
        self.competitions.insert(address, value.clone());
        value
    }
    fn club(&mut self, club: &Entity) -> Result<Option<ClubLeague>> {
        ensure!(self.object(club.address, ".?AVCLUB@db@@")? == club.id, "구단 ID가 변경됐습니다");
        let header = self.guarded(club.address + 0x18, 16)?;
        let (begin, end) = (q(&header, 0), q(&header, 8));
        if begin == end { return Ok(None); }
        ensure!(pointer(begin) && end > begin && end-begin <= 512 && (end-begin)%8 == 0, "구단 팀 목록 범위 오류");
        let slots = self.guarded(begin, (end-begin) as usize)?;
        let mut seen = HashSet::new();
        let mut first = None;
        for slot in slots.chunks_exact(8) {
            let team = q(slot, 0);
            ensure!(seen.insert(team), "구단 팀 연결 중복");
            self.object(team, ".?AVTEAM@db@@")?;
            let raw = self.guarded(team + 0x28, 0x30)?;
            ensure!(q(&raw, 8) == club.address, "팀의 소속 구단 불일치");
            if raw[0] != 0 { continue; }
            // Only the verified squad kind and club backlink define the first
            // team here. Do not reuse growth's extra +2F branch as a sex filter.
            ensure!(first.is_none(), "1군 팀 연결이 모호합니다");
            first = Some(q(&raw, 0x28));
        }
        let Some(comp) = first else { return Ok(None); };
        Ok(self.competition(comp).map(|mut link| { link.club_id = club.id; link }))
    }
    fn verify(&self) -> Result<()> {
        for (address, expected) in &self.guards {
            ensure!(self.bytes(*address, expected.len())? == *expected, "리그 정보를 읽는 동안 데이터가 변경됐습니다");
        }
        Ok(())
    }
}

/// No use of old offsets is permitted merely because the game's process name matches.
fn verified_layout(session: &Session) -> bool {
    backend::profiles::profile_for_session(session).is_some_and(|p|
        p.id == backend::profiles::FM26.id || p.id == backend::profiles::FM27.id)
}

pub fn populate(p: &Process, index: &mut Index) {
    index.club_leagues.clear();
    index.warnings.retain(|warning| !warning.starts_with("리그 탐색:"));
    if !verified_layout(&index.session) {
        index.warnings.push("리그 탐색: 이 빌드의 대회 연결이 아직 검증되지 않아 모든 구단을 미확인으로 유지합니다.".into());
        return;
    }
    let read = |a, n| p.read(a, n);
    let type_name = |vtable| backend::type_name(p, &index.session, vtable);
    let mut capture = Capture { read: &read, type_name: &type_name, types: HashMap::new(),
        competitions: HashMap::new(), nations: HashMap::new(), guards: vec![] };
    let mut links = vec![];
    let mut failures = 0;
    for club in index.entities.iter().filter(|row| row.kind == EntityKind::Club) {
        match capture.club(club) {
            Ok(Some(link)) => links.push(link),
            Ok(None) => {},
            Err(_) => failures += 1,
        }
    }
    if capture.verify().is_ok() {
        index.club_leagues = links;
        if failures > 0 { index.warnings.push(format!("리그 탐색: {failures}개 구단의 팀 연결을 확인하지 못해 미확인으로 유지했습니다.")); }
    } else {
        index.warnings.push("리그 탐색: 조회 중 데이터가 변경되어 분류를 취소했습니다. 새로고침하세요.".into());
    }
}

pub fn refresh(index: &mut Index) -> Result<()> {
    let p = Process::open(index.session.pid, false)?;
    backend::validate_with(&p, &index.session)?;
    populate(&p, index);
    backend::validate_with(&p, &index.session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    fn club(id: u32, category: FootballCategory) -> Entity {
        serde_json::from_value(json!({"id":id,"kind":"Club","name":format!("Club {id}"),"english_name":format!("Club {id}"),
            "club":format!("Club {id}"),"club_id":id,"age":null,"ca":null,"pa":null,"role":"구단","group":"",
            "football_category":category,"address":0x10000,"base":0x10000,"index":null})).unwrap()
    }
    fn index() -> Index {
        serde_json::from_value(json!({"session":{"pid":0,"created":0,"module_base":0,"module_size":0,"module_stamp":0,
            "db":0,"registry_begin":0,"game_date":"2026-10-30","profile":"synthetic","own_club_id":null},
            "entities":[],"warnings":[]})).unwrap()
    }
    fn link(club_id: u32, league_id: u32, nation_id: u32, league: &str) -> ClubLeague {
        ClubLeague { club_id, league_id:Some(league_id), league:league.into(), nation_id:Some(nation_id), nation:format!("Nation {nation_id}") }
    }
    #[test]
    fn leagues_legacy_snapshot_preserves_all_clubs_as_unclassified() {
        let mut snapshot = index();
        assert!(snapshot.club_leagues.is_empty());
        snapshot.entities = vec![club(1, FootballCategory::Men), club(2, FootballCategory::Unknown)];
        let result = catalog(&snapshot);
        assert_eq!((result.total_clubs, result.known_clubs), (2, 0));
        assert_eq!(result.groups.len(), 2);
        assert!(result.groups.iter().all(|group| group.id.is_none() && group.nation_id.is_none()
            && group.name == UNCLASSIFIED && group.nation == UNKNOWN_NATION));
    }
    #[test]
    fn leagues_ids_categories_and_unknowns_never_drop_or_merge_clubs() {
        let mut snapshot = index();
        snapshot.entities = (1..=7).map(|id| club(id, if id == 4 {FootballCategory::Women} else {FootballCategory::Men})).collect();
        snapshot.club_leagues = vec![link(1,100,10,"Premier"),link(2,100,10,"Premier"),
            link(3,101,11,"Premier"),link(4,100,10,"Premier"),
            ClubLeague {club_id:5,league_id:Some(102),league:"No nation".into(),..Default::default()},
            link(6,0,10,"Invalid ID")];
        let result = catalog(&snapshot);
        assert_eq!((result.total_clubs,result.known_clubs),(7,5));
        let mut ids = result.groups.iter().flat_map(|group|group.clubs.iter().map(|club|club.id)).collect::<Vec<_>>();
        ids.sort(); assert_eq!(ids, (1..=7).collect::<Vec<_>>());
        assert_eq!(result.groups.iter().find(|group|group.id==Some(100)&&group.football_category==FootballCategory::Men).unwrap().clubs.len(),2);
        assert!(result.groups.iter().any(|group|group.id==Some(102)&&group.nation_id.is_none()));
        assert!(result.groups.iter().any(|group|group.id.is_none()&&group.nation_id==Some(10)));
        let keys=result.groups.iter().map(|group|&group.key).collect::<HashSet<_>>();assert_eq!(keys.len(),result.groups.len());
    }
    #[test]
    fn leagues_conflicting_memberships_fail_unknown_and_unrelated_metadata_is_ignored() {
        let mut snapshot=index();snapshot.entities=vec![club(1,FootballCategory::Men),club(2,FootballCategory::Men)];
        snapshot.club_leagues=vec![link(1,100,10,"One"),link(1,101,10,"Two"),link(2,102,11,"Three"),link(99,999,99,"Absent club")];
        let result=catalog(&snapshot);assert_eq!((result.total_clubs,result.known_clubs),(2,1));
        assert!(result.groups.iter().find(|group|group.clubs[0].id==1).unwrap().id.is_none());
        assert!(!result.groups.iter().any(|group|group.id==Some(999)));
    }

    struct Memory { bytes: RefCell<BTreeMap<u64,u8>>, types: HashMap<u64,String> }
    impl Memory {
        fn put(&self,address:u64,bytes:&[u8]) {self.bytes.borrow_mut().extend(bytes.iter().enumerate().map(|(i,b)|(address+i as u64,*b)));}
        fn q(&self,address:u64,value:u64) {self.put(address,&value.to_le_bytes());}
        fn object(&self,address:u64,vtable:u64,id:u32) {self.put(address,&vec![0;0x100]);self.q(address,vtable);self.put(address+12,&id.to_le_bytes());}
        fn string(&self,object:u64,offset:u64,address:u64,text:&str) {self.q(object+offset,address);let mut raw=vec![0;196];raw[4..4+text.len()].copy_from_slice(text.as_bytes());self.put(address,&raw);}
        fn read(&self,address:u64,size:usize)->Result<Vec<u8>> {(0..size).map(|i|self.bytes.borrow().get(&(address+i as u64)).copied().context("synthetic unreadable byte")).collect()}
    }
    fn native_fixture()->Memory {
        let fixture=Memory{bytes:RefCell::new(BTreeMap::new()),types:HashMap::from([
            (0x1000,".?AVCLUB@db@@".into()),(0x2000,".?AVTEAM@db@@".into()),
            (0x3000,".?AVCOMP@db@@".into()),(0x4000,".?AVNATION@db@@".into())])};
        fixture.object(0x10000,0x1000,1);
        fixture.q(0x10018,0x20000);fixture.q(0x10020,0x20010);
        // Youth team deliberately comes first: no vector[0] shortcut is allowed.
        fixture.q(0x20000,0x30000);fixture.q(0x20008,0x40000);
        fixture.object(0x30000,0x2000,2);fixture.put(0x30028,&[18]);fixture.q(0x30030,0x10000);fixture.q(0x30050,0x50000);
        fixture.object(0x40000,0x2000,3);fixture.q(0x40030,0x10000);fixture.q(0x40050,0x60000);
        fixture.object(0x50000,0x3000,101);fixture.string(0x50000,0x40,0x70000,"Youth League");
        fixture.object(0x60000,0x3000,102);fixture.string(0x60000,0x40,0x80000,"Premier League");fixture.q(0x60060,0x90000);
        fixture.object(0x90000,0x4000,10);fixture.string(0x90000,0x20,0xa0000,"England");fixture
    }
    fn capture<'a>(read:&'a dyn Fn(u64,usize)->Result<Vec<u8>>,types:&'a dyn Fn(u64)->String)->Capture<'a> {
        Capture{read,type_name:types,types:HashMap::new(),competitions:HashMap::new(),nations:HashMap::new(),guards:vec![]}
    }
    #[test]
    fn leagues_native_route_selects_verified_first_team_not_youth() {
        let fixture=native_fixture();let read=|a,n|fixture.read(a,n);let types=|vt|fixture.types.get(&vt).cloned().unwrap_or_default();
        let mut c=capture(&read,&types);let result=c.club(&club(1,FootballCategory::Men)).unwrap().unwrap();
        assert_eq!(result,ClubLeague{club_id:1,league_id:Some(102),league:"Premier League".into(),nation_id:Some(10),nation:"England".into()});
        c.verify().unwrap();fixture.q(0x40050,0x50000);assert!(c.verify().is_err());
    }
    #[test]
    fn leagues_ambiguous_first_team_and_wrong_backlink_fail_closed() {
        let fixture=native_fixture();let read=|a,n|fixture.read(a,n);let types=|vt|fixture.types.get(&vt).cloned().unwrap_or_default();
        fixture.put(0x30028,&[0]);assert!(capture(&read,&types).club(&club(1,FootballCategory::Men)).is_err());
        fixture.put(0x30028,&[18]);fixture.q(0x40030,0x11000);assert!(capture(&read,&types).club(&club(1,FootballCategory::Men)).is_err());
    }
    #[test]
    fn leagues_missing_competition_or_wrong_type_remains_unclassified() {
        let fixture=native_fixture();let read=|a,n|fixture.read(a,n);let types=|vt|fixture.types.get(&vt).cloned().unwrap_or_default();
        fixture.q(0x40050,0);assert_eq!(capture(&read,&types).club(&club(1,FootballCategory::Men)).unwrap(),None);
        fixture.q(0x40050,0x90000);assert_eq!(capture(&read,&types).club(&club(1,FootballCategory::Men)).unwrap(),None);
    }
    #[test]
    fn leagues_missing_country_preserves_known_league_and_snapshot_roundtrip() {
        let fixture=native_fixture();let read=|a,n|fixture.read(a,n);let types=|vt|fixture.types.get(&vt).cloned().unwrap_or_default();
        fixture.q(0x60060,0);let result=capture(&read,&types).club(&club(1,FootballCategory::Men)).unwrap().unwrap();
        assert_eq!(result.league_id,Some(102));assert_eq!(result.nation_id,None);
        let mut snapshot=index();snapshot.entities=vec![club(1,FootballCategory::Men)];snapshot.club_leagues=vec![result];
        let restored:Index=serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
        assert_eq!(catalog(&restored).known_clubs,1);
    }
}
