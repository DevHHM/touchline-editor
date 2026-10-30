//! Typed, bounded person relations and existing database name references.
//! Native relation layouts are enabled only by verified adapter capabilities.
//! Inline vector growth uses the separately verified native allocator capability. Shared
//! strings are never mutated and targets are always selected explicitly.
use crate::{backend, memory::Process, model::*, transaction};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(test)]
pub(crate) mod fixtures;
mod growth;
mod social;
pub use social::{social_editable, social_reason_label, SOCIAL_KINDS};
pub use growth::RelationVectorGrowth;
mod career_growth;
pub use career_growth::{CareerGrowth, CareerStorage};
pub mod targets;

const MAX_VECTOR_BYTES: u64 = 16_384;
// Native relation insert wrapper RVA 0x2CF2610 requires usedBytes < 0xFF0.
const MAX_RELATION_COUNT: usize = 255;
const NATIONALITY_TAIL: [u8; 8] = [0, 0, 8, 9, 100, 80, 0, 255];
// Native relation editor RVA 0x2CF7640 initializes a new LikesClub record
// with no permanence flag. Explicit donor templates retain their original tail.
const FAVOURITE_TAIL: [u8; 8] = [1, 0, 1, 3, 100, 0, 0, 255];
pub const CAREER_CONDITIONS: &[(u8, &str)] = &[(1, "맡고 싶음"), (2, "맡고 싶지 않음")];
pub const CAREER_JOBS: &[(u8, &str)] = &[
    (0, "무직"),
    (1, "선수"),
    (2, "코치"),
    (3, "U18 코치"),
    (4, "회장"),
    (6, "이사"),
    (8, "상무 이사"),
    (10, "풋볼 디렉터"),
    (12, "물리치료사"),
    (14, "스카우트"),
    (16, "감독"),
    (17, "U18 감독"),
    (20, "수석 코치"),
    (21, "U18 수석 코치"),
    (22, "세트피스 코치"),
    (24, "단장"),
    (26, "체력 코치"),
    (27, "U18 체력 코치"),
    (34, "골키퍼 코치"),
    (35, "U18 골키퍼 코치"),
    (38, "수석 의사"),
    (40, "수석 스포츠 과학자"),
    (44, "수석 스카우트"),
    (45, "U18 수석 스카우트"),
    (46, "의사"),
    (48, "스포츠 과학자"),
    (50, "수석 물리치료사"),
    (52, "U19 감독"),
    (58, "영입 분석가"),
    (60, "전력 분석가"),
    (61, "U18 전력 분석가"),
    (62, "수석 전력 분석가"),
    (64, "유소년 육성 총괄"),
    (66, "구단주"),
    (70, "대표"),
    (86, "임대 매니저"),
    (88, "테크니컬 디렉터"),
    (89, "U18 테크니컬 디렉터"),
    (144, "감독 대행"),
];
pub fn career_condition_label(value: u8) -> &'static str {
    match value {
        1 => "맡고 싶음",
        2 => "맡고 싶지 않음",
        3 => "지식 있음",
        4 => "지식 없음",
        5 => "스카우트 선호 지역",
        6 => "스카우트 비선호 지역",
        _ => "확인되지 않은 조건",
    }
}
pub fn career_job_label(value: u8) -> &'static str {
    CAREER_JOBS
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, label)| *label)
        .unwrap_or("확인되지 않은 직무")
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NameField {
    First,
    Last,
    Common,
}
impl NameField {
    fn offset(self) -> u64 {
        match self {
            Self::First => 0x50,
            Self::Last => 0x58,
            Self::Common => 0x60,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::First => "이름",
            Self::Last => "성",
            Self::Common => "통용명",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NameRecord {
    pub field: NameField,
    pub text: String,
    pub record: u64,
    pub text_object: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkedRecord {
    pub uid: u32,
    pub name: String,
    pub address: u64,
    pub object_type: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelationRow {
    pub slot: usize,
    pub kind: u8,
    pub kind_label: String,
    pub object_type: u8,
    pub reason: u16,
    pub level: u8,
    pub target: Option<LinkedRecord>,
    pub editable_favourite: bool,
    pub editable_secondary_nationality: bool,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerPlanRow {
    pub slot: usize,
    pub condition: u8,
    pub scope: u8,
    pub job: u8,
    pub object_type: u8,
    pub target: Option<LinkedRecord>,
    pub editable_club: bool,
    pub record: u64,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CareerPlans {
    pub rows: Vec<CareerPlanRow>,
    pub holder: u64,
    pub begin: u64,
    pub end: u64,
    pub capacity: u64,
    /// Only populated from an exact, still-current deletion receipt. Never infer
    /// ownership from arbitrary bytes outside the native vector's used range.
    #[serde(default)]
    pub reusable: Option<CareerPlanRow>,
    #[serde(default)]
    pub allocation_available: bool,
    #[serde(default)]
    pub allocation_reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersonRelations {
    pub entity: Entity,
    pub names: Vec<NameRecord>,
    pub primary_nationality: Option<LinkedRecord>,
    pub rows: Vec<RelationRow>,
    pub spare_slots: usize,
    pub notes: Vec<String>,
    pub holder: u64,
    pub begin: u64,
    pub end: u64,
    pub capacity: u64,
    pub guards: Vec<Guard>,
    #[serde(default)]
    pub career: Option<CareerPlans>,
    #[serde(default)]
    pub append_bytes: Option<Vec<u8>>,
    #[serde(default)]
    pub vector_growth_available: bool,
    #[serde(default)]
    pub vector_growth_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RelationRequest {
    AddCareerTarget { target: targets::CareerTargetRef, condition:u8, scope:u8, job:u8 },
    SetCareerTarget { slot:usize, target: targets::CareerTargetRef, condition:u8, scope:u8, job:u8 },
    SetSocialRelation { slot: usize, kind: u8, target: Entity, reason: u16, level: u8 },
    AddSocialRelation { kind: u8, target: Entity, reason: u16, level: u8 },
    DeleteSocialRelation { slot: usize },
    SetCareerDetails {
        slot: usize,
        condition: u8,
        scope: u8,
        job: u8,
    },
    AddCareerClub {
        club: Entity,
        condition: u8,
        job: u8,
    },
    DeleteCareerPlan {
        slot: usize,
    },
    DeleteFavouriteClub {
        slot: usize,
    },
    DeleteSecondaryNationality {
        slot: usize,
    },
    SetCareerClub {
        slot: usize,
        club: Entity,
        condition: u8,
        job: u8,
    },
    SetFavouriteClub {
        slot: usize,
        club: Entity,
        reason: u16,
        level: u8,
    },
    AddFavouriteClub {
        #[serde(default)]
        template_slot: Option<usize>,
        club: Entity,
        reason: u16,
        level: u8,
    },
    SetSecondaryNationality {
        slot: usize,
        donor: Entity,
        expected_nation_uid: u32,
    },
    AddSecondaryNationality {
        donor: Entity,
        expected_nation_uid: u32,
    },
    ReuseName {
        field: NameField,
        donor: Entity,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelationPlan {
    pub session: Session,
    pub entity: Entity,
    pub request: RelationRequest,
    pub summary: String,
    pub before: String,
    pub after: String,
    pub changes: Vec<Change>,
    pub guards: Vec<Guard>,
    #[serde(default)]
    pub growth: Option<RelationVectorGrowth>,
    #[serde(default)]
    pub allocation: Option<crate::native_alloc::NativeAllocation>,
    #[serde(default)]
    pub retained_original: Option<crate::native_alloc::NativeAllocation>,
    #[serde(default)]
    pub career_growth: Option<CareerGrowth>,
    #[serde(default)]
    pub career_storage: Option<CareerStorage>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelationReceipt {
    pub id: String,
    pub time: String,
    pub status: String,
    pub plan: RelationPlan,
    pub undo_of: Option<String>,
}

/// A native-array publication was attempted. Even when byte rollback reports
/// success, consumers must discard the connection and reload ownership state.
#[derive(Debug)]
pub struct RelationWriteFailure {
    pub message: String,
    pub receipt_id: String,
}
impl std::fmt::Display for RelationWriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (기록 ID: {})", self.message, self.receipt_id)
    }
}
impl std::error::Error for RelationWriteFailure {}

pub fn requires_reconnect(error: &anyhow::Error) -> bool {
    error.downcast_ref::<RelationWriteFailure>().is_some()
}
fn commit_failure(receipt_id: &str, message: &str, growth: bool, write_started: bool) -> anyhow::Error {
    if growth && write_started {
        RelationWriteFailure { message: message.into(), receipt_id: receipt_id.into() }.into()
    } else {
        anyhow::anyhow!("{message}")
    }
}

fn q(raw: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(raw[offset..offset + 8].try_into().unwrap())
}
fn pointer(address: u64) -> bool {
    (0x10000..0x0000_8000_0000_0000).contains(&address)
}
fn guard(
    p: &impl RelationMemory,
    guards: &mut Vec<Guard>,
    address: u64,
    size: usize,
) -> Result<Vec<u8>> {
    let raw = p.read(address, size)?;
    for (index, chunk) in raw.chunks(256).enumerate() {
        let g = Guard {
            address: address + (index * 256) as u64,
            expected: chunk.to_vec(),
        };
        if !guards.contains(&g) {
            guards.push(g);
        }
    }
    Ok(raw)
}
fn vector_bounds(raw: &[u8]) -> Result<(u64, u64, u64)> {
    ensure!(raw.len() == 24, "관계 벡터 헤더 크기가 다릅니다");
    let (begin, end, capacity) = (q(raw, 0), q(raw, 8), q(raw, 16));
    if (begin, end, capacity) == (0, 0, 0) {
        return Ok((0, 0, 0));
    }
    ensure!(
        pointer(begin)
            && begin <= end
            && end <= capacity
            && capacity - begin <= MAX_VECTOR_BYTES
            && (end - begin) % 16 == 0
            && (capacity - begin) % 16 == 0,
        "관계 벡터 범위 검증 실패"
    );
    Ok((begin, end, capacity))
}
fn kind_label(kind: u8) -> &'static str {
    match kind {
        1 => "좋아하는 인물",
        2 => "싫어하는 인물",
        3 => "좋아하는 구단",
        4 => "싫어하는 구단",
        5 => "라이벌 구단",
        6 => "라이벌 국가",
        9 => "추가 국적",
        10 => "국가대표 은퇴",
        25 => "대회 등록",
        27 => "대회 미등록",
        57 => "국가대표 초대",
        67 => "UEFA 출전",
        70 => "국가 홈그로운",
        71 => "훈련 만족",
        72 => "구단 홈그로운",
        74 => "국가 육성",
        81 => "국가대표 차출 제외",
        104 => "다른 국가 출전",
        106 => "출전 경기",
        107 => "다른 국가 유소년 출전",
        109 => "정착 자격",
        117 => "취업허가 신청 불가",
        124 => "디비전 출전",
        _ => "기타 관계 (읽기 전용)",
    }
}
fn linked(
    p: &Process,
    s: &Session,
    address: u64,
    object_type: u8,
    guards: &mut Vec<Guard>,
) -> Result<LinkedRecord> {
    ensure!(pointer(address), "연결 대상 포인터가 유효하지 않습니다");
    let raw = guard(p, guards, address, 16)?;
    let typ = backend::type_name(p, s, q(&raw, 0));
    let name = match object_type {
        1 => {
            ensure!(typ == ".?AVCLUB@db@@", "구단 객체 형식이 다릅니다: {typ}");
            backend::club_name(p, address)
        }
        8 => {
            ensure!(typ == ".?AVNATION@db@@", "국가 객체 형식이 다릅니다: {typ}");
            backend::indirect_string(p, address + 0x20)
        }
        3 => {
            ensure!(
                typ.contains("PLAYER@db@@") || typ.contains("PERSON@db@@"),
                "인물 객체 형식이 다릅니다"
            );
            backend::person_name(p, address)
        }
        _ => bail!("읽기 전용 관계 유형입니다"),
    };
    ensure!(
        !name.is_empty() && !name.chars().any(char::is_control),
        "연결 대상 이름이 유효하지 않습니다"
    );
    let uid = u32::from_le_bytes(raw[12..16].try_into().unwrap());
    ensure!(uid > 0, "연결 대상 ID가 유효하지 않습니다");
    Ok(LinkedRecord {
        uid,
        name,
        address,
        object_type,
    })
}
// CareerPlanObjectType::Competition is 25 (FMRTE 26.3.2 enum metadata).
// The COMP name fields were corroborated against Palestra's live Premier League
// target. This resolver is deliberately separate from editable person relations.
fn career_competition_target(
    p: &Process,
    s: &Session,
    address: u64,
    guards: &mut Vec<Guard>,
) -> Result<LinkedRecord> {
    ensure!(pointer(address), "대회 대상 포인터가 유효하지 않습니다");
    let raw = guard(p, guards, address, 16)?;
    let typ = backend::type_name(p, s, q(&raw, 0));
    ensure!(typ == ".?AVCOMP@db@@", "대회 객체 형식이 다릅니다: {typ}");
    let uid = u32::from_le_bytes(raw[12..16].try_into().unwrap());
    ensure!(uid > 0, "대회 대상 ID가 유효하지 않습니다");
    let name = career_competition_name(p, address, guards)?;
    Ok(LinkedRecord { uid, name, address, object_type: 25 })
}
fn career_competition_name(
    p: &impl RelationMemory,
    address: u64,
    guards: &mut Vec<Guard>,
) -> Result<String> {
    for offset in [0x40, 0x48] {
        let text_object = q(&guard(p, guards, address + offset, 8)?, 0);
        if text_object == 0 {
            continue;
        }
        ensure!(pointer(text_object), "대회 이름 문자열 포인터가 유효하지 않습니다");
        let raw = p.read(text_object, 4 + 192)?;
        let end = raw[4..].iter().position(|&v| v == 0)
            .context("대회 이름이 허용 길이를 넘습니다")?;
        let name = std::str::from_utf8(&raw[4..4 + end])
            .context("대회 이름 UTF-8 검증 실패")?;
        ensure!(!name.chars().any(char::is_control), "대회 이름 제어문자 검증 실패");
        guards.push(Guard { address: text_object, expected: raw[..4 + end + 1].to_vec() });
        if !name.is_empty() {
            return Ok(name.to_owned());
        }
    }
    bail!("대회 이름을 확인할 수 없습니다")
}
fn read_name(
    p: &Process,
    person: u64,
    field: NameField,
    guards: &mut Vec<Guard>,
) -> Result<NameRecord> {
    let record = q(&guard(p, guards, person + field.offset(), 8)?, 0);
    if record == 0 {
        return Ok(NameRecord {
            field,
            text: String::new(),
            record: 0,
            text_object: 0,
        });
    }
    ensure!(pointer(record), "이름 레코드 포인터 검증 실패");
    let text_object = q(&guard(p, guards, record, 16)?, 0);
    ensure!(pointer(text_object), "이름 문자열 포인터 검증 실패");
    // Read only through the terminator, then retain the exact object/text bytes.
    let raw = p.read(text_object + 4, 192)?;
    let end = raw
        .iter()
        .position(|&v| v == 0)
        .context("이름이 허용 길이를 넘습니다")?;
    let text = std::str::from_utf8(&raw[..end])
        .context("이름 UTF-8 검증 실패")?
        .to_owned();
    ensure!(
        !text.chars().any(char::is_control),
        "이름 제어문자 검증 실패"
    );
    guard(p, guards, text_object, 4 + end + 1)?;
    Ok(NameRecord {
        field,
        text,
        record,
        text_object,
    })
}
fn snapshot(
    p: &Process,
    s: &Session,
    e: &Entity,
    mut guards: Vec<Guard>,
) -> Result<PersonRelations> {
    ensure!(
        matches!(
            e.kind,
            EntityKind::Player | EntityKind::Staff | EntityKind::Person
        ),
        "인물만 관계를 조회할 수 있습니다"
    );
    guard(p, &mut guards, s.module_base + 0x4DF3C18, 4)?;
    let mut names = vec![];
    for field in [NameField::First, NameField::Last, NameField::Common] {
        names.push(read_name(p, e.address, field, &mut guards)?);
    }
    let nation = q(&guard(p, &mut guards, e.address + 0x68, 8)?, 0);
    let primary_nationality = if nation == 0 {
        None
    } else {
        Some(linked(p, s, nation, 8, &mut guards)?)
    };
    let holder = q(&guard(p, &mut guards, e.address + 0x78, 8)?, 0);
    let mut out = PersonRelations {
        entity: e.clone(),
        names,
        primary_nationality,
        rows: vec![],
        spare_slots: 0,
        notes: vec![
            "기존 데이터베이스 이름 재사용만 지원합니다. 공유 문자열은 수정하지 않습니다".into(),
            "추가 국적은 출전 자격·취업허가·국가대표 등록을 자동 변경하지 않습니다".into(),
            "관계 추가는 기존 여유 공간 또는 현재 게임 버전에서 검증된 배열 확장 기능을 사용합니다"
                .into(),
        ],
        holder,
        begin: 0,
        end: 0,
        capacity: 0,
        guards,
        career: None,
        append_bytes: None,
        vector_growth_available: false,
        vector_growth_reason: None,
    };
    if holder == 0 {
        out.notes
            .push("관계 컬렉션이 없어 추가할 수 없습니다".into());
        return Ok(out);
    }
    ensure!(pointer(holder), "관계 컬렉션 포인터 검증 실패");
    let header = guard(p, &mut out.guards, holder, 24)?;
    (out.begin, out.end, out.capacity) = vector_bounds(&header)?;
    out.spare_slots = ((out.capacity - out.end) / 16) as usize;
    if out.begin == out.end {
        return Ok(out);
    }
    let raw = guard(
        p,
        &mut out.guards,
        out.begin,
        (out.end - out.begin) as usize,
    )?;
    for (slot, bytes) in raw.chunks_exact(16).enumerate() {
        let object_type = bytes[10];
        let kind = bytes[11];
        let mut target_guards = vec![];
        let target = linked(p, s, q(bytes, 0), object_type, &mut target_guards).ok();
        if target.is_some() {
            out.guards.extend(target_guards);
        }
        let editable_favourite = object_type == 1
            && kind == 3
            && target.is_some()
            && bytes[12] <= 100
            && u16::from_le_bytes(bytes[8..10].try_into().unwrap()) <= 4;
        let editable_secondary_nationality = object_type == 8 && kind == 9 && target.is_some();
        out.rows.push(RelationRow {
            slot,
            kind,
            kind_label: kind_label(kind).into(),
            object_type,
            reason: u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
            level: bytes[12],
            target,
            editable_favourite,
            editable_secondary_nationality,
            raw: bytes.to_vec(),
        });
    }
    Ok(out)
}

fn career_vector_bounds(raw: &[u8]) -> Result<(u64, u64, u64)> {
    ensure!(raw.len() == 24, "커리어 계획 벡터 헤더 크기가 다릅니다");
    let (begin, end, capacity) = (q(raw, 0), q(raw, 8), q(raw, 16));
    if (begin, end, capacity) == (0, 0, 0) {
        return Ok((0, 0, 0));
    }
    ensure!(
        pointer(begin)
            && begin <= end
            && end <= capacity
            && end - begin <= 64 * 8
            && capacity - begin <= MAX_VECTOR_BYTES
            && (end - begin) % 8 == 0
            && (capacity - begin) % 8 == 0,
        "커리어 계획 벡터 범위 검증 실패"
    );
    Ok((begin, end, capacity))
}
fn blank_career(raw: &[u8]) -> bool {
    raw.len() == 16 && raw[..8] == [0; 8] && raw[8..] == [1, 1, 0, 255, 2, 0, 0, 0]
}
pub fn career_target_type_label(object_type: u8) -> &'static str {
    match object_type {
        3 => "구단",
        5 => "대륙",
        9 => "국가",
        25 => "대회",
        255 => "대상 없음",
        _ => "확인되지 않은 대상",
    }
}
fn career_record(old: &[u8], target: u64, condition: u8, job: u8) -> Result<Vec<u8>> {
    ensure!(
        old.len() == 16
            && (blank_career(old)
                || (pointer(q(old, 0))
                    && (1..=6).contains(&old[8])
                    && [1, 2].contains(&old[9])
                    && [3, 5, 9, 25].contains(&old[11])
                    && CAREER_JOBS.iter().any(|(v, _)| *v == old[10]))),
        "검증된 커리어 계획 또는 할당된 빈 항목이 아닙니다"
    );
    ensure!(
        pointer(target)
            && [1, 2].contains(&condition)
            && CAREER_JOBS.iter().any(|(v, _)| *v == job),
        "커리어 계획 조건·직무가 검증 범위를 벗어납니다"
    );
    let mut new = old.to_vec();
    new[..8].copy_from_slice(&target.to_le_bytes());
    new[8..12].copy_from_slice(&[condition, 1, job, 3]);
    Ok(new)
}

fn career_detail_record(old: &[u8], condition: u8, scope: u8, job: u8) -> Result<Vec<u8>> {
    ensure!(old.len() == 16 && pointer(q(old, 0)) && [3, 5, 9, 25].contains(&old[11]), "지원하는 커리어 대상 형식이 아닙니다");
    ensure!((1..=6).contains(&condition) && [1, 2].contains(&scope) && CAREER_JOBS.iter().any(|(v,_)| *v == job), "커리어 계획 조건·범위·직무가 검증 범위를 벗어납니다");
    let mut new = old.to_vec();
    new[8..11].copy_from_slice(&[condition, scope, job]);
    Ok(new)
}

fn career_target_record(old:&[u8], target:&LinkedRecord, condition:u8,scope:u8,job:u8)->Result<Vec<u8>> {
    ensure!(old.len()==16 && (blank_career(old)||[3,5,9,25].contains(&old[11])),"커리어 원본 대상 유형이 다릅니다");
    ensure!(pointer(target.address)&&target.uid>0&&[3,5,9,25].contains(&target.object_type),"선택한 커리어 대상이 유효하지 않습니다");
    ensure!((1..=6).contains(&condition)&&[1,2].contains(&scope)&&CAREER_JOBS.iter().any(|(v,_)|*v==job),"커리어 조건·범위·직무가 유효하지 않습니다");
    let mut new=old.to_vec();new[..8].copy_from_slice(&target.address.to_le_bytes());new[8..12].copy_from_slice(&[condition,scope,job,target.object_type]);Ok(new)
}

fn prepare_career_target(snapshot:&PersonRelations, guards:&[Guard], target:&LinkedRecord, slot:Option<usize>, condition:u8,scope:u8,job:u8)->Result<(String,String,String,Vec<Change>,Option<CareerGrowth>)> {
    let career=snapshot.career.as_ref().context("커리어 계획 목록을 검증할 수 없습니다")?;
    let blank=career.rows.iter().find(|r|blank_career(&r.raw));
    let old=if let Some(slot)=slot {career_delete_check(snapshot,slot)?;career.rows.get(slot)} else {blank.or(career.reusable.as_ref())};
    let default=[vec![0;8],vec![1,1,0,255,2,0,0,0]].concat();
    let new=career_target_record(old.map(|r|r.raw.as_slice()).unwrap_or(&default),target,condition,scope,job)?;
    ensure!(!career.rows.iter().any(|r|Some(r.slot)!=slot&&r.raw[..12]==new[..12]),"같은 커리어 계획이 이미 있습니다");
    let after=format!("{} · {} · {}",target.name,career_condition_label(condition),career_job_label(job));
    if let Some(old)=old {
        let changes=career_edit_changes(career,guards,old,new,slot.is_none()&&blank.is_none(),if slot.is_some(){"career_target"}else{"career_target_add"},"커리어 계획",)?;
        Ok((if slot.is_some(){"커리어 계획 수정"}else{"커리어 계획 추가"}.into(),old.target.as_ref().map(|r|r.name.clone()).unwrap_or_else(||format!("{} 항목",career_target_type_label(old.object_type))),after,changes,None))
    } else {
        let intent=career_growth::intent(snapshot,new)?;
        Ok(("커리어 계획 추가".into(),format!("커리어 계획 {}개",career.rows.len()),after,vec![],Some(intent)))
    }
}

pub fn career_details_check(snapshot: &PersonRelations, slot: usize) -> Result<()> {
    career_delete_check(snapshot, slot)?;
    let row = &snapshot.career.as_ref().unwrap().rows[slot];
    career_detail_record(&row.raw, row.condition, row.scope, row.job)?;
    Ok(())
}
// FM26 26.3.2 game_plugin.dll: append RVA 0x1736B40 calls the type-rank
// ordering helper at RVA 0x173FE70. Its key is record[0xB], not job/scope.
fn career_type_rank(object_type: u8) -> u8 {
    match object_type {
        3 => 4,
        25 => 3,
        9 => 2,
        5 => 1,
        _ => 0,
    }
}

/// Edit the owned record, reorder the whole used pointer vector, then publish
/// an appended end. Equal ranks retain their previous relative order. Every
/// original pointer/record is guarded, including an owned detached tail slot.
fn career_edit_changes(
    career: &CareerPlans,
    guards: &[Guard],
    old: &CareerPlanRow,
    new: Vec<u8>,
    appending: bool,
    record_key: &str,
    record_label: &str,
) -> Result<Vec<Change>> {
    ensure!(
        old.raw.len() == 16 && new.len() == 16,
        "커리어 레코드 크기가 다릅니다"
    );
    let bounds = career_vector_bounds(&guarded_bytes(guards, career.holder, 24)?)?;
    ensure!(
        bounds == (career.begin, career.end, career.capacity)
            && (career.end - career.begin) / 8 == career.rows.len() as u64,
        "커리어 계획 원본 경계가 다릅니다"
    );
    let mut seen = std::collections::HashSet::new();
    let mut pointers = Vec::new();
    let mut ordered = Vec::new();
    for (slot, row) in career.rows.iter().enumerate() {
        ensure!(
            row.slot == slot
                && pointer(row.record)
                && seen.insert(row.record)
                && row.raw.len() == 16
                && guarded_bytes(guards, row.record, 16)? == row.raw,
            "커리어 계획 원본 레코드가 다릅니다"
        );
        pointers.extend(row.record.to_le_bytes());
        ordered.push((
            row.record,
            if row.record == old.record {
                new[11]
            } else {
                row.raw[11]
            },
        ));
    }
    ensure!(
        guarded_bytes(guards, career.begin, pointers.len())? == pointers,
        "커리어 계획 원본 순서가 다릅니다"
    );
    ensure!(
        old.raw.len() == 16 && new.len() == 16 && guarded_bytes(guards, old.record, 16)? == old.raw,
        "수정할 커리어 레코드 원본이 다릅니다"
    );
    if appending {
        ensure!(
            career.end < career.capacity
                && old.slot == career.rows.len()
                && !seen.contains(&old.record)
                && guarded_bytes(guards, career.end, 8)? == old.record.to_le_bytes(),
            "재사용할 커리어 항목 범위가 다릅니다"
        );
        pointers.extend(old.record.to_le_bytes());
        ordered.push((old.record, new[11]));
    } else {
        ensure!(
            career
                .rows
                .get(old.slot)
                .is_some_and(|row| row.record == old.record && row.raw == old.raw),
            "수정할 커리어 항목이 변경됐습니다"
        );
    }
    let mut changes = vec![];
    if old.raw != new {
        changes.push(change(
            record_key,
            record_label,
            old.record,
            old.raw.clone(),
            new,
        )?);
    }
    ordered.sort_by_key(|(_, typ)| std::cmp::Reverse(career_type_rank(*typ)));
    let sorted: Vec<u8> = ordered
        .iter()
        .flat_map(|(record, _)| record.to_le_bytes())
        .collect();
    for (chunk, (before, after)) in pointers.chunks(64).zip(sorted.chunks(64)).enumerate() {
        if before != after {
            changes.push(change(
                "career_sort",
                "커리어 계획 순서",
                career.begin + (chunk * 64) as u64,
                before.to_vec(),
                after.to_vec(),
            )?);
        }
    }
    if appending {
        changes.push(change(
            "career_add_end",
            "커리어 계획 개수",
            career.holder + 8,
            career.end.to_le_bytes().to_vec(),
            (career.end + 8).to_le_bytes().to_vec(),
        )?);
    }
    ensure!(!changes.is_empty(), "변경할 커리어 계획이 없습니다");
    Ok(changes)
}
fn career_snapshot(
    p: &impl RelationMemory,
    e: &Entity,
    guards: &mut Vec<Guard>,
) -> Result<CareerPlans> {
    let holder = q(&guard(p, guards, e.address + 0xC8, 8)?, 0);
    let mut out = CareerPlans {
        rows: vec![],
        holder,
        begin: 0,
        end: 0,
        capacity: 0,
        reusable: None,
        allocation_available: false,
        allocation_reason: None,
    };
    if holder == 0 {
        return Ok(out);
    }
    ensure!(pointer(holder), "커리어 계획 컬렉션 포인터 검증 실패");
    (out.begin, out.end, out.capacity) = career_vector_bounds(&guard(p, guards, holder, 24)?)?;
    if out.begin == out.end {
        return Ok(out);
    }
    let pointers = guard(p, guards, out.begin, (out.end - out.begin) as usize)?;
    let mut seen = std::collections::HashSet::new();
    for (slot, bytes) in pointers.chunks_exact(8).enumerate() {
        let record = q(bytes, 0);
        ensure!(
            pointer(record) && seen.insert(record),
            "커리어 계획 레코드가 잘못되었거나 중복되었습니다"
        );
        let raw = guard(p, guards, record, 16)?;
        out.rows.push(CareerPlanRow {
            slot,
            condition: raw[8],
            scope: raw[9],
            job: raw[10],
            object_type: raw[11],
            target: None,
            editable_club: blank_career(&raw),
            record,
            raw,
        });
    }
    Ok(out)
}
pub fn inspect(s: &Session, e: &Entity) -> Result<PersonRelations> {
    crate::compatibility::require_legacy_feature(s, "선호 관계·커리어 조회")?;
    let detail = backend::detail(s, e)?;
    let p = Process::open(s.pid, false)?;
    backend::validate_with(&p, s)?;
    let mut out = snapshot(&p, s, e, detail.guards)?;
    let mut career_guards = vec![];
    match career_snapshot(&p, e, &mut career_guards) {
        Ok(mut career) => {
            for row in &mut career.rows {
                if row.object_type == 25 {
                    let mut target_guards = vec![];
                    if let Ok(target) = career_competition_target(
                        &p, s, q(&row.raw, 0), &mut target_guards,
                    ) {
                        row.target = Some(target);
                        career_guards.extend(target_guards);
                    }
                    continue;
                }
                if let Some(typ) = match row.object_type {
                    3 => Some(1),
                    9 => Some(8),
                    _ => None,
                } {
                    let mut target_guards = vec![];
                    if let Ok(target) = linked(&p, s, q(&row.raw, 0), typ, &mut target_guards) {
                        row.editable_club = typ == 1
                            && [1, 2].contains(&row.condition)
                            && row.scope == 1
                            && CAREER_JOBS.iter().any(|(v, _)| *v == row.job);
                        row.target = Some(target);
                        career_guards.extend(target_guards);
                    }
                }
            }
            out.guards.extend(career_guards);
            out.career = Some(career);
        }
        Err(error) => out
            .notes
            .push(format!("커리어 계획을 읽을 수 없습니다: {error:#}")),
    }
    if out.spare_slots > 0 {
        out.append_bytes = Some(guard(&p, &mut out.guards, out.end, 16)?);
    }
    match crate::native_alloc::available(s) {
        Ok(()) => out.vector_growth_available = true,
        Err(error) => out.vector_growth_reason = Some(format!("{error:#}")),
    }
    if let Some(career) = out.career.as_mut() {
        match career_growth::available(s) {
            Ok(()) => career.allocation_available = true,
            Err(error) => career.allocation_reason = Some(format!("{error:#}")),
        }
        // A deletion keeps the detached pointer and record allocated. Reuse is
        // allowed only while the complete saved post-delete state still agrees.
        if let Some((row, checked)) =
            find_reusable_deleted_career(&p, s, e, career, &all_history())?
        {
            career.reusable = Some(row);
            out.guards.extend(checked);
        }
    }
    validate_guards(&p, &out.guards)?;
    Ok(out)
}

/// Existing favourite strength/reason fields can join the standard scalar editor.
/// Call only after backend::detail has validated the person. Missing or malformed
/// relation data should remain an explicit unavailable section in the caller.
pub fn append(p: &Process, s: &Session, d: &mut Detail) -> Result<()> {
    let out = snapshot(p, s, &d.entity, vec![])?;
    d.guards.extend(out.guards);
    d.entity.nationality.clear();
    d.entity.nationality_id = None;
    if let Some(n) = out.primary_nationality {
        d.entity.nationality = n.name.clone();
        d.entity.nationality_id = Some(n.uid);
        d.notes.push(format!("기본 국적: {} ({})", n.name, n.uid));
    }
    for n in &out.names {
        d.notes.push(format!("{}: {}", n.field.label(), n.text));
    }
    for row in out.rows.iter().filter(|r| r.editable_favourite) {
        let name = &row.target.as_ref().unwrap().name;
        for (suffix, label, offset, storage, max, value) in [
            ("level", "선호도", 12, Storage::U8, 100., row.level as f64),
            (
                "reason",
                "선호 이유",
                8,
                Storage::U16,
                4.,
                row.reason as f64,
            ),
        ] {
            d.fields.push(Field {
                key: format!("relation_{}_{}", row.slot, suffix),
                label: format!("{name} · {label}"),
                section: "좋아하는 구단".into(),
                value,
                min: 0.,
                max,
                step: 1.,
                hint: if suffix == "reason" {
                    "0 없음 · 1 서포터 · 2 선수 · 3 감독 · 4 스태프".into()
                } else {
                    "기존 관계 항목의 0–100 선호도".into()
                },
                binding: Some(Binding {
                    address: out.begin + row.slot as u64 * 16 + offset,
                    storage,
                    scale: 1.,
                    original: row.raw[offset as usize..offset as usize + storage.size()].to_vec(),
                }),
            });
        }
    }
    Ok(())
}
fn row<'a>(snapshot: &'a PersonRelations, slot: usize) -> Result<&'a RelationRow> {
    snapshot
        .rows
        .get(slot)
        .filter(|r| r.slot == slot)
        .context("관계 항목이 변경됐습니다")
}
fn change(key: &str, label: &str, address: u64, old: Vec<u8>, new: Vec<u8>) -> Result<Change> {
    ensure!(
        pointer(address) && !old.is_empty() && old.len() <= 64 && old.len() == new.len(),
        "변경 바이트 범위 검증 실패"
    );
    ensure!(old != new, "실제 변경한 값이 없습니다");
    Ok(Change {
        key: key.into(),
        label: label.into(),
        before: 0.,
        after: 0.,
        address,
        old_bytes: old,
        new_bytes: new,
    })
}
fn favourite_record(old: &[u8], target: u64, reason: u16, level: u8) -> Result<Vec<u8>> {
    ensure!(
        old.len() == 16 && old[10..12] == [1, 3],
        "검증된 선호 구단 템플릿이 아닙니다"
    );
    ensure!(
        pointer(target) && reason <= 4 && level <= 100,
        "선호 구단 값의 범위가 다릅니다"
    );
    let mut new = old.to_vec();
    new[..8].copy_from_slice(&target.to_le_bytes());
    new[8..10].copy_from_slice(&reason.to_le_bytes());
    new[12] = level;
    Ok(new)
}
fn favourite_reason(reason: u16) -> &'static str {
    match reason {
        0 => "없음",
        1 => "서포터",
        2 => "선수",
        3 => "감독",
        4 => "스태프",
        _ => "미확인",
    }
}
fn check_duplicate(
    snapshot: &PersonRelations,
    target: u64,
    kind: u8,
    exclude: Option<usize>,
) -> Result<()> {
    ensure!(
        !snapshot
            .rows
            .iter()
            .any(|r| Some(r.slot) != exclude && r.kind == kind && q(&r.raw, 0) == target),
        "같은 대상의 관계가 이미 있습니다"
    );
    Ok(())
}
fn club_target(
    p: &Process,
    s: &Session,
    club: &Entity,
    guards: &mut Vec<Guard>,
) -> Result<LinkedRecord> {
    ensure!(club.kind == EntityKind::Club, "구단을 선택하세요");
    let d = backend::detail(s, club)?;
    guards.extend(d.guards);
    let target = linked(p, s, club.address, 1, guards)?;
    ensure!(target.uid == club.id, "구단 ID가 변경됐습니다");
    Ok(target)
}
fn nation_target(
    p: &Process,
    s: &Session,
    donor: &Entity,
    id: u32,
    guards: &mut Vec<Guard>,
) -> Result<LinkedRecord> {
    ensure!(
        matches!(
            donor.kind,
            EntityKind::Player | EntityKind::Staff | EntityKind::Person
        ),
        "국적이 확인된 인물을 선택하세요"
    );
    let d = backend::detail(s, donor)?;
    guards.extend(d.guards);
    let address = q(&guard(p, guards, donor.address + 0x68, 8)?, 0);
    let target = linked(p, s, address, 8, guards)?;
    ensure!(
        target.uid == id,
        "선택한 국가가 해당 인물의 현재 기본 국적과 다릅니다"
    );
    Ok(target)
}
/// The UI and planner share this check so a stale/malformed snapshot cannot
/// advertise an append that the actual vector and retained slot do not support.
pub fn relation_add_unavailable_reason(snapshot: &PersonRelations) -> Option<&'static str> {
    if snapshot.holder == 0 {
        return Some("관계 컬렉션이 없습니다. 새 목록 할당은 지원하지 않습니다");
    }
    let header = [
        snapshot.begin.to_le_bytes(),
        snapshot.end.to_le_bytes(),
        snapshot.capacity.to_le_bytes(),
    ]
    .concat();
    if !pointer(snapshot.holder)
        || vector_bounds(&header).is_err()
        || (snapshot.end - snapshot.begin) / 16 != snapshot.rows.len() as u64
        || (snapshot.capacity - snapshot.end) / 16 != snapshot.spare_slots as u64
    {
        return Some("관계 목록 범위를 검증하지 못했습니다. 관계를 다시 조회하세요");
    }
    if snapshot.rows.len() >= MAX_RELATION_COUNT {
        return Some("관계 목록이 게임에서 허용하는 최대 255개에 도달했습니다");
    }
    if snapshot.spare_slots == 0 {
        if snapshot.vector_growth_available && snapshot.capacity - snapshot.begin < MAX_VECTOR_BYTES {
            return None;
        }
        return Some("관계 목록에 여유 공간이 없으며, 현재 연결에서는 배열 확장을 확인하지 못했습니다");
    }
    if !snapshot.append_bytes.as_ref().is_some_and(|raw| raw.len() == 16) {
        return Some("추가 슬롯의 원본을 검증하지 못했습니다. 관계를 다시 조회하세요");
    }
    None
}
fn append_change(snapshot: &PersonRelations, new: Vec<u8>, growth: &mut Option<RelationVectorGrowth>) -> Result<Vec<Change>> {
    if let Some(reason) = relation_add_unavailable_reason(snapshot) {
        bail!("{reason}");
    }
    ensure!(new.len() == 16, "관계 레코드 크기 오류");
    if snapshot.spare_slots == 0 {
        *growth = Some(growth::intent(snapshot, new)?);
        return Ok(vec![]);
    }
    let old = snapshot
        .append_bytes
        .clone()
        .context("추가 항목 공간의 원본이 없습니다. 관계를 다시 조회하세요")?;
    ensure!(old.len() == 16, "추가 항목 원본 크기가 다릅니다");
    // Publish the end pointer only after the complete new record has read back.
    let mut changes = vec![];
    if old != new {
        changes.push(change(
            "relation_append",
            "새 관계",
            snapshot.end,
            old,
            new,
        )?);
    }
    changes.push(change(
        "relation_end",
        "관계 개수",
        snapshot.holder + 8,
        snapshot.end.to_le_bytes().to_vec(),
        (snapshot.end + 16).to_le_bytes().to_vec(),
    )?);
    Ok(changes)
}

/// Remove a logical element, retain every allocation and rotate the removed
/// value into the first unused slot. Other active elements keep their order.
fn delete_changes(
    holder: u64,
    begin: u64,
    end: u64,
    size: usize,
    slot: usize,
    raw: &[u8],
) -> Result<Vec<Change>> {
    ensure!(
        pointer(holder)
            && pointer(begin)
            && [8, 16].contains(&size)
            && raw.len() as u64 == end.checked_sub(begin).context("벡터 범위 오류")?
            && raw.len() % size == 0
            && slot < raw.len() / size,
        "삭제할 항목 범위가 다릅니다"
    );
    let start = slot * size;
    let mut rotated = raw[start..].to_vec();
    rotated.rotate_left(size);
    let mut changes = vec![];
    for (index, (old, new)) in raw[start..].chunks(64).zip(rotated.chunks(64)).enumerate() {
        if old != new {
            changes.push(change(
                "relation_remove_shift",
                "남은 항목 순서 유지",
                begin + (start + index * 64) as u64,
                old.to_vec(),
                new.to_vec(),
            )?);
        }
    }
    changes.push(change(
        "relation_remove_end",
        "항목 삭제",
        holder + 8,
        end.to_le_bytes().to_vec(),
        (end - size as u64).to_le_bytes().to_vec(),
    )?);
    Ok(changes)
}

fn guarded_bytes(guards: &[Guard], address: u64, size: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(size);
    for offset in 0..size {
        let at = address
            .checked_add(offset as u64)
            .context("검증 주소 범위 오류")?;
        let value = guards
            .iter()
            .filter_map(|g| {
                at.checked_sub(g.address)
                    .and_then(|i| g.expected.get(i as usize))
            })
            .next()
            .context("기록의 원본 검증 범위가 없습니다")?;
        ensure!(
            guards.iter().all(|g| at
                .checked_sub(g.address)
                .and_then(|i| g.expected.get(i as usize))
                .is_none_or(|b| b == value)),
            "서로 다른 원본 검증 기록입니다"
        );
        bytes.push(*value);
    }
    Ok(bytes)
}

/// Removing an owned pointer-vector entry does not require understanding its
/// target or editing its record. Verify the complete active vector and every
/// immutable record before granting deletion; live apply rechecks these guards.
pub fn career_delete_check(snapshot: &PersonRelations, slot: usize) -> Result<()> {
    let career = snapshot
        .career
        .as_ref()
        .context("검증된 커리어 계획 조회 자료가 없습니다")?;
    ensure!(pointer(career.holder), "커리어 계획 컬렉션 포인터 검증 실패");
    let owner = snapshot
        .entity
        .address
        .checked_add(0xC8)
        .context("커리어 계획 소유자 주소 범위 오류")?;
    ensure!(
        guarded_bytes(&snapshot.guards, owner, 8)? == career.holder.to_le_bytes(),
        "커리어 계획 소유자 연결이 원본과 다릅니다"
    );
    let bounds = career_vector_bounds(&guarded_bytes(&snapshot.guards, career.holder, 24)?)?;
    ensure!(
        bounds == (career.begin, career.end, career.capacity),
        "커리어 계획 벡터 경계가 원본과 다릅니다"
    );
    let count = ((career.end - career.begin) / 8) as usize;
    ensure!(
        count == career.rows.len() && slot < count,
        "삭제할 커리어 계획이 없거나 항목 개수가 다릅니다"
    );
    let pointers = guarded_bytes(&snapshot.guards, career.begin, count * 8)?;
    let mut seen = std::collections::HashSet::new();
    for (index, row) in career.rows.iter().enumerate() {
        ensure!(
            row.slot == index
                && pointer(row.record)
                && seen.insert(row.record)
                && q(&pointers, index * 8) == row.record,
            "커리어 계획 레코드 순서·소유권 검증 실패"
        );
        ensure!(
            row.raw.len() == 16
                && row.raw[8..12] == [row.condition, row.scope, row.job, row.object_type],
            "커리어 계획 레코드 내용이 조회 자료와 다릅니다"
        );
        ensure!(
            guarded_bytes(&snapshot.guards, row.record, 16)? == row.raw,
            "커리어 계획 레코드가 검증된 원본과 다릅니다"
        );
    }
    Ok(())
}

/// Reconstruct the whole bounded removal from the immutable pre-write guards.
/// Comparing its changes also prevents an undo receipt from restoring arbitrary
/// addresses merely because they happen to fall near a native vector.
fn validate_deleted_plan(
    plan: &RelationPlan,
    holder: u64,
    begin: u64,
    end: u64,
    capacity: u64,
    size: usize,
    slot: usize,
) -> Result<()> {
    let header = guarded_bytes(&plan.guards, holder, 24)?;
    let (b, e, c) = if size == 8 {
        career_vector_bounds(&header)?
    } else {
        vector_bounds(&header)?
    };
    ensure!(
        b == begin && c == capacity && e == end + size as u64,
        "삭제 후 컬렉션 경계가 변경됐습니다"
    );
    let raw = guarded_bytes(&plan.guards, b, (e - b) as usize)?;
    let expected = delete_changes(holder, b, e, size, slot, &raw)?;
    ensure!(
        serde_json::to_vec(&expected)? == serde_json::to_vec(&plan.changes)?,
        "삭제 기록의 변경 범위가 다릅니다"
    );
    Ok(())
}

fn reusable_deleted_career(
    p: &impl RelationMemory,
    career: &CareerPlans,
    plan: &RelationPlan,
) -> Result<(CareerPlanRow, Vec<Guard>)> {
    let RelationRequest::DeleteCareerPlan { slot } = plan.request else {
        bail!("커리어 삭제 기록이 아닙니다")
    };
    validate_deleted_plan(
        plan,
        career.holder,
        career.begin,
        career.end,
        career.capacity,
        8,
        slot,
    )?;
    let mut checked = expected_guards(&plan.guards, &plan.changes);
    validate_guards(p, &checked)?;
    let record = q(&guard(p, &mut checked, career.end, 8)?, 0);
    ensure!(
        pointer(record) && !career.rows.iter().any(|row| row.record == record),
        "보존 레코드가 활성 항목과 중복됩니다"
    );
    let raw = guard(p, &mut checked, record, 16)?;
    ensure!(
        raw == guarded_bytes(&plan.guards, record, 16)?,
        "삭제한 커리어 레코드가 변경됐습니다"
    );
    // Validate the detached record's supported layout without changing it.
    if !blank_career(&raw) {
        career_record(&raw, q(&raw, 0), raw[8], raw[10])?;
    }
    Ok((
        CareerPlanRow {
            slot: career.rows.len(),
            condition: raw[8],
            scope: raw[9],
            job: raw[10],
            object_type: raw[11],
            target: None,
            editable_club: true,
            record,
            raw,
        },
        checked,
    ))
}

fn find_reusable_deleted_career(
    p: &impl RelationMemory,
    session: &Session,
    entity: &Entity,
    career: &CareerPlans,
    receipts: &[RelationReceipt],
) -> Result<Option<(CareerPlanRow, Vec<Guard>)>> {
    // Display pagination must not discard ownership evidence. An explicit
    // successful undo also revokes its original receipt, even if bytes later
    // happen to resemble that old deletion's post-state again.
    let undone: std::collections::HashSet<&str> = receipts
        .iter()
        .filter(|r| r.status.starts_with("undone"))
        .filter_map(|r| r.undo_of.as_deref())
        .collect();
    let session_bytes = serde_json::to_vec(session)?;
    for receipt in receipts.iter().filter(|r| {
        r.status.starts_with("applied")
            && r.undo_of.is_none()
            && !undone.contains(r.id.as_str())
            && r.plan.entity.kind == entity.kind
            && r.plan.entity.id == entity.id
            && r.plan.entity.address == entity.address
            && matches!(r.plan.request, RelationRequest::DeleteCareerPlan { .. })
    }) {
        if serde_json::to_vec(&receipt.plan.session)? != session_bytes {
            continue;
        }
        if let Ok(retained) = reusable_deleted_career(p, career, &receipt.plan) {
            return Ok(Some(retained));
        }
    }
    Ok(None)
}

pub fn career_add_mode(career: &CareerPlans) -> Option<&'static str> {
    if career
        .rows
        .iter()
        .any(|row| row.editable_club && blank_career(&row.raw))
    {
        Some("blank")
    } else if career.reusable.is_some() && career.end < career.capacity {
        Some("deleted_record")
    } else if career.allocation_available && career.rows.len()<64 {
        Some("native_allocation")
    } else {
        None
    }
}

pub fn prepare(s: &Session, e: &Entity, request: &RelationRequest) -> Result<RelationPlan> {
    let snapshot = inspect(s, e)?;
    prepare_retained(s, &snapshot, request)
}
pub fn prepare_retained(
    s: &Session,
    snapshot: &PersonRelations,
    request: &RelationRequest,
) -> Result<RelationPlan> {
    let p = Process::open(s.pid, false)?;
    backend::validate_with(&p, s)?;
    validate_guards(&p, &snapshot.guards)?;
    let mut guards = vec![];
    let (target, name) = match request {
        RelationRequest::AddCareerTarget { target,.. } | RelationRequest::SetCareerTarget { target,.. } => (Some(targets::resolve_ref(s,target,&mut guards)?),None),
        RelationRequest::AddSocialRelation { target, kind, .. } | RelationRequest::SetSocialRelation { target, kind, .. } =>
            (Some(social::target(&p,s,target,*kind,&mut guards)?),None),
        RelationRequest::DeleteSocialRelation { .. } => (None,None),
        RelationRequest::AddCareerClub { club, .. }
        | RelationRequest::SetCareerClub { club, .. }
        | RelationRequest::SetFavouriteClub { club, .. }
        | RelationRequest::AddFavouriteClub { club, .. } => {
            (Some(club_target(&p, s, club, &mut guards)?), None)
        }
        RelationRequest::SetSecondaryNationality {
            donor,
            expected_nation_uid,
            ..
        }
        | RelationRequest::AddSecondaryNationality {
            donor,
            expected_nation_uid,
        } => (
            Some(nation_target(
                &p,
                s,
                donor,
                *expected_nation_uid,
                &mut guards,
            )?),
            None,
        ),
        RelationRequest::ReuseName { field, donor } => {
            ensure!(
                matches!(
                    donor.kind,
                    EntityKind::Player | EntityKind::Staff | EntityKind::Person
                ),
                "이름 원본 인물을 선택하세요"
            );
            guards.extend(backend::detail(s, donor)?.guards);
            (
                None,
                Some(read_name(&p, donor.address, *field, &mut guards)?),
            )
        }
        RelationRequest::SetCareerDetails { .. }
        | RelationRequest::DeleteCareerPlan { .. }
        | RelationRequest::DeleteFavouriteClub { .. }
        | RelationRequest::DeleteSecondaryNationality { .. } => (None, None),
    };
    let plan = prepare_snapshot(s, snapshot, request, target.as_ref(), name.as_ref(), guards)?;
    validate_guards(&p, &plan.guards)?;
    Ok(plan)
}
/// Pure planner shared by retained live previews and explicitly read-only
/// snapshot previews. Live apply always rebuilds from verified process data.
pub(crate) fn prepare_snapshot(
    s: &Session,
    snapshot: &PersonRelations,
    request: &RelationRequest,
    target: Option<&LinkedRecord>,
    source: Option<&NameRecord>,
    extra_guards: Vec<Guard>,
) -> Result<RelationPlan> {
    ensure!(
        snapshot
            .rows
            .iter()
            .enumerate()
            .all(|(slot, row)| row.slot == slot && row.raw.len() == 16),
        "관계 조회 레코드가 손상되었습니다"
    );
    ensure!(
        snapshot.career.as_ref().is_none_or(|career| career
            .rows
            .iter()
            .enumerate()
            .all(|(slot, row)| row.slot == slot && row.raw.len() == 16 && pointer(row.record))),
        "커리어 조회 레코드가 손상되었습니다"
    );
    let mut guards = snapshot.guards.clone();
    guards.extend(extra_guards);
    let e = &snapshot.entity;
    let club_target = |club: &Entity| -> Result<&LinkedRecord> {
        let target = target.context("선택한 구단 정보가 없습니다")?;
        ensure!(
            club.kind == EntityKind::Club
                && target.object_type == 1
                && target.uid == club.id
                && target.address == club.address,
            "구단 연결 정보가 다릅니다"
        );
        Ok(target)
    };
    let nation_target = |expected: u32| -> Result<&LinkedRecord> {
        let target = target.context("선택한 국가 정보가 없습니다")?;
        ensure!(
            target.object_type == 8 && target.uid == expected,
            "선택한 국가가 해당 인물의 현재 기본 국적과 다릅니다"
        );
        Ok(target)
    };
    let mut growth = None;
    let mut career_growth = None;
    let (summary, before, after, changes) = match request {
        RelationRequest::AddCareerTarget {target:reference,condition,scope,job} | RelationRequest::SetCareerTarget {target:reference,condition,scope,job,..} => {
            let target=target.context("선택한 커리어 대상이 없습니다")?;
            ensure!(target.uid==reference.uid&&target.object_type==reference.object_type,"선택한 커리어 대상과 조회 정보가 다릅니다");
            let slot=match request {RelationRequest::SetCareerTarget{slot,..}=>Some(*slot),_=>None};
            let (summary,before,after,changes,created)=prepare_career_target(snapshot,&guards,target,slot,*condition,*scope,*job)?;
            career_growth=created;(summary,before,after,changes)
        }
        RelationRequest::AddSocialRelation { .. } | RelationRequest::SetSocialRelation { .. } | RelationRequest::DeleteSocialRelation { .. } => social::prepare(snapshot,request,target,&mut growth)?,
        RelationRequest::SetCareerDetails { slot, condition, scope, job } => {
            career_details_check(snapshot, *slot)?;
            let career = snapshot.career.as_ref().unwrap();
            let old = &career.rows[*slot];
            let new = career_detail_record(&old.raw, *condition, *scope, *job)?;
            ensure!(!career.rows.iter().any(|row| row.slot != *slot && row.raw[..12] == new[..12]), "같은 커리어 계획이 이미 있습니다");
            let changes = career_edit_changes(career, &guards, old, new, false, "career_details", "커리어 계획 조건")?;
            ("커리어 계획 조건 수정".into(), format!("{} · {} · 범위 {}", career_condition_label(old.condition), career_job_label(old.job), old.scope), format!("{} · {} · 범위 {}", career_condition_label(*condition), career_job_label(*job), scope), changes)
        }
        RelationRequest::AddCareerClub {
            club,
            condition,
            job,
        } => {
            let career = snapshot
                .career
                .as_ref()
                .context("검증된 커리어 계획 조회 자료가 없습니다")?;
            let blank = career
                .rows
                .iter()
                .find(|row| row.editable_club && blank_career(&row.raw));
            let target = club_target(club)?;
            if blank.or(career.reusable.as_ref()).is_none() {
                let blank_record=[vec![0;8],vec![1,1,0,255,2,0,0,0]].concat();
                let new=career_record(&blank_record,target.address,*condition,*job)?;
                career_growth=Some(career_growth::intent(snapshot,new)?);
                return Ok(RelationPlan{session:s.clone(),entity:e.clone(),request:request.clone(),summary:"구단 커리어 계획 추가".into(),before:format!("커리어 계획 {}개",career.rows.len()),after:format!("{} · {} · {}",target.name,career_condition_label(*condition),career_job_label(*job)),changes:vec![],guards,growth:None,allocation:None,retained_original:None,career_growth,career_storage:None});
            }
            let old = blank.or(career.reusable.as_ref()).unwrap();
            let new = career_record(&old.raw, target.address, *condition, *job)?;
            ensure!(
                !career.rows.iter().any(|row| row.raw[..12] == new[..12]),
                "같은 커리어 계획이 이미 있습니다"
            );
            let changes = career_edit_changes(
                career,
                &guards,
                old,
                new,
                blank.is_none(),
                "career_add_record",
                "커리어 계획 추가",
            )?;
            (
                "구단 커리어 계획 추가".into(),
                if blank.is_some() {
                    "이미 할당된 빈 항목".into()
                } else {
                    "삭제 후 보존된 항목 재사용".into()
                },
                format!(
                    "{} · {} · {}",
                    target.name,
                    career_condition_label(*condition),
                    career_job_label(*job)
                ),
                changes,
            )
        }
        RelationRequest::DeleteCareerPlan { slot } => {
            career_delete_check(snapshot, *slot)?;
            let career = snapshot
                .career
                .as_ref()
                .context("검증된 커리어 계획 조회 자료가 없습니다")?;
            let old = career
                .rows
                .get(*slot)
                .filter(|row| row.slot == *slot)
                .context("삭제할 커리어 계획이 없습니다")?;
            let raw: Vec<u8> = career
                .rows
                .iter()
                .flat_map(|row| row.record.to_le_bytes())
                .collect();
            (
                "커리어 계획 삭제".into(),
                format!(
                    "{} · {} · {}",
                    old.target
                        .as_ref()
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| if blank_career(&old.raw) {
                            "빈 항목".into()
                        } else {
                            format!("{} 이름 미확인 (유형 {})", career_target_type_label(old.object_type), old.object_type)
                        }),
                    career_condition_label(old.condition),
                    career_job_label(old.job)
                ),
                "목록에서 삭제 · 원본 레코드 보존".into(),
                delete_changes(career.holder, career.begin, career.end, 8, *slot, &raw)?,
            )
        }
        RelationRequest::DeleteFavouriteClub { slot }
        | RelationRequest::DeleteSecondaryNationality { slot } => {
            let old = row(snapshot, *slot)?;
            let favourite = matches!(request, RelationRequest::DeleteFavouriteClub { .. });
            ensure!(
                if favourite {
                    old.editable_favourite
                } else {
                    old.editable_secondary_nationality
                },
                "선택한 관계 유형은 삭제할 수 없습니다"
            );
            let raw: Vec<u8> = snapshot
                .rows
                .iter()
                .flat_map(|row| row.raw.iter().copied())
                .collect();
            (
                if favourite {
                    "선호 구단 삭제".into()
                } else {
                    "추가 국적 삭제".into()
                },
                old.target
                    .as_ref()
                    .context("삭제 대상 연결 정보가 없습니다")?
                    .name
                    .clone(),
                "목록에서 삭제 · 다른 관계 유지".into(),
                delete_changes(
                    snapshot.holder,
                    snapshot.begin,
                    snapshot.end,
                    16,
                    *slot,
                    &raw,
                )?,
            )
        }
        RelationRequest::SetCareerClub {
            slot,
            club,
            condition,
            job,
        } => {
            let career = snapshot
                .career
                .as_ref()
                .context("검증된 커리어 계획 조회 자료가 없습니다")?;
            let old = career
                .rows
                .get(*slot)
                .filter(|row| row.slot == *slot)
                .context("해당 커리어 계획이 없습니다. 새 항목은 자동 할당하지 않습니다")?;
            career_delete_check(snapshot, *slot)?;
            let target = club_target(club)?;
            let new = career_record(&old.raw, target.address, *condition, *job)?;
            ensure!(
                !career.rows.iter().any(|row| row.slot != *slot
                    && row.raw.len() == 16
                    && row.raw[..12] == new[..12]),
                "같은 커리어 계획이 이미 있습니다"
            );
            let changes = career_edit_changes(
                career,
                &guards,
                old,
                new,
                false,
                "career_replace",
                "커리어 계획",
            )?;
            (
                "구단 커리어 계획 수정".into(),
                if blank_career(&old.raw) {
                    "이미 할당된 빈 커리어 계획".into()
                } else {
                    format!(
                        "{} · {} · {}",
                        old.target
                            .as_ref()
                            .map(|t| t.name.as_str())
                            .unwrap_or("알 수 없는 구단"),
                        career_condition_label(old.condition),
                        career_job_label(old.job)
                    )
                },
                format!(
                    "{} · {} · {}",
                    target.name,
                    career_condition_label(*condition),
                    career_job_label(*job)
                ),
                changes,
            )
        }
        RelationRequest::SetFavouriteClub {
            slot,
            club,
            reason,
            level,
        } => {
            let old = row(&snapshot, *slot)?;
            ensure!(old.editable_favourite, "이 관계는 수정할 수 없습니다");
            let target = club_target(club)?;
            check_duplicate(&snapshot, target.address, 3, Some(*slot))?;
            let new = favourite_record(&old.raw, target.address, *reason, *level)?;
            (
                "선호 구단 관계 수정".into(),
                format!(
                    "{} · {} · 선호도 {}",
                    old.target
                        .as_ref()
                        .context("선호 구단 연결 정보가 없습니다")?
                        .name,
                    favourite_reason(old.reason),
                    old.level
                ),
                format!(
                    "{} · {} · 선호도 {level}",
                    target.name,
                    favourite_reason(*reason)
                ),
                vec![change(
                    "relation_replace",
                    "선호 구단",
                    snapshot.begin + *slot as u64 * 16,
                    old.raw.clone(),
                    new,
                )?],
            )
        }
        RelationRequest::AddFavouriteClub {
            template_slot,
            club,
            reason,
            level,
        } => {
            let mut template = vec![0; 8];
            template.extend(FAVOURITE_TAIL);
            if let Some(slot) = template_slot {
                let old = row(snapshot, *slot)?;
                ensure!(
                    old.editable_favourite,
                    "복사할 검증된 선호 구단 관계가 필요합니다"
                );
                template = old.raw.clone();
            }
            let target = club_target(club)?;
            check_duplicate(&snapshot, target.address, 3, None)?;
            let new = favourite_record(&template, target.address, *reason, *level)?;
            (
                "선호 구단 관계 추가".into(),
                format!("기존 {}개 관계 유지", snapshot.rows.len()),
                format!(
                    "{} · {} · 선호도 {level}",
                    target.name,
                    favourite_reason(*reason)
                ),
                append_change(snapshot, new, &mut growth)?,
            )
        }
        RelationRequest::SetSecondaryNationality {
            slot,
            donor: _,
            expected_nation_uid,
        } => {
            let old = row(&snapshot, *slot)?;
            ensure!(
                old.editable_secondary_nationality,
                "추가 국적 관계가 아닙니다"
            );
            let target = nation_target(*expected_nation_uid)?;
            ensure!(
                snapshot
                    .primary_nationality
                    .as_ref()
                    .is_none_or(|n| n.uid != target.uid),
                "기본 국적과 같은 국적입니다"
            );
            check_duplicate(&snapshot, target.address, 9, Some(*slot))?;
            let mut new = old.raw.clone();
            new[..8].copy_from_slice(&target.address.to_le_bytes());
            (
                "추가 국적 수정".into(),
                old.target
                    .as_ref()
                    .context("추가 국적 연결 정보가 없습니다")?
                    .name
                    .clone(),
                target.name.clone(),
                vec![change(
                    "relation_replace",
                    "추가 국적",
                    snapshot.begin + *slot as u64 * 16,
                    old.raw.clone(),
                    new,
                )?],
            )
        }
        RelationRequest::AddSecondaryNationality {
            donor: _,
            expected_nation_uid,
        } => {
            let target = nation_target(*expected_nation_uid)?;
            ensure!(
                snapshot
                    .primary_nationality
                    .as_ref()
                    .is_none_or(|n| n.uid != target.uid),
                "기본 국적과 같은 국적입니다"
            );
            check_duplicate(&snapshot, target.address, 9, None)?;
            let mut new = target.address.to_le_bytes().to_vec();
            new.extend(NATIONALITY_TAIL);
            (
                "추가 국적 추가".into(),
                format!("기존 {}개 관계 유지", snapshot.rows.len()),
                target.name.clone(),
                append_change(snapshot, new, &mut growth)?,
            )
        }
        RelationRequest::ReuseName { field, donor } => {
            ensure!(
                matches!(
                    donor.kind,
                    EntityKind::Player | EntityKind::Staff | EntityKind::Person
                ),
                "이름 원본 인물을 선택하세요"
            );
            let source = source.context("원본 인물의 이름 조회 자료가 없습니다")?;
            ensure!(
                source.field == *field && pointer(source.record) && !source.text.is_empty(),
                "원본 인물의 해당 이름이 비어 있습니다"
            );
            let old = snapshot
                .names
                .iter()
                .find(|n| n.field == *field)
                .context("대상 인물의 이름 조회 자료가 없습니다")?;
            (
                format!("{} 데이터베이스 이름 재사용", field.label()),
                old.text.clone(),
                source.text.clone(),
                vec![change(
                    "name_reference",
                    field.label(),
                    e.address + field.offset(),
                    old.record.to_le_bytes().to_vec(),
                    source.record.to_le_bytes().to_vec(),
                )?],
            )
        }
    };
    Ok(RelationPlan {
        session: s.clone(),
        entity: e.clone(),
        request: request.clone(),
        summary,
        before,
        after,
        changes,
        guards,
        growth,
        allocation: None,
        retained_original: None,
        career_growth,
        career_storage: None,
    })
}

trait RelationMemory {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>>;
    fn write(&self, address: u64, bytes: &[u8]) -> Result<()>;
}
/// Read-only view of the immutable receipt preimage, used to reconstruct the
/// permitted career mutation rather than trusting recorded write addresses.
struct GuardMemory<'a>(&'a [Guard]);
impl RelationMemory for GuardMemory<'_> {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        guarded_bytes(self.0, address, size)
    }
    fn write(&self, _: u64, _: &[u8]) -> Result<()> {
        bail!("원본 검증 기록은 읽기 전용입니다")
    }
}
impl RelationMemory for Process {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        Process::read(self, address, size)
    }
    fn write(&self, address: u64, bytes: &[u8]) -> Result<()> {
        Process::write(self, address, bytes)
    }
}
fn validate_guards(p: &impl RelationMemory, guards: &[Guard]) -> Result<()> {
    ensure!(!guards.is_empty(), "대상 검증 정보가 없습니다");
    for g in guards {
        ensure!(
            pointer(g.address) && !g.expected.is_empty() && g.expected.len() <= 256,
            "검증 범위 오류"
        );
        ensure!(
            p.read(g.address, g.expected.len())? == g.expected,
            "대상 또는 연결 데이터가 바뀌었습니다. 다시 미리보기 하세요"
        );
    }
    Ok(())
}
fn expected_guards(guards: &[Guard], changes: &[Change]) -> Vec<Guard> {
    guards
        .iter()
        .map(|g| {
            let mut out = g.clone();
            for c in changes {
                for (i, byte) in c.new_bytes.iter().enumerate() {
                    if let Some(offset) = (c.address + i as u64).checked_sub(g.address) {
                        if offset < out.expected.len() as u64 {
                            out.expected[offset as usize] = *byte;
                        }
                    }
                }
            }
            out
        })
        .collect()
}
fn write_batch(p: &impl RelationMemory, changes: &[Change], post: &[Guard]) -> Result<()> {
    let mut attempted = 0;
    let result = (|| -> Result<()> {
        for c in changes {
            ensure!(
                p.read(c.address, c.old_bytes.len())? == c.old_bytes,
                "변경 전 값이 바뀌었습니다"
            );
            attempted += 1;
            p.write(c.address, &c.new_bytes)?;
            ensure!(
                p.read(c.address, c.new_bytes.len())? == c.new_bytes,
                "관계 쓰기 재읽기 불일치"
            );
        }
        validate_guards(p, post)
    })();
    if let Err(err) = result {
        let mut failures = vec![];
        for c in changes[..attempted].iter().rev() {
            if let Err(e) = p.write(c.address, &c.old_bytes).and_then(|()| {
                ensure!(
                    p.read(c.address, c.old_bytes.len())? == c.old_bytes,
                    "복구 재읽기 실패"
                );
                Ok(())
            }) {
                failures.push(format!("{}: {e:#}", c.label));
            }
        }
        ensure!(
            failures.is_empty(),
            "관계 적용 실패 및 복구 실패: {err:#}; {}",
            failures.join("; ")
        );
        bail!("관계 적용 실패. 수정한 바이트를 복구했습니다: {err:#}");
    }
    Ok(())
}
fn folder() -> Result<PathBuf> {
    let path = transaction::data_dir().join("relation-history");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
fn receipt_path(id: &str, prepared: bool) -> Result<PathBuf> {
    ensure!(
        !id.is_empty() && id.len() < 100 && id.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "잘못된 기록 ID"
    );
    Ok(folder()?.join(format!(
        "{id}{}.json",
        if prepared { "-prepared" } else { "" }
    )))
}
fn persist(receipt: &RelationReceipt, prepared: bool) -> Result<()> {
    use std::io::Write;
    let path = receipt_path(&receipt.id, prepared)?;
    let temp = path.with_extension("pending");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(receipt)?)?;
    file.sync_all()?;
    drop(file);
    // Each state has a new name, preserving the immutable pre-write backup.
    ensure!(!path.exists(), "기록 파일이 이미 존재합니다");
    std::fs::rename(&temp, &path)?;
    Ok(())
}
fn read_history(path: &std::path::Path) -> Vec<RelationReceipt> {
    let mut out: Vec<RelationReceipt> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().is_some_and(|s| s == "json")
                && !e.file_name().to_string_lossy().ends_with("-prepared.json")
        })
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    out.sort_by(|a, b| b.time.cmp(&a.time));
    out
}
fn all_history() -> Vec<RelationReceipt> {
    folder().map(|path| read_history(&path)).unwrap_or_default()
}
pub fn history() -> Vec<RelationReceipt> {
    let mut out = all_history();
    out.truncate(500);
    out
}
fn commit(plan: RelationPlan, undo_of: Option<String>) -> Result<RelationReceipt> {
    crate::compatibility::require_legacy_feature(&plan.session, "관계·커리어 수정")?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    let mut receipt = RelationReceipt {
        id: format!("{time}-{}", plan.session.pid),
        time,
        status: "prepared".into(),
        plan,
        undo_of,
    };
    if let Err(error) = persist(&receipt, true) {
        growth::mark_uncertain(&receipt.plan);
        return Err(error).context("변경 전 관계 백업을 저장하지 못했습니다");
    }
    let mut resume_error = None;
    let mut write_started = false;
    let result = (|| -> Result<()> {
        let p = Process::open(receipt.plan.session.pid, true)?;
        backend::validate_with(&p, &receipt.plan.session)?;
        let suspended = p.suspend()?;
        let applied = (|| -> Result<()> {
            backend::validate_with(&p, &receipt.plan.session)?;
            if receipt.plan.growth.is_some() {
                growth::validate_ownership(&receipt.plan)?;
            }
            if receipt.plan.career_growth.is_some() { career_growth::verify_ownership(&receipt.plan)?; }
            validate_guards(&p, &receipt.plan.guards)?;
            write_started = true;
            write_batch(
                &p,
                &receipt.plan.changes,
                &expected_guards(&receipt.plan.guards, &receipt.plan.changes),
            )
        })();
        if let Err(e) = suspended.resume() {
            resume_error = Some(format!("{e:#}"));
        }
        applied
    })();
    match result {
        Ok(()) => {
            receipt.status = if receipt.undo_of.is_some() {
                "undone".into()
            } else {
                "applied".into()
            };
            if let Some(e) = resume_error {
                receipt.status.push_str(&format!("_resume_warning: {e}"));
            }
            if receipt.plan.growth.is_some() {
                if let Some(warning) = growth::record_publication(&receipt.plan, receipt.undo_of.is_some()) {
                    // Publication already succeeded. Ledger failures must not
                    // turn a committed mutation into a reported write failure.
                    receipt.status.push_str(&format!("_allocation_log_warning: {warning}"));
                }
            }
            if receipt.plan.career_growth.is_some() { career_growth::record_publication(&receipt.plan, receipt.undo_of.is_some()); }
            if let Err(e) = persist(&receipt, false) {
                receipt.status.push_str(&format!("_log_warning: {e:#}"));
            }
            Ok(receipt)
        }
        Err(e) => {
            growth::mark_uncertain(&receipt.plan);
            receipt.status = format!("failed: {e:#}");
            if let Some(r) = resume_error {
                receipt.status.push_str(&format!("; resume_warning: {r}"));
            }
            let _ = persist(&receipt, false);
            Err(commit_failure(&receipt.id, &receipt.status, receipt.plan.growth.is_some() || receipt.plan.career_growth.is_some(), write_started))
        }
    }
}

/// The caller must supply the retained preview plan. A new plan is compared byte
/// for byte; changed baseline data is rejected rather than silently accepted.
pub fn apply(s: &Session, plan: &RelationPlan) -> Result<RelationReceipt> {
    crate::compatibility::require_legacy_feature(s, "관계·커리어 수정")?;
    ensure!(
        serde_json::to_vec(s)? == serde_json::to_vec(&plan.session)?,
        "미리보기 세션이 다릅니다"
    );
    let fresh = prepare(s, &plan.entity, &plan.request)?;
    ensure!(
        serde_json::to_vec(&fresh)? == serde_json::to_vec(plan)?,
        "미리보기 이후 데이터가 변경됐습니다. 다시 검토하세요"
    );
    if fresh.career_growth.is_some() {
        let materialized=career_growth::allocate_and_materialize(&fresh)?;
        commit(materialized,None)
    } else if fresh.growth.is_some() {
        let materialized = growth::allocate_and_materialize(&fresh)?;
        commit(materialized, None)
    } else {
        commit(fresh, None)
    }
}

/// Undo takes a local receipt ID, never caller-supplied addresses or bytes.
/// It requires every post-edit byte and dependency to remain unchanged.
pub fn undo(id: &str) -> Result<RelationReceipt> {
    let receipt: RelationReceipt =
        serde_json::from_slice(&std::fs::read(receipt_path(id, false)?)?)?;
    ensure!(
        receipt.id == id && receipt.status.starts_with("applied") && receipt.undo_of.is_none(),
        "되돌릴 수 있는 관계 기록이 아닙니다"
    );
    let original = receipt.plan;
    crate::compatibility::require_legacy_feature(&original.session, "관계·커리어 되돌리기")?;
    backend::validate_session(&original.session)?;
    let detail = backend::detail(&original.session, &original.entity)?;
    let p = Process::open(original.session.pid, false)?;
    let current = snapshot(&p, &original.session, &original.entity, detail.guards)?;
    if original.growth.is_some() {
        growth::validate_ownership(&original)?;
    }
    if original.career_growth.is_some() { career_growth::verify_ownership(&original)?; }
    validate_undo_ranges(&p, &current, &original)?;
    let guards = expected_guards(&original.guards, &original.changes);
    validate_guards(&p, &guards)?;
    for c in &original.changes {
        ensure!(
            p.read(c.address, c.new_bytes.len())? == c.new_bytes,
            "적용 후 관계가 변경되어 되돌릴 수 없습니다"
        );
    }
    let reverse = reverse_plan(&original);
    commit(reverse, Some(id.into()))
}

fn reverse_plan(original: &RelationPlan) -> RelationPlan {
    RelationPlan {
        session: original.session.clone(),
        entity: original.entity.clone(),
        request: original.request.clone(),
        summary: format!("되돌리기: {}", original.summary),
        before: original.after.clone(),
        after: original.before.clone(),
        guards: expected_guards(&original.guards, &original.changes),
        growth: original.growth.clone(),
        allocation: original.allocation.clone(),
        retained_original: original.retained_original.clone(),
        career_growth: original.career_growth.clone(),
        career_storage: original.career_storage.clone(),
        changes: original
            .changes
            .iter()
            .rev()
            // A suspended FM thread may retain the replacement buffer's
            // address across undo. Detach it without clearing its records.
            .filter(|c| (original.growth.is_none() || c.key == "relation_grow_publish") && (original.career_growth.is_none() || c.key == "career_create_publish"))
            .map(|c| Change {
                key: c.key.clone(),
                label: c.label.clone(),
                before: c.after,
                after: c.before,
                address: c.address,
                old_bytes: c.new_bytes.clone(),
                new_bytes: c.old_bytes.clone(),
            })
            .collect(),
    }
}

fn validate_career_edit_plan(p: &impl RelationMemory, original: &RelationPlan) -> Result<()> {
    let preimage = GuardMemory(&original.guards);
    let before = career_snapshot(&preimage, &original.entity, &mut vec![])?;
    if matches!(&original.request,RelationRequest::AddCareerTarget{..}|RelationRequest::SetCareerTarget{..}) {
        let (reference,slot,condition,scope,job)=match &original.request {
            RelationRequest::AddCareerTarget{target,condition,scope,job}=>(target,None,*condition,*scope,*job),
            RelationRequest::SetCareerTarget{target,slot,condition,scope,job}=>(target,Some(*slot),*condition,*scope,*job),
            _=>unreachable!(),
        };
        let retained;
        let blank=before.rows.iter().find(|r|blank_career(&r.raw));
        let old=if let Some(slot)=slot {before.rows.get(slot).context("원래 커리어 항목이 없습니다")?} else if let Some(blank)=blank {blank} else {
            ensure!(before.end<before.capacity,"재사용 커리어 공간이 없습니다");
            let record=q(&guarded_bytes(&original.guards,before.end,8)?,0);let raw=guarded_bytes(&original.guards,record,16)?;
            retained=CareerPlanRow{slot:before.rows.len(),condition:raw[8],scope:raw[9],job:raw[10],object_type:raw[11],target:None,editable_club:false,record,raw}; &retained
        };
        let key=if slot.is_some(){"career_target"}else{"career_target_add"};
        let modified=original.changes.iter().find(|c|c.key==key&&c.address==old.record&&c.new_bytes.len()==16).context("커리어 수정 레코드가 없습니다")?;
        let address=q(&modified.new_bytes,0);
        let header=guarded_bytes(&original.guards,address,16)?;
        ensure!(u32::from_le_bytes(header[12..16].try_into().unwrap())==reference.uid,"커리어 대상 원본 ID가 다릅니다");
        let target=LinkedRecord{uid:reference.uid,name:String::new(),address,object_type:reference.object_type};
        let new=career_target_record(&old.raw,&target,condition,scope,job)?;
        let expected=career_edit_changes(&before,&original.guards,old,new,slot.is_none()&&blank.is_none(),key,"커리어 계획")?;
        ensure!(serde_json::to_vec(&expected)?==serde_json::to_vec(&original.changes)?,"커리어 대상 수정 기록이 다릅니다");
        return validate_guards(p,&expected_guards(&original.guards,&expected));
    }
    if let RelationRequest::SetCareerDetails { slot, condition, scope, job } = &original.request {
        let old = before.rows.get(*slot).context("원래 커리어 항목이 없습니다")?;
        let new = career_detail_record(&old.raw, *condition, *scope, *job)?;
        let expected = career_edit_changes(&before, &original.guards, old, new, false, "career_details", "커리어 계획 조건")?;
        ensure!(serde_json::to_vec(&expected)? == serde_json::to_vec(&original.changes)?, "커리어 조건 수정 기록이 다릅니다");
        return validate_guards(p, &expected_guards(&original.guards, &expected));
    }
    let retained;
    let (old, club, condition, job, appending, key, label) = match &original.request {
        RelationRequest::AddCareerClub {
            club,
            condition,
            job,
        } => {
            let blank = before.rows.iter().find(|row| blank_career(&row.raw));
            let old = if let Some(blank) = blank {
                blank
            } else {
                ensure!(before.end < before.capacity, "원래 재사용 공간이 없습니다");
                let record = q(&guarded_bytes(&original.guards, before.end, 8)?, 0);
                let raw = guarded_bytes(&original.guards, record, 16)?;
                retained = CareerPlanRow {
                    slot: before.rows.len(),
                    condition: raw[8],
                    scope: raw[9],
                    job: raw[10],
                    object_type: raw[11],
                    target: None,
                    editable_club: true,
                    record,
                    raw,
                };
                &retained
            };
            (
                old,
                club,
                *condition,
                *job,
                blank.is_none(),
                "career_add_record",
                "커리어 계획 추가",
            )
        }
        RelationRequest::SetCareerClub {
            slot,
            club,
            condition,
            job,
        } => {
            let old = before
                .rows
                .get(*slot)
                .context("원래 커리어 항목이 없습니다")?;
            (
                old,
                club,
                *condition,
                *job,
                false,
                "career_replace",
                "커리어 계획",
            )
        }
        _ => bail!("커리어 수정 기록이 아닙니다"),
    };
    ensure!(club.kind == EntityKind::Club, "커리어 구단 형식이 다릅니다");
    let new = career_record(&old.raw, club.address, condition, job)?;
    let expected = career_edit_changes(&before, &original.guards, old, new, appending, key, label)?;
    ensure!(
        serde_json::to_vec(&expected)? == serde_json::to_vec(&original.changes)?,
        "커리어 수정·정렬 기록 범위가 다릅니다"
    );
    // Includes the full post-sort vector, every record and the holder/end.
    // A native reorder or any unrelated record change makes undo stale.
    validate_guards(p, &expected_guards(&original.guards, &expected))
}

fn validate_undo_ranges(
    p: &impl RelationMemory,
    current: &PersonRelations,
    original: &RelationPlan,
) -> Result<()> {
    if original.career_growth.is_some() { return career_growth::validate_undo(p,original); }
    if original.growth.is_some() {
        return growth::validate_undo(p, current, original);
    }
    // Only addresses permitted by this typed operation can be restored.
    match &original.request {
        RelationRequest::DeleteSocialRelation { slot }
        | RelationRequest::DeleteFavouriteClub { slot }
        | RelationRequest::DeleteSecondaryNationality { slot } => {
            validate_deleted_plan(
                original,
                current.holder,
                current.begin,
                current.end,
                current.capacity,
                16,
                *slot,
            )?;
        }
        RelationRequest::DeleteCareerPlan { slot } => {
            let mut checked = vec![];
            let career = career_snapshot(p, &original.entity, &mut checked)?;
            validate_deleted_plan(
                original,
                career.holder,
                career.begin,
                career.end,
                career.capacity,
                8,
                *slot,
            )?;
            validate_guards(p, &checked)?;
        }
        RelationRequest::AddCareerClub { .. } | RelationRequest::SetCareerClub { .. } | RelationRequest::SetCareerDetails { .. } | RelationRequest::AddCareerTarget{..} | RelationRequest::SetCareerTarget{..} => {
            validate_career_edit_plan(p, original)?;
        }
        RelationRequest::ReuseName { field, .. } => ensure!(
            original.changes.len() == 1
                && original.changes[0].address == original.entity.address + field.offset()
                && original.changes[0].old_bytes.len() == 8
                && original.changes[0].new_bytes.len() == 8,
            "이름 기록 범위가 다릅니다"
        ),
        RelationRequest::SetSocialRelation { slot, .. }
        | RelationRequest::SetFavouriteClub { slot, .. }
        | RelationRequest::SetSecondaryNationality { slot, .. } => {
            ensure!(
                original.changes.len() == 1
                    && *slot < current.rows.len()
                    && original.changes[0].address == current.begin + *slot as u64 * 16
                    && original.changes[0].old_bytes.len() == 16
                    && original.changes[0].new_bytes.len() == 16,
                "관계 수정 기록 범위가 다릅니다"
            );
        }
        RelationRequest::AddSocialRelation { .. }
        | RelationRequest::AddFavouriteClub { .. }
        | RelationRequest::AddSecondaryNationality { .. } => {
            let end_change = original
                .changes
                .last()
                .context("관계 추가 경계 기록이 없습니다")?;
            ensure!(
                [1, 2].contains(&original.changes.len())
                    && current.end >= current.begin + 16
                    && (original.changes.len() == 1
                        || (original.changes[0].address == current.end - 16
                            && original.changes[0].old_bytes.len() == 16
                            && original.changes[0].new_bytes.len() == 16))
                    && end_change.address == current.holder + 8
                    && end_change.old_bytes.len() == 8
                    && end_change.new_bytes.len() == 8
                    && q(&end_change.old_bytes, 0) == current.end - 16
                    && q(&end_change.new_bytes, 0) == current.end,
                "관계 추가 기록 범위가 다릅니다"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};
    fn competition_name_fixture(full: &[u8], short: &[u8]) -> Mock {
        let mut bytes = BTreeMap::new();
        for (address, data) in [
            (0x50040u64, 0x60000u64.to_le_bytes().to_vec()),
            (0x50048u64, 0x61000u64.to_le_bytes().to_vec()),
            (0x60000u64, [vec![0; 4], full.to_vec(), vec![0; 192 - full.len()]].concat()),
            (0x61000u64, [vec![0; 4], short.to_vec(), vec![0; 192 - short.len()]].concat()),
        ] {
            for (offset, byte) in data.into_iter().enumerate() {
                bytes.insert(address + offset as u64, byte);
            }
        }
        Mock { bytes: RefCell::new(bytes), partial_failure: RefCell::new(None), writes: RefCell::new(vec![]) }
    }
    #[test]
    fn career_competition_name_prefers_full_and_guards_exact_text() {
        let p = competition_name_fixture("잉글랜드 프리미어 리그".as_bytes(), "프리미어 리그".as_bytes());
        let mut guards = vec![];
        assert_eq!(career_competition_name(&p, 0x50000, &mut guards).unwrap(), "잉글랜드 프리미어 리그");
        validate_guards(&p, &guards).unwrap();
        assert!(p.writes.borrow().is_empty());
        p.bytes.borrow_mut().insert(0x60004, b'x');
        assert!(validate_guards(&p, &guards).is_err());
    }
    #[test]
    fn career_competition_name_falls_back_only_for_empty_full_name() {
        let p = competition_name_fixture(b"", b"Premier League");
        let mut guards = vec![];
        assert_eq!(career_competition_name(&p, 0x50000, &mut guards).unwrap(), "Premier League");
        validate_guards(&p, &guards).unwrap();
        // The empty full-name field is part of the guarded selection too.
        p.bytes.borrow_mut().insert(0x60004, b'x');
        assert!(validate_guards(&p, &guards).is_err());
        for invalid in [b"Bad\nName".as_slice(), &[0xff], &[b'x'; 192]] {
            let p = competition_name_fixture(invalid, b"Premier League");
            assert!(career_competition_name(&p, 0x50000, &mut vec![]).is_err());
        }
        let p = competition_name_fixture(b"", b"");
        assert!(career_competition_name(&p, 0x50000, &mut vec![]).is_err());
    }
    #[test]
    fn reconnect_error_requires_growth_and_started_write_and_survives_context() {
        for growth in [false, true] {
            for write_started in [false, true] {
                let error = commit_failure("123-77", "synthetic failed transaction", growth, write_started)
                    .context("synthetic outer context");
                assert_eq!(requires_reconnect(&error), growth && write_started);
                if growth && write_started {
                    assert!(format!("{error:#}").contains("123-77"));
                    assert!(format!("{error:#}").contains("synthetic failed transaction"));
                }
            }
        }
        assert!(!requires_reconnect(&anyhow::anyhow!("RelationWriteFailure 123-77")));
    }
    #[test]
    #[ignore = "explicit synthetic offline fixture export"]
    fn export_offline_fixture_v09() {
        fixtures::write(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("output/relations-fixture-v09"),
        )
        .unwrap();
    }
    fn mock_snapshot(snapshot: &PersonRelations) -> Mock {
        let mut bytes = BTreeMap::new();
        for guard in &snapshot.guards {
            for (offset, byte) in guard.expected.iter().enumerate() {
                bytes.insert(guard.address + offset as u64, *byte);
            }
        }
        Mock {
            bytes: RefCell::new(bytes),
            partial_failure: RefCell::new(None),
            writes: RefCell::new(vec![]),
        }
    }
    fn deletion_plan(snapshot: &PersonRelations, request: RelationRequest) -> RelationPlan {
        let (index, _, _) = fixtures::data();
        prepare_snapshot(&index.session, snapshot, &request, None, None, vec![]).unwrap()
    }
    fn three_careers() -> PersonRelations {
        let (_, _, mut snapshots) = fixtures::data();
        let mut snapshot = snapshots.remove(0);
        let career = snapshot.career.as_mut().unwrap();
        let base = career.rows[0].clone();
        for slot in 1..3 {
            let mut row = base.clone();
            row.slot = slot;
            row.record += slot as u64 * 0x100;
            row.job = if slot == 1 { 16 } else { 2 };
            row.raw[10] = row.job;
            snapshot.guards.push(Guard {
                address: row.record,
                expected: row.raw.clone(),
            });
            career.rows.push(row);
        }
        career.end = career.begin + 24;
        career.capacity = career.begin + 32;
        for g in &mut snapshot.guards {
            if g.address == career.holder {
                g.expected = [
                    career.begin.to_le_bytes(),
                    career.end.to_le_bytes(),
                    career.capacity.to_le_bytes(),
                ]
                .concat();
            }
            if g.address == career.begin {
                g.expected = career
                    .rows
                    .iter()
                    .flat_map(|r| r.record.to_le_bytes())
                    .collect();
            }
        }
        snapshot
    }
    fn mixed_careers(types: &[u8]) -> PersonRelations {
        let (_, _, mut snapshots) = fixtures::data();
        let mut snapshot = snapshots.remove(0);
        snapshot.guards.retain(|g| {
            g.address != snapshot.entity.address + 0xC8 && !(0x90000..0x93000).contains(&g.address)
        });
        let rows: Vec<_> = types
            .iter()
            .enumerate()
            .map(|(slot, typ)| {
                let mut raw = (0x60000u64 + slot as u64 * 0x10).to_le_bytes().to_vec();
                raw.extend([1, 1, 1, *typ, 2, slot as u8, 165, 255]);
                if *typ == 255 {
                    raw = vec![0; 8];
                    raw.extend([1, 1, 0, 255, 2, 0, 0, 0]);
                }
                CareerPlanRow {
                    slot,
                    condition: raw[8],
                    scope: raw[9],
                    job: raw[10],
                    object_type: *typ,
                    target: None,
                    editable_club: [3, 255].contains(typ),
                    record: 0xE0000 + slot as u64 * 0x100,
                    raw,
                }
            })
            .collect();
        let career = CareerPlans {
            holder: 0xD0000,
            begin: 0xD1000,
            end: 0xD1000 + rows.len() as u64 * 8,
            capacity: 0xD1000 + (rows.len() as u64 + 1) * 8,
            rows,
            reusable: None,
        allocation_available: false,
        allocation_reason: None,
        };
        snapshot.guards.extend([
            Guard {
                address: snapshot.entity.address + 0xC8,
                expected: career.holder.to_le_bytes().to_vec(),
            },
            Guard {
                address: career.holder,
                expected: [
                    career.begin.to_le_bytes(),
                    career.end.to_le_bytes(),
                    career.capacity.to_le_bytes(),
                ]
                .concat(),
            },
            Guard {
                address: career.begin,
                expected: career
                    .rows
                    .iter()
                    .flat_map(|r| r.record.to_le_bytes())
                    .collect(),
            },
        ]);
        snapshot.guards.extend(career.rows.iter().map(|r| Guard {
            address: r.record,
            expected: r.raw.clone(),
        }));
        snapshot.career = Some(career);
        snapshot
    }
    fn mixed_career_plan(
        snapshot: &PersonRelations,
        slot: Option<usize>,
        unchanged: bool,
    ) -> RelationPlan {
        let (index, _, _) = fixtures::data();
        let mut club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
        let (condition, job) = if unchanged {
            let row = &snapshot.career.as_ref().unwrap().rows[slot.unwrap()];
            club.address = q(&row.raw, 0);
            (row.condition, row.job)
        } else {
            (2, 16)
        };
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let request = if let Some(slot) = slot {
            RelationRequest::SetCareerClub {
                slot,
                club,
                condition,
                job,
            }
        } else {
            RelationRequest::AddCareerClub {
                club,
                condition,
                job,
            }
        };
        prepare_snapshot(
            &index.session,
            snapshot,
            &request,
            Some(&target),
            None,
            vec![],
        )
        .unwrap()
    }
    fn assert_career_roundtrip(
        snapshot: &PersonRelations,
        plan: &RelationPlan,
        expected_slots: &[usize],
    ) {
        let memory = mock_snapshot(snapshot);
        let original = memory.bytes.borrow().clone();
        validate_guards(&memory, &plan.guards).unwrap();
        write_batch(
            &memory,
            &plan.changes,
            &expected_guards(&plan.guards, &plan.changes),
        )
        .unwrap();
        let current = career_snapshot(&memory, &snapshot.entity, &mut vec![]).unwrap();
        assert_eq!(
            current.rows.iter().map(|r| r.record).collect::<Vec<_>>(),
            expected_slots
                .iter()
                .map(|slot| 0xE0000 + *slot as u64 * 0x100)
                .collect::<Vec<_>>()
        );
        validate_undo_ranges(&memory, snapshot, plan).unwrap();
        let reverse = reverse_plan(plan);
        write_batch(&memory, &reverse.changes, &plan.guards).unwrap();
        assert_eq!(*memory.bytes.borrow(), original);
    }
    fn mixed_retained_career() -> (PersonRelations, RelationPlan, PersonRelations) {
        let snapshot = mixed_careers(&[9, 3, 25, 5, 42, 3, 9, 25, 5, 77, 3, 25]);
        let deleted = deletion_plan(&snapshot, RelationRequest::DeleteCareerPlan { slot: 1 });
        let memory = mock_snapshot(&snapshot);
        write_batch(
            &memory,
            &deleted.changes,
            &expected_guards(&deleted.guards, &deleted.changes),
        )
        .unwrap();
        let mut career = career_snapshot(&memory, &snapshot.entity, &mut vec![]).unwrap();
        let (retained, guards) = reusable_deleted_career(&memory, &career, &deleted).unwrap();
        career.reusable = Some(retained);
        let mut post = snapshot.clone();
        post.career = Some(career);
        post.guards = guards;
        (snapshot, deleted, post)
    }
    #[test]
    fn career_blank_add_and_set_sort_all_native_ranks_and_restore_original_order() {
        let snapshot = mixed_careers(&[9, 3, 25, 5, 255, 42, 3, 9, 25, 5, 77, 3]);
        for slot in [None, Some(4)] {
            let plan = mixed_career_plan(&snapshot, slot, false);
            assert!(
                plan.changes
                    .iter()
                    .filter(|c| c.key == "career_sort")
                    .count()
                    >= 2
            );
            assert!(!plan.changes.iter().any(|c| c.key == "career_add_end"));
            assert_eq!(plan.changes[0].address, 0xE0400);
            assert_eq!(
                &plan.changes[0].new_bytes[12..],
                &plan.changes[0].old_bytes[12..]
            );
            assert_career_roundtrip(&snapshot, &plan, &[1, 4, 6, 11, 2, 8, 0, 7, 3, 9, 5, 10]);
        }
    }
    #[test]
    fn career_existing_edit_repairs_unsorted_vector_even_when_record_is_unchanged() {
        let snapshot = mixed_careers(&[5, 3, 25, 9, 42, 3, 25, 9, 5, 42, 3]);
        for unchanged in [false, true] {
            let plan = mixed_career_plan(&snapshot, Some(1), unchanged);
            assert_eq!(
                plan.changes.iter().any(|c| c.key == "career_replace"),
                !unchanged
            );
            assert_career_roundtrip(&snapshot, &plan, &[1, 5, 10, 2, 6, 3, 7, 0, 8, 4, 9]);
        }
    }
    #[test]
    fn career_retained_readd_sorts_before_end_publication_and_chained_undo_restores_every_byte() {
        let (original, deleted, post) = mixed_retained_career();
        let added = mixed_career_plan(&post, None, false);
        assert_eq!(added.changes.last().unwrap().key, "career_add_end");
        assert_career_roundtrip(&post, &added, &[5, 10, 1, 2, 7, 11, 0, 6, 3, 8, 4, 9]);
        let memory = mock_snapshot(&original);
        let before = memory.bytes.borrow().clone();
        write_batch(
            &memory,
            &deleted.changes,
            &expected_guards(&deleted.guards, &deleted.changes),
        )
        .unwrap();
        write_batch(
            &memory,
            &added.changes,
            &expected_guards(&added.guards, &added.changes),
        )
        .unwrap();
        assert!(validate_undo_ranges(&memory, &post, &deleted).is_err());
        validate_undo_ranges(&memory, &post, &added).unwrap();
        write_batch(&memory, &reverse_plan(&added).changes, &added.guards).unwrap();
        validate_undo_ranges(&memory, &post, &deleted).unwrap();
        write_batch(&memory, &reverse_plan(&deleted).changes, &deleted.guards).unwrap();
        assert_eq!(*memory.bytes.borrow(), before);
    }
    #[test]
    fn career_sort_each_forward_and_undo_write_failure_restores_complete_preimage() {
        let blank = mixed_careers(&[9, 3, 25, 5, 255, 42, 3, 9, 25, 5, 77, 3]);
        let (_, _, retained) = mixed_retained_career();
        for (snapshot, slot) in [(&blank, None), (&blank, Some(4)), (&retained, None)] {
            let plan = mixed_career_plan(snapshot, slot, false);
            for fail in &plan.changes {
                let memory = mock_snapshot(snapshot);
                let before = memory.bytes.borrow().clone();
                memory.partial_failure.replace(Some(fail.address));
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
            let reverse = reverse_plan(&plan);
            for fail in &reverse.changes {
                let memory = mock_snapshot(snapshot);
                write_batch(
                    &memory,
                    &plan.changes,
                    &expected_guards(&plan.guards, &plan.changes),
                )
                .unwrap();
                let applied = memory.bytes.borrow().clone();
                memory.partial_failure.replace(Some(fail.address));
                assert!(write_batch(&memory, &reverse.changes, &plan.guards).is_err());
                assert_eq!(*memory.bytes.borrow(), applied);
            }
        }
    }
    #[test]
    fn career_sort_full_vector_and_records_reject_external_changes_and_forged_undo() {
        let snapshot = mixed_careers(&[9, 3, 25, 5, 255, 42, 3, 9, 25, 5, 77, 3]);
        let plan = mixed_career_plan(&snapshot, None, false);
        let career = snapshot.career.as_ref().unwrap();
        // Both pointer chunks, an unrelated type, unknown tail, holder and end.
        for address in [
            career.begin,
            career.begin + 88,
            0xE0A0B,
            0xE0A0F,
            career.holder,
            career.holder + 8,
        ] {
            let memory = mock_snapshot(&snapshot);
            let original = memory.bytes.borrow()[&address];
            memory.bytes.borrow_mut().insert(address, original ^ 1);
            assert!(validate_guards(&memory, &plan.guards).is_err());
            assert!(memory.writes.borrow().is_empty());
            let memory = mock_snapshot(&snapshot);
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            let original = memory.bytes.borrow()[&address];
            memory.bytes.borrow_mut().insert(address, original ^ 1);
            memory.writes.borrow_mut().clear();
            assert!(validate_undo_ranges(&memory, &snapshot, &plan).is_err());
            assert!(memory.writes.borrow().is_empty());
        }
        let memory = mock_snapshot(&snapshot);
        write_batch(
            &memory,
            &plan.changes,
            &expected_guards(&plan.guards, &plan.changes),
        )
        .unwrap();
        for alteration in 0..3 {
            let mut forged = plan.clone();
            let at = forged
                .changes
                .iter()
                .position(|c| c.key == "career_sort")
                .unwrap();
            match alteration {
                0 => forged.changes[at].address += 8,
                1 => {
                    forged.changes.remove(at);
                }
                _ => forged.changes[at].new_bytes[0] ^= 1,
            }
            assert!(validate_undo_ranges(&memory, &snapshot, &forged).is_err());
        }
        // Even a permutation of valid, equal-rank pointers is an external edit.
        let first = memory.read(career.begin, 8).unwrap();
        let second = memory.read(career.begin + 8, 8).unwrap();
        memory.write(career.begin, &second).unwrap();
        memory.write(career.begin + 8, &first).unwrap();
        assert!(career_snapshot(&memory, &snapshot.entity, &mut vec![]).is_ok());
        assert!(validate_undo_ranges(&memory, &snapshot, &plan).is_err());
    }
    #[test]
    fn career_reuse_reads_older_than_500_receipts_without_trusting_corrupt_files() {
        let (_, deleted, post) = mixed_retained_career();
        let career = post.career.as_ref().unwrap();
        let memory = mock_snapshot(&post);
        let original = RelationReceipt {
            id: "1-77".into(),
            time: "0001".into(),
            status: "applied".into(),
            plan: deleted,
            undo_of: None,
        };
        struct TempHistory(PathBuf);
        impl Drop for TempHistory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let directory = TempHistory(std::env::temp_dir().join(format!(
            "touchline-career-history-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        )));
        std::fs::create_dir(&directory.0).unwrap();
        std::fs::write(
            directory.0.join("1-77.json"),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        for i in 2..=502 {
            let mut receipt = original.clone();
            receipt.id = format!("{i}-77");
            receipt.time = format!("{i:04}");
            receipt.plan.session.pid += 1;
            std::fs::write(
                directory.0.join(format!("{i}-77.json")),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
        }
        std::fs::write(directory.0.join("corrupt.json"), b"{broken").unwrap();
        std::fs::write(
            directory.0.join("503-77-prepared.json"),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        let all = read_history(&directory.0);
        assert_eq!(all.len(), 502);
        assert_eq!(all.last().unwrap().id, original.id);
        let display = &all[..500];
        assert!(
            find_reusable_deleted_career(
                &memory,
                &original.plan.session,
                &post.entity,
                career,
                display
            )
            .unwrap()
            .is_none()
        );
        let retained = find_reusable_deleted_career(
            &memory,
            &original.plan.session,
            &post.entity,
            career,
            &all,
        )
        .unwrap()
        .unwrap();
        assert_eq!(retained.0.record, 0xE0100);
        assert!(memory.writes.borrow().is_empty());
    }
    #[test]
    fn career_reuse_excludes_other_sessions_invalid_plans_and_successfully_undone_receipts() {
        let (_, deleted, post) = mixed_retained_career();
        let career = post.career.as_ref().unwrap();
        let memory = mock_snapshot(&post);
        let original = RelationReceipt {
            id: "1-77".into(),
            time: "0001".into(),
            status: "applied".into(),
            plan: deleted,
            undo_of: None,
        };
        let find = |receipts: &[RelationReceipt]| {
            find_reusable_deleted_career(
                &memory,
                &original.plan.session,
                &post.entity,
                career,
                receipts,
            )
            .unwrap()
        };
        assert!(find(std::slice::from_ref(&original)).is_some());
        let mut warning = original.clone();
        warning.status = "applied_resume_warning: synthetic warning".into();
        assert!(find(&[warning]).is_some());
        for variation in 0..4 {
            let mut invalid = original.clone();
            match variation {
                0 => invalid.status = "undone".into(),
                1 => invalid.plan.session.created += 1,
                2 => invalid.plan.changes[0].address += 8,
                _ => invalid.undo_of = Some("another-receipt".into()),
            }
            assert!(find(&[invalid]).is_none());
        }
        let mut undo = original.clone();
        undo.id = "2-77".into();
        undo.status = "undone".into();
        undo.undo_of = Some(original.id.clone());
        assert!(find(&[original.clone(), undo.clone()]).is_none());
        undo.status = "failed: no writes".into();
        assert!(find(&[original.clone(), undo]).is_some());
        memory.bytes.borrow_mut().insert(0xE010F, 0);
        assert!(find(std::slice::from_ref(&original)).is_none());
        assert!(memory.writes.borrow().is_empty());
    }
    #[test]
    fn delete_relations_preserves_order_detached_value_and_exact_undo() {
        let (_, _, snapshots) = fixtures::data();
        for request in [
            RelationRequest::DeleteFavouriteClub { slot: 0 },
            RelationRequest::DeleteSecondaryNationality { slot: 1 },
        ] {
            let snapshot = &snapshots[0];
            let memory = mock_snapshot(snapshot);
            let original = memory.bytes.borrow().clone();
            let plan = deletion_plan(snapshot, request.clone());
            validate_guards(&memory, &plan.guards).unwrap();
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            let slot = match request {
                RelationRequest::DeleteFavouriteClub { slot }
                | RelationRequest::DeleteSecondaryNationality { slot } => slot,
                _ => unreachable!(),
            };
            assert_eq!(
                memory.read(snapshot.begin, 16).unwrap(),
                snapshot.rows[1 - slot].raw
            );
            assert_eq!(
                memory.read(snapshot.end - 16, 16).unwrap(),
                snapshot.rows[slot].raw
            );
            let mut current = snapshot.clone();
            current.end -= 16;
            current.rows.remove(slot);
            validate_undo_ranges(&memory, &current, &plan).unwrap();
            let reverse = reverse_plan(&plan);
            validate_guards(&memory, &reverse.guards).unwrap();
            write_batch(&memory, &reverse.changes, &plan.guards).unwrap();
            assert_eq!(*memory.bytes.borrow(), original);
        }
    }
    #[test]
    fn noneditable_career_deletion_preserves_records_order_and_undo() {
        for types in [vec![25], vec![25, 9, 5, 42]] {
            for slot in 0..types.len() {
                let snapshot = mixed_careers(&types);
                let career = snapshot.career.as_ref().unwrap();
                assert!(!career.rows[slot].editable_club);
                career_delete_check(&snapshot, slot).unwrap();
                let plan = deletion_plan(&snapshot, RelationRequest::DeleteCareerPlan { slot });
                assert!(!plan.before.contains("빈 항목"));
                if types[slot] == 25 {
                    assert!(plan.before.contains("대회 이름 미확인"));
                }
                let expected = (0..types.len()).filter(|index| *index != slot).collect::<Vec<_>>();
                assert_career_roundtrip(&snapshot, &plan, &expected);

                let memory = mock_snapshot(&snapshot);
                let before = memory.bytes.borrow().clone();
                write_batch(&memory, &plan.changes, &expected_guards(&plan.guards, &plan.changes)).unwrap();
                for row in &career.rows {
                    assert_eq!(memory.read(row.record, 16).unwrap(), row.raw);
                }
                assert_eq!(memory.bytes.borrow().len(), before.len());
                assert_eq!(q(&memory.read(career.end - 8, 8).unwrap(), 0), career.rows[slot].record);
                let current = career_snapshot(&memory, &snapshot.entity, &mut vec![]).unwrap();
                // Known career target types share the verified 16-byte layout;
                // unknown target kinds remain delete-only.
                assert_eq!(reusable_deleted_career(&memory, &current, &plan).is_ok(), types[slot] != 42);
            }
        }
    }
    #[test]
    fn career_delete_requires_complete_consistent_structural_guards() {
        let snapshot = mixed_careers(&[25, 9, 3]);
        let career = snapshot.career.as_ref().unwrap();
        let guarded_addresses = [snapshot.entity.address + 0xC8, career.holder, career.begin, career.rows[0].record];
        for address in guarded_addresses {
            let mut missing = snapshot.clone();
            missing.guards.retain(|g| g.address != address);
            assert!(career_delete_check(&missing, 0).is_err());
            let mut changed = snapshot.clone();
            changed.guards.iter_mut().find(|g| g.address == address).unwrap().expected[0] ^= 1;
            assert!(career_delete_check(&changed, 0).is_err());
        }
        for mutation in 0..8 {
            let mut invalid = snapshot.clone();
            let c = invalid.career.as_mut().unwrap();
            match mutation {
                0 => c.rows[0].raw.pop().map(|_| ()).unwrap(),
                1 => c.rows[0].raw[15] ^= 1,
                2 => c.rows[0].condition ^= 1,
                3 => c.rows[0].scope ^= 1,
                4 => c.rows[0].job ^= 1,
                5 => c.rows[0].object_type ^= 1,
                6 => c.rows[0].slot = 1,
                _ => { c.rows.pop(); },
            }
            assert!(career_delete_check(&invalid, 0).is_err(), "mutation {mutation}");
            let (index, _, _) = fixtures::data();
            assert!(prepare_snapshot(&index.session, &invalid, &RelationRequest::DeleteCareerPlan {slot: 0}, None, None, vec![]).is_err());
        }
        let mut duplicate = snapshot.clone();
        let c = duplicate.career.as_mut().unwrap();
        c.rows[1] = c.rows[0].clone();
        c.rows[1].slot = 1;
        duplicate.guards.iter_mut().find(|g| g.address == c.begin).unwrap().expected[8..16].copy_from_slice(&c.rows[0].record.to_le_bytes());
        assert!(career_delete_check(&duplicate, 0).is_err());
        let mut conflict = snapshot.clone();
        conflict.guards.push(Guard {address: career.rows[0].record + 15, expected: vec![0]});
        assert!(career_delete_check(&conflict, 0).is_err());
        assert!(career_delete_check(&snapshot, 3).is_err());
    }
    #[test]
    fn noneditable_career_delete_rejects_changed_live_owner_vector_or_record() {
        let snapshot = mixed_careers(&[25, 9, 5]);
        let career = snapshot.career.as_ref().unwrap();
        let plan = deletion_plan(&snapshot, RelationRequest::DeleteCareerPlan {slot: 1});
        for address in [snapshot.entity.address + 0xC8, career.holder + 8, career.begin, career.rows[1].record + 15] {
            let memory = mock_snapshot(&snapshot);
            let value = memory.bytes.borrow()[&address];
            memory.bytes.borrow_mut().insert(address, value ^ 1);
            assert!(validate_guards(&memory, &plan.guards).is_err());
            assert!(memory.writes.borrow().is_empty());
        }
    }
    #[test]
    fn career_delete_readd_and_both_undos_preserve_every_allocation() {
        let snapshot = three_careers();
        let memory = mock_snapshot(&snapshot);
        let original = memory.bytes.borrow().clone();
        let plan = deletion_plan(&snapshot, RelationRequest::DeleteCareerPlan { slot: 1 });
        write_batch(
            &memory,
            &plan.changes,
            &expected_guards(&plan.guards, &plan.changes),
        )
        .unwrap();
        validate_undo_ranges(&memory, &snapshot, &plan).unwrap();
        let mut checked = vec![];
        let mut current = career_snapshot(&memory, &snapshot.entity, &mut checked).unwrap();
        assert_eq!(
            current.rows.iter().map(|r| r.record).collect::<Vec<_>>(),
            vec![0x92000, 0x92200]
        );
        let (saved, guards) = reusable_deleted_career(&memory, &current, &plan).unwrap();
        assert_eq!(saved.record, 0x92100);
        current.reusable = Some(saved);
        assert_eq!(career_add_mode(&current), Some("deleted_record"));
        let mut post = snapshot.clone();
        post.career = Some(current);
        post.guards = guards;
        let (index, _, _) = fixtures::data();
        let club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let added = prepare_snapshot(
            &index.session,
            &post,
            &RelationRequest::AddCareerClub {
                club,
                condition: 2,
                job: 16,
            },
            Some(&target),
            None,
            vec![],
        )
        .unwrap();
        assert_eq!(added.changes.len(), 2);
        write_batch(
            &memory,
            &added.changes,
            &expected_guards(&added.guards, &added.changes),
        )
        .unwrap();
        validate_undo_ranges(&memory, &post, &added).unwrap();
        // An earlier delete cannot be undone while its reused record is active.
        assert!(validate_undo_ranges(&memory, &post, &plan).is_err());
        let reverse = reverse_plan(&added);
        write_batch(&memory, &reverse.changes, &added.guards).unwrap();
        validate_undo_ranges(&memory, &post, &plan).unwrap();
        let reverse = reverse_plan(&plan);
        write_batch(&memory, &reverse.changes, &plan.guards).unwrap();
        assert_eq!(*memory.bytes.borrow(), original);
    }
    #[test]
    fn deleted_career_reuse_rejects_changed_pointer_record_header_and_forged_receipt() {
        let snapshot = three_careers();
        let plan = deletion_plan(&snapshot, RelationRequest::DeleteCareerPlan { slot: 1 });
        for changed in [0x91010, 0x92100, 0x90008] {
            let memory = mock_snapshot(&snapshot);
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            let current = career_snapshot(&memory, &snapshot.entity, &mut vec![]).unwrap();
            let original = memory.bytes.borrow()[&changed];
            memory.bytes.borrow_mut().insert(changed, original ^ 0xFF);
            memory.writes.borrow_mut().clear();
            assert!(reusable_deleted_career(&memory, &current, &plan).is_err());
            assert!(memory.writes.borrow().is_empty());
        }
        let memory = mock_snapshot(&snapshot);
        write_batch(
            &memory,
            &plan.changes,
            &expected_guards(&plan.guards, &plan.changes),
        )
        .unwrap();
        let current = career_snapshot(&memory, &snapshot.entity, &mut vec![]).unwrap();
        let mut forged = plan.clone();
        forged.changes[0].address += 8;
        assert!(reusable_deleted_career(&memory, &current, &forged).is_err());
        assert!(validate_undo_ranges(&memory, &snapshot, &forged).is_err());
    }
    #[test]
    fn deletion_rolls_back_at_every_shift_chunk_and_end_publication() {
        let mut snapshot = three_careers();
        // Exercise multi-chunk compaction independently of the element type.
        for size in [8usize, 16] {
            let raw = (0..size * 12).map(|v| (v % 251) as u8).collect::<Vec<_>>();
            let (holder, begin) = (0xD0000u64, 0xD1000u64);
            let end = begin + raw.len() as u64;
            snapshot.guards = vec![
                Guard {
                    address: holder,
                    expected: [begin.to_le_bytes(), end.to_le_bytes(), end.to_le_bytes()].concat(),
                },
                Guard {
                    address: begin,
                    expected: raw.clone(),
                },
            ];
            let changes = delete_changes(holder, begin, end, size, 1, &raw).unwrap();
            assert!(changes.len() > 2);
            for failed in &changes {
                let memory = mock_snapshot(&snapshot);
                let before = memory.bytes.borrow().clone();
                memory.partial_failure.replace(Some(failed.address));
                assert!(
                    write_batch(
                        &memory,
                        &changes,
                        &expected_guards(&snapshot.guards, &changes)
                    )
                    .is_err()
                );
                assert_eq!(*memory.bytes.borrow(), before);
            }
        }
    }
    #[test]
    fn first_favourite_uses_verified_inline_template_and_requires_capacity() {
        let (index, _, snapshots) = fixtures::data();
        let snapshot = &snapshots[2];
        assert!(snapshot.rows.is_empty());
        let club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let request = RelationRequest::AddFavouriteClub {
            template_slot: None,
            club,
            reason: 1,
            level: 85,
        };
        let plan = prepare_snapshot(
            &index.session,
            snapshot,
            &request,
            Some(&target),
            None,
            vec![],
        )
        .unwrap();
        assert_eq!(
            &plan.changes[0].new_bytes[8..],
            &[1, 0, 1, 3, 85, 0, 0, 255]
        );
        let memory = mock_snapshot(snapshot);
        let before = memory.bytes.borrow().clone();
        write_batch(
            &memory,
            &plan.changes,
            &expected_guards(&plan.guards, &plan.changes),
        )
        .unwrap();
        let mut current = snapshot.clone();
        current.end += 16;
        validate_undo_ranges(&memory, &current, &plan).unwrap();
        let reverse = reverse_plan(&plan);
        write_batch(&memory, &reverse.changes, &plan.guards).unwrap();
        assert_eq!(*memory.bytes.borrow(), before);
        let mut full = snapshot.clone();
        full.spare_slots = 0;
        full.capacity = full.end;
        assert!(
            prepare_snapshot(&index.session, &full, &request, Some(&target), None, vec![]).is_err()
        );
    }
    #[test]
    fn first_secondary_nationality_uses_allocated_empty_vector() {
        let (index, _, snapshots) = fixtures::data();
        let snapshot = &snapshots[2];
        let donor = snapshots[0].entity.clone();
        let target = snapshots[0].primary_nationality.as_ref().unwrap();
        assert!(snapshot.rows.is_empty());
        let request = RelationRequest::AddSecondaryNationality {
            donor,
            expected_nation_uid: target.uid,
        };
        let plan = prepare_snapshot(
            &index.session, snapshot, &request, Some(target), None, vec![],
        ).unwrap();
        assert_eq!(&plan.changes[0].new_bytes[8..], &NATIONALITY_TAIL);
        let memory = mock_snapshot(snapshot);
        let original = memory.bytes.borrow().clone();
        write_batch(&memory, &plan.changes, &expected_guards(&plan.guards, &plan.changes)).unwrap();
        let mut current = snapshot.clone();
        current.end += 16;
        validate_undo_ranges(&memory, &current, &plan).unwrap();
        write_batch(&memory, &reverse_plan(&plan).changes, &plan.guards).unwrap();
        assert_eq!(*memory.bytes.borrow(), original);
    }
    #[test]
    fn native_relation_count_limit_applies_with_spare_capacity_and_growth_enabled() {
        let (_, _, snapshots) = fixtures::data();
        let mut snapshot = snapshots[0].clone();
        let row = snapshot.rows[0].clone();
        snapshot.rows = (0..MAX_RELATION_COUNT).map(|slot| { let mut row = row.clone(); row.slot = slot; row }).collect();
        snapshot.end = snapshot.begin + MAX_RELATION_COUNT as u64 * 16;
        snapshot.vector_growth_available = true;
        for spare_slots in [0, 1] {
            snapshot.capacity = snapshot.end + spare_slots as u64 * 16;
            snapshot.spare_slots = spare_slots;
            assert!(relation_add_unavailable_reason(&snapshot).unwrap().contains("255"));
        }
        snapshot.rows.pop();
        snapshot.end -= 16;
        snapshot.spare_slots = ((snapshot.capacity - snapshot.end) / 16) as usize;
        assert!(relation_add_unavailable_reason(&snapshot).is_none());
    }
    #[test]
    fn full_relation_vector_delete_reuses_last_slot_and_both_undos_restore_all_bytes() {
        let (index, _, snapshots) = fixtures::data();
        for slot in [0, 1] {
            let mut snapshot = snapshots[0].clone();
            snapshot.capacity = snapshot.end;
            snapshot.spare_slots = 0;
            snapshot.append_bytes = None;
            snapshot.guards.retain(|g| g.address != snapshot.end);
            let header = snapshot.guards.iter_mut().find(|g| g.address == snapshot.holder).unwrap();
            header.expected[16..24].copy_from_slice(&snapshot.capacity.to_le_bytes());
            assert!(relation_add_unavailable_reason(&snapshot).is_some());
            let memory = mock_snapshot(&snapshot);
            let original = memory.bytes.borrow().clone();
            let deleted = deletion_plan(&snapshot, if slot == 0 {
                RelationRequest::DeleteFavouriteClub { slot }
            } else {
                RelationRequest::DeleteSecondaryNationality { slot }
            });
            write_batch(&memory, &deleted.changes, &expected_guards(&deleted.guards, &deleted.changes)).unwrap();
            let mut current = snapshot.clone();
            current.end -= 16;
            current.spare_slots = 1;
            current.append_bytes = Some(snapshot.rows[slot].raw.clone());
            current.rows.remove(slot);
            current.rows[0].slot = 0;
            current.guards = expected_guards(&deleted.guards, &deleted.changes);
            assert!(relation_add_unavailable_reason(&current).is_none());
            let club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
            let target = if slot == 0 {
                LinkedRecord { uid: club.id, name: club.name.clone(), address: club.address, object_type: 1 }
            } else {
                snapshots[2].primary_nationality.clone().unwrap()
            };
            let request = if slot == 0 {
                RelationRequest::AddFavouriteClub { template_slot: None, club, reason: 1, level: 90 }
            } else {
                RelationRequest::AddSecondaryNationality { donor: snapshots[2].entity.clone(), expected_nation_uid: target.uid }
            };
            let added = prepare_snapshot(&index.session, &current, &request, Some(&target), None, vec![]).unwrap();
            let after_delete = memory.bytes.borrow().clone();
            // Both writing the reused slot and publishing the new length must roll back.
            for failed in &added.changes {
                let attempted = mock_snapshot(&current);
                attempted.partial_failure.replace(Some(failed.address));
                assert!(write_batch(&attempted, &added.changes, &expected_guards(&added.guards, &added.changes)).is_err());
                assert_eq!(*attempted.bytes.borrow(), after_delete);
            }
            write_batch(&memory, &added.changes, &expected_guards(&added.guards, &added.changes)).unwrap();
            let mut after_add = current.clone();
            after_add.end += 16;
            validate_undo_ranges(&memory, &after_add, &added).unwrap();
            assert!(validate_guards(&memory, &reverse_plan(&deleted).guards).is_err());
            write_batch(&memory, &reverse_plan(&added).changes, &added.guards).unwrap();
            validate_undo_ranges(&memory, &current, &deleted).unwrap();
            write_batch(&memory, &reverse_plan(&deleted).changes, &deleted.guards).unwrap();
            assert_eq!(*memory.bytes.borrow(), original);
        }
    }
    #[test]
    fn add_career_uses_only_owned_blank_or_guarded_deleted_record() {
        let (index, _, snapshots) = fixtures::data();
        let club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let request = RelationRequest::AddCareerClub {
            club,
            condition: 1,
            job: 16,
        };
        for i in [3, 4] {
            let snapshot = &snapshots[i];
            let plan = prepare_snapshot(
                &index.session,
                snapshot,
                &request,
                Some(&target),
                None,
                vec![],
            )
            .unwrap();
            let memory = mock_snapshot(snapshot);
            let before = memory.bytes.borrow().clone();
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes),
            )
            .unwrap();
            validate_undo_ranges(&memory, snapshot, &plan).unwrap();
            let reverse = reverse_plan(&plan);
            write_batch(&memory, &reverse.changes, &plan.guards).unwrap();
            assert_eq!(*memory.bytes.borrow(), before);
        }
        // Spare pointer capacity alone never grants ownership of a new record.
        assert!(
            prepare_snapshot(
                &index.session,
                &snapshots[0],
                &request,
                Some(&target),
                None,
                vec![]
            )
            .is_err()
        );
        assert!(
            prepare_snapshot(
                &index.session,
                &snapshots[2],
                &request,
                Some(&target),
                None,
                vec![]
            )
            .is_err()
        );
    }
    #[test]
    fn delete_rejects_wrong_types_and_out_of_bounds_without_mutating_snapshot() {
        let (_, _, snapshots) = fixtures::data();
        let snapshot = &snapshots[0];
        let before = serde_json::to_vec(snapshot).unwrap();
        for request in [
            RelationRequest::DeleteFavouriteClub { slot: 1 },
            RelationRequest::DeleteSecondaryNationality { slot: 0 },
            RelationRequest::DeleteCareerPlan { slot: 1 },
        ] {
            let (index, _, _) = fixtures::data();
            assert!(
                prepare_snapshot(&index.session, snapshot, &request, None, None, vec![]).is_err()
            );
        }
        assert_eq!(serde_json::to_vec(snapshot).unwrap(), before);
    }
    fn career_test_plan() -> (PersonRelations, RelationPlan) {
        let (index, _, snapshots) = fixtures::data();
        let snapshot = snapshots[0].clone();
        let club = index
            .entities
            .iter()
            .find(|e| e.kind == EntityKind::Club && e.id == 45)
            .unwrap()
            .clone();
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let plan = prepare_snapshot(
            &index.session,
            &snapshot,
            &RelationRequest::SetCareerClub {
                slot: 0,
                club,
                condition: 2,
                job: 16,
            },
            Some(&target),
            None,
            vec![],
        )
        .unwrap();
        (snapshot, plan)
    }
    #[test]
    fn career_change_preserves_unknown_tail_and_unrelated_relations_and_undo_bytes() {
        let (snapshot, plan) = career_test_plan();
        let memory = mock_snapshot(&snapshot);
        let original = memory.bytes.borrow().clone();
        let change = &plan.changes[0];
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(&change.new_bytes[8..12], &[2, 1, 16, 3]);
        assert_eq!(&change.new_bytes[12..], &change.old_bytes[12..]);
        assert!(plan.before.contains("선수"));
        assert!(plan.after.contains("감독"));
        validate_guards(&memory, &plan.guards).unwrap();
        let post = expected_guards(&plan.guards, &plan.changes);
        write_batch(&memory, &plan.changes, &post).unwrap();
        for (address, byte) in &original {
            if !(*address >= change.address && *address < change.address + 16) {
                assert_eq!(memory.bytes.borrow()[address], *byte);
            }
        }
        let reverse = Change {
            old_bytes: change.new_bytes.clone(),
            new_bytes: change.old_bytes.clone(),
            ..change.clone()
        };
        write_batch(&memory, &[reverse], &plan.guards).unwrap();
        assert_eq!(*memory.bytes.borrow(), original);
    }
    #[test]
    fn career_partial_write_rolls_back_and_stale_pointer_rejects_before_write() {
        let (snapshot, plan) = career_test_plan();
        let memory = mock_snapshot(&snapshot);
        memory
            .partial_failure
            .replace(Some(plan.changes[0].address));
        assert!(
            write_batch(
                &memory,
                &plan.changes,
                &expected_guards(&plan.guards, &plan.changes)
            )
            .is_err()
        );
        validate_guards(&memory, &plan.guards).unwrap();
        let stale = mock_snapshot(&snapshot);
        stale
            .bytes
            .borrow_mut()
            .insert(snapshot.entity.address + 0xC8, 0xFF);
        assert!(validate_guards(&stale, &plan.guards).is_err());
        assert!(stale.writes.borrow().is_empty());
    }
    #[test]
    fn career_pointer_vector_is_distinct_bounded_and_never_allocates() {
        let (index, _, snapshots) = fixtures::data();
        let snapshot = &snapshots[0];
        let memory = mock_snapshot(snapshot);
        let career = career_snapshot(&memory, &snapshot.entity, &mut vec![]).unwrap();
        assert_eq!(career.rows.len(), 1);
        assert_eq!(career.rows[0].record, 0x92000);
        assert_eq!(career.rows[0].raw[11], 3);
        assert_ne!(career.begin, snapshot.begin);
        for raw in [
            [0x10000u64, 0x10009, 0x10010],
            [0x10000, 0x10010, 0x10008],
            [0x10000, 0x10008, 0x15008],
        ]
        .map(|v| v.into_iter().flat_map(u64::to_le_bytes).collect::<Vec<_>>())
        {
            assert!(career_vector_bounds(&raw).is_err());
        }
        let mut empty = snapshot.clone();
        empty.career = Some(CareerPlans {
            rows: vec![],
            holder: 0,
            begin: 0,
            end: 0,
            capacity: 0,
            reusable: None,
        allocation_available: false,
        allocation_reason: None,
        });
        let club = index.entities.iter().find(|e| e.id == 45).unwrap().clone();
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        assert!(
            prepare_snapshot(
                &index.session,
                &empty,
                &RelationRequest::SetCareerClub {
                    slot: 0,
                    club,
                    condition: 1,
                    job: 1
                },
                Some(&target),
                None,
                vec![]
            )
            .is_err()
        );
        assert!(memory.writes.borrow().is_empty());
    }
    #[test]
    fn career_initializes_only_verified_existing_blank_record_and_rejects_unknown_values() {
        let (_, _, snapshots) = fixtures::data();
        let blank = &snapshots[3].career.as_ref().unwrap().rows[0].raw;
        let filled = career_record(blank, 0x61000, 1, 22).unwrap();
        assert_eq!(&filled[8..12], &[1, 1, 22, 3]);
        assert_eq!(&filled[12..], &blank[12..]);
        for (condition, job) in [(0, 1), (3, 1), (1, 255)] {
            assert!(career_record(blank, 0x61000, condition, job).is_err());
        }
        let mut unsupported = filled.clone();
        unsupported[11] = 42;
        assert!(career_record(&unsupported, 0x61000, 1, 16).is_err());
        unsupported = filled;
        unsupported[9] = 3;
        assert!(career_record(&unsupported, 0x61000, 1, 16).is_err());
        assert!(career_record(&blank[..12], 0x61000, 1, 16).is_err());
    }
    #[test]
    fn duplicate_career_and_malformed_offline_rows_are_rejected() {
        let (_, plan) = career_test_plan();
        let mut snapshot = mixed_careers(&[3, 3]);
        let club = match &plan.request {
            RelationRequest::SetCareerClub { club, .. } => club.clone(),
            _ => unreachable!(),
        };
        let target = LinkedRecord {
            uid: club.id,
            name: club.name.clone(),
            address: club.address,
            object_type: 1,
        };
        let duplicate = &mut snapshot.career.as_mut().unwrap().rows[1];
        duplicate.raw = plan.changes[0].new_bytes.clone();
        duplicate.condition = duplicate.raw[8];
        duplicate.scope = duplicate.raw[9];
        duplicate.job = duplicate.raw[10];
        snapshot.guards.iter_mut().find(|g|g.address==duplicate.record).unwrap().expected=duplicate.raw.clone();
        assert!(
            prepare_snapshot(
                &plan.session,
                &snapshot,
                &plan.request,
                Some(&target),
                None,
                vec![]
            )
            .unwrap_err()
            .to_string()
            .contains("이미")
        );
        snapshot.rows[0].raw.clear();
        assert!(
            prepare_snapshot(
                &plan.session,
                &snapshot,
                &plan.request,
                Some(&target),
                None,
                vec![]
            )
            .unwrap_err()
            .to_string()
            .contains("손상")
        );
    }
    #[test]
    #[ignore = "explicit read-only native fixture required"]
    fn readonly_native_probe() {
        let path =
            std::env::var("TOUCHLINE_RELATION_FIXTURE").expect("set a current Index JSON fixture");
        let index: Index = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        backend::validate_session(&index.session).unwrap();
        let selected: Vec<_> = index
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::Player && e.club_id == Some(1141))
            .take(6)
            .collect();
        assert!(!selected.is_empty());
        let mut results = vec![];
        for e in &selected {
            let view = inspect(&index.session, e).unwrap();
            results.push(serde_json::json!({"uid":e.id,"names":view.names.iter().map(|n|serde_json::json!({"field":n.field,"text":n.text})).collect::<Vec<_>>(),
              "primary":view.primary_nationality.as_ref().map(|n|serde_json::json!({"uid":n.uid,"name":n.name})),"relations":view.rows.len(),"spare_slots":view.spare_slots,
              "favourites":view.rows.iter().filter(|r|r.editable_favourite).map(|r|serde_json::json!({"slot":r.slot,"name":r.target.as_ref().unwrap().name,"level":r.level,"reason":r.reason})).collect::<Vec<_>>()}));
        }
        let e = selected[0];
        let donor = selected[1];
        let plan = prepare(
            &index.session,
            e,
            &RelationRequest::ReuseName {
                field: NameField::First,
                donor: donor.clone(),
            },
        )
        .unwrap();
        assert_eq!(plan.changes.len(), 1);
        let view = inspect(&index.session, e).unwrap();
        let favourite = view.rows.iter().find(|r| r.editable_favourite).unwrap();
        let club = index
            .entities
            .iter()
            .find(|c| {
                c.kind == EntityKind::Club && Some(c.id) == favourite.target.as_ref().map(|x| x.uid)
            })
            .unwrap();
        let level = if favourite.level == 100 { 99 } else { 100 };
        let favourite_plan = prepare(
            &index.session,
            e,
            &RelationRequest::SetFavouriteClub {
                slot: favourite.slot,
                club: club.clone(),
                reason: favourite.reason,
                level,
            },
        )
        .unwrap();
        assert_eq!(favourite_plan.changes.len(), 1);
        let donor_view = inspect(&index.session, donor).unwrap();
        let nation = donor_view.primary_nationality.unwrap();
        let nationality_preview = prepare(
            &index.session,
            e,
            &RelationRequest::AddSecondaryNationality {
                donor: donor.clone(),
                expected_nation_uid: nation.uid,
            },
        );
        let report = serde_json::json!({"read_only":true,"game_modified":false,"process_write_handle_opened":false,"pid":index.session.pid,
          "profile":index.session.profile,"records":results,"name_preview":{"before":plan.before,"after":plan.after},
          "favourite_preview":{"before":favourite_plan.before,"after":favourite_plan.after},
          "nationality_preview":match nationality_preview{Ok(p)=>serde_json::json!({"available":true,"before":p.before,"after":p.after,"changes":p.changes.len()}),Err(e)=>serde_json::json!({"available":false,"reason":format!("{e:#}")})}});
        std::fs::write(
            "output/relations-native-read.json",
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!(
            "{}",
            serde_json::json!({"records":selected.len(),"name_preview":true,"favourite_preview":true,"game_modified":false})
        );
    }
    struct Mock {
        bytes: RefCell<BTreeMap<u64, u8>>,
        partial_failure: RefCell<Option<u64>>,
        writes: RefCell<Vec<u64>>,
    }
    impl RelationMemory for Mock {
        fn read(&self, a: u64, n: usize) -> Result<Vec<u8>> {
            (0..n)
                .map(|i| {
                    self.bytes
                        .borrow()
                        .get(&(a + i as u64))
                        .copied()
                        .context("unmapped")
                })
                .collect()
        }
        fn write(&self, a: u64, b: &[u8]) -> Result<()> {
            self.writes.borrow_mut().push(a);
            let fail = *self.partial_failure.borrow() == Some(a);
            for (i, byte) in b.iter().take(if fail { 1 } else { b.len() }).enumerate() {
                self.bytes.borrow_mut().insert(a + i as u64, *byte);
            }
            if fail {
                self.partial_failure.replace(None);
                bail!("injected partial write");
            }
            Ok(())
        }
    }
    fn append_fixture(fail: Option<u64>) -> (Mock, Vec<Guard>, Vec<Change>) {
        let header = [
            0x30000u64.to_le_bytes(),
            0x30000u64.to_le_bytes(),
            0x30020u64.to_le_bytes(),
        ]
        .concat();
        let guards = vec![
            Guard {
                address: 0x20000,
                expected: header,
            },
            Guard {
                address: 0x30000,
                expected: vec![0; 16],
            },
        ];
        let mut bytes = BTreeMap::new();
        for g in &guards {
            for (i, b) in g.expected.iter().enumerate() {
                bytes.insert(g.address + i as u64, *b);
            }
        }
        let changes = vec![
            change("record", "record", 0x30000, vec![0; 16], vec![5; 16]).unwrap(),
            change(
                "end",
                "end",
                0x20008,
                0x30000u64.to_le_bytes().to_vec(),
                0x30010u64.to_le_bytes().to_vec(),
            )
            .unwrap(),
        ];
        (
            Mock {
                bytes: RefCell::new(bytes),
                partial_failure: RefCell::new(fail),
                writes: RefCell::new(vec![]),
            },
            guards,
            changes,
        )
    }
    #[test]
    fn append_publishes_record_before_length() {
        let (m, guards, changes) = append_fixture(None);
        validate_guards(&m, &guards).unwrap();
        write_batch(&m, &changes, &expected_guards(&guards, &changes)).unwrap();
        assert_eq!(*m.writes.borrow(), vec![0x30000, 0x20008]);
    }
    #[test]
    fn partial_record_and_publication_failures_restore_original_bytes() {
        for failure in [0x30000, 0x20008] {
            let (m, guards, changes) = append_fixture(Some(failure));
            assert!(write_batch(&m, &changes, &expected_guards(&guards, &changes)).is_err());
            validate_guards(&m, &guards).unwrap();
            if failure == 0x20008 {
                assert_eq!(*m.writes.borrow(), vec![0x30000, 0x20008, 0x20008, 0x30000]);
            }
        }
    }
    #[test]
    fn changed_preview_guard_rejects_before_any_write() {
        let (m, guards, _) = append_fixture(None);
        m.bytes.borrow_mut().insert(0x20000, 9);
        assert!(validate_guards(&m, &guards).is_err());
        assert!(m.writes.borrow().is_empty());
    }
    #[test]
    fn vector_rejects_invalid_alignment_capacity_and_limits() {
        let raw =
            |a: u64, b: u64, c: u64| [a.to_le_bytes(), b.to_le_bytes(), c.to_le_bytes()].concat();
        assert_eq!(
            vector_bounds(&raw(0x20000, 0x20020, 0x20040)).unwrap(),
            (0x20000, 0x20020, 0x20040)
        );
        // Native relation holder constructor RVA 0x333A5D0 initializes all
        // three pointers to zero; it is an empty vector, not corruption.
        assert_eq!(vector_bounds(&raw(0, 0, 0)).unwrap(), (0, 0, 0));
        for bytes in [
            raw(0, 0, 0x20000),
            raw(0x20000, 0x20021, 0x20040),
            raw(0x20000, 0x20040, 0x20020),
            raw(0x20000, 0x20020, 0x30000),
        ] {
            assert!(vector_bounds(&bytes).is_err());
        }
    }
    #[test]
    fn favourite_changes_only_selected_fields_preserving_unknown_flags() {
        let mut old = 0x20000u64.to_le_bytes().to_vec();
        old.extend([2, 0, 1, 3, 100, 78, 0, 255]);
        let new = favourite_record(&old, 0x30000, 1, 85).unwrap();
        assert_eq!(q(&new, 0), 0x30000);
        assert_eq!(&new[8..13], &[1, 0, 1, 3, 85]);
        assert_eq!(&new[13..], &old[13..]);
        assert!(favourite_record(&old, 0x30000, 5, 85).is_err());
        assert!(favourite_record(&old, 0x30000, 1, 101).is_err());
        old[11] = 9;
        assert!(favourite_record(&old, 0x30000, 1, 85).is_err());
    }
    #[test]
    fn post_guards_overlay_only_changed_bytes_and_support_reverse_order() {
        let original = vec![
            Guard {
                address: 0x20000,
                expected: vec![1, 2, 3, 4, 5, 6],
            },
            Guard {
                address: 0x30000,
                expected: vec![8, 9],
            },
        ];
        let c = change("x", "x", 0x20002, vec![3, 4], vec![30, 40]).unwrap();
        let updated = expected_guards(&original, &[c]);
        assert_eq!(updated[0].expected, vec![1, 2, 30, 40, 5, 6]);
        assert_eq!(updated[1], original[1]);
    }
    #[test]
    fn receipt_id_cannot_escape_history_directory() {
        for id in ["../test", "C:\\test", "a", "", &"1".repeat(100)] {
            assert!(receipt_path(id, false).is_err());
        }
    }
    #[test]
    fn unchanged_and_oversized_changes_are_rejected() {
        assert!(change("x", "x", 0x20000, vec![1], vec![1]).is_err());
        assert!(change("x", "x", 0x20000, vec![1; 65], vec![2; 65]).is_err());
        assert!(change("x", "x", 0x20000, vec![1], vec![2, 3]).is_err());
    }
}
