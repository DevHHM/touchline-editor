//! Local scenario generator and guarded, atomic sponsorship / club / city edits.
//! Company names and generated scenario articles
//! are companion content, never injected into unknown FM news/string structures.
use crate::{backend, memory::Process, model::*, transaction};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const DATE_RVA: u64 = 0x4df3c18;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventDraft {
    pub title: String,
    pub sponsor_name: String,
    pub city_project: String,
    /// Native FM money is GBP. Zero means no sponsorship replacement.
    pub annual_gbp: i64,
    pub years: u16,
    /// Optional user-provided display conversion, never an inferred save rate.
    pub krw_per_gbp: Option<f64>,
    /// Existing record selected explicitly by the user. No new allocation.
    pub sponsor_slot: Option<usize>,
    /// Grants are independent of sponsorship and are never auto-added.
    pub cash_grant_gbp: i64,
    pub transfer_budget_grant_gbp: i64,
    pub weekly_wage_increase_gbp: i64,
    pub population_band: Option<u8>,
    pub attraction: Option<u8>,
}
impl Default for EventDraft {
    fn default() -> Self {
        Self {
            title: "새로운 10년을 여는 파트너십".into(),
            sponsor_name: "SAMSUNG".into(),
            city_project: "지역 연구개발 거점 조성".into(),
            annual_gbp: 26_653_522,
            years: 10,
            krw_per_gbp: None,
            sponsor_slot: None,
            cash_grant_gbp: 0,
            transfer_budget_grant_gbp: 0,
            weekly_wage_increase_gbp: 0,
            population_band: None,
            attraction: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SponsorRecord {
    pub slot: usize,
    pub kind: u8,
    pub annual_gbp: i32,
    pub total_gbp: i32,
    pub start: String,
    pub end: String,
    pub address: u64,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventContext {
    pub session: Session,
    pub club: Detail,
    pub city: Option<Detail>,
    pub sponsors: Vec<SponsorRecord>,
    pub game_date: String,
    pub warnings: Vec<String>,
    /// Includes current game date and bounded sponsor routing.
    pub guards: Vec<Guard>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventDelta {
    pub section: String,
    pub label: String,
    pub before: String,
    pub after: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventPlan {
    pub id: String,
    pub draft: EventDraft,
    pub context: EventContext,
    pub deltas: Vec<EventDelta>,
    pub narrative: String,
    pub warnings: Vec<String>,
    pub changes: Vec<Change>,
    pub guards: Vec<Guard>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventReceipt {
    pub id: String,
    pub time: String,
    pub status: String,
    pub plan: EventPlan,
    pub game_saved: bool,
    pub engine_accrual_verified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Date {
    year: u16,
    month: u8,
    day: u8,
}
impl Date {
    fn leap(year: u16) -> bool {
        year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
    }
    fn days(year: u16, month: u8) -> u8 {
        match month {
            2 => {
                if Self::leap(year) {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        }
    }
    fn new(year: u16, month: u8, day: u8) -> Result<Self> {
        ensure!(
            (1900..=2200).contains(&year) && day > 0 && day <= Self::days(year, month),
            "날짜 범위 오류"
        );
        Ok(Self { year, month, day })
    }
    fn parse(s: &str) -> Result<Self> {
        let p: Vec<_> = s.split('-').collect();
        ensure!(p.len() == 3, "날짜 형식은 YYYY-MM-DD입니다");
        Self::new(p[0].parse()?, p[1].parse()?, p[2].parse()?)
    }
    fn unpack(raw: u32) -> Result<Self> {
        let year = (raw >> 16) as u16;
        let mut day = (raw & 511) as u16;
        ensure!(
            (1900..=2200).contains(&year)
                && day >= 1
                && day <= if Self::leap(year) { 366 } else { 365 },
            "FM 날짜 인코딩 오류"
        );
        for month in 1..=12 {
            let n = Self::days(year, month) as u16;
            if day <= n {
                return Self::new(year, month, day as u8);
            }
            day -= n;
        }
        bail!("FM 날짜 오류")
    }
    fn packed(self) -> u32 {
        ((self.year as u32) << 16)
            | ((1..self.month)
                .map(|m| Self::days(self.year, m) as u32)
                .sum::<u32>()
                + self.day as u32)
    }
    fn anniversary(self, years: u16) -> Result<Self> {
        let year = self
            .year
            .checked_add(years)
            .context("계약 연도 범위 초과")?;
        Self::new(year, self.month, self.day.min(Self::days(year, self.month)))
    }
    fn text(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}
fn q(raw: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}
fn d(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn pointer(p: u64) -> bool {
    (0x10000..0x8000_0000_0000).contains(&p)
}
fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
fn new_id() -> String {
    format!(
        "{}-{}-{}",
        now(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
fn capture_guard(p: &Process, guards: &mut Vec<Guard>, at: u64, n: usize) -> Result<Vec<u8>> {
    let raw = p.read(at, n)?;
    for (i, chunk) in raw.chunks(256).enumerate() {
        guards.push(Guard {
            address: at + (i * 256) as u64,
            expected: chunk.to_vec(),
        });
    }
    Ok(raw)
}

/// Read-only, follows the same verified home-team/stadium/city route as extras.
fn capture_city(p: &Process, session: &Session, club: &Entity) -> Result<Detail> {
    let head = p.read(club.address + 0x18, 16)?;
    let (begin, end) = (q(&head, 0), q(&head, 8));
    ensure!(
        pointer(begin) && end > begin && end - begin <= 512 && (end - begin) % 8 == 0,
        "구단 팀 목록 범위 오류"
    );
    let slots = p.read(begin, (end - begin) as usize)?;
    let mut first = None;
    for slot in slots.chunks_exact(8) {
        let team = q(slot, 0);
        if !pointer(team) {
            continue;
        }
        if p.u8(team + 0x28)? == 0 && p.u64(team + 0x30)? == club.address {
            ensure!(first.is_none(), "1군 팀 연결 중복");
            first = Some(team);
        }
    }
    let team = first.context("1군 연결 없음")?;
    let stadium = p.u64(team + 0x78)?;
    ensure!(pointer(stadium), "홈 구장 없음");
    let city = p.u64(stadium + 0x60)?;
    ensure!(pointer(city), "도시 연결 없음");
    let raw = p.read(city, 0x20)?;
    let name_ptr = q(&raw, 0x18);
    ensure!(pointer(name_ptr), "도시 이름 없음");
    let name_raw = p.read(name_ptr + 4, 256)?;
    let end = name_raw
        .iter()
        .position(|v| *v == 0)
        .context("도시 이름 길이 오류")?;
    let name = std::str::from_utf8(&name_raw[..end])?.to_owned();
    let entity = Entity {
        nationality: String::new(),
        nationality_id: None,
        football_category: crate::model::FootballCategory::Unknown,
        id: d(&raw, 12),
        kind: EntityKind::City,
        name: name.clone(),
        english_name: name,
        club: club.name.clone(),
        club_id: Some(club.id),
        age: None,
        ca: None,
        pa: None,
        role: "홈 구장 소재지".into(),
        group: "도시".into(),
        address: city,
        base: club.address,
        index: Some(d(&raw, 8)),
    };
    backend::detail(session, &entity)
}
pub fn capture(session: &Session, club: &Entity) -> Result<EventContext> {
    crate::compatibility::require_legacy_feature(session, "후원·구단 이벤트")?;
    ensure!(
        club.kind == EntityKind::Club,
        "이벤트 대상 구단을 선택하세요"
    );
    backend::validate_session(session)?;
    let club_detail = backend::detail(session, club)?;
    let p = Process::open(session.pid, false)?;
    let mut guards = vec![];
    let date_raw = capture_guard(&p, &mut guards, session.module_base + DATE_RVA, 4)?;
    let game_date = Date::unpack(d(&date_raw, 0))?.text();
    let mut warnings = vec![];
    let city = match capture_city(&p, session, club) {
        Ok(x) => Some(x),
        Err(e) => {
            warnings.push(format!("도시 항목을 읽을 수 없습니다: {e}"));
            None
        }
    };
    let mut sponsors = vec![];
    let sponsor_result = (|| -> Result<()> {
        let wrapper_raw = capture_guard(&p, &mut guards, club.address + 0x130, 8)?;
        let wrapper = q(&wrapper_raw, 0);
        ensure!(pointer(wrapper), "기존 스폰서 목록 없음");
        let vector = capture_guard(&p, &mut guards, wrapper, 24)?;
        let (begin, end, capacity) = (q(&vector, 0), q(&vector, 8), q(&vector, 16));
        ensure!(
            pointer(begin)
                && begin <= end
                && end <= capacity
                && end - begin <= 2048
                && (end - begin) % 8 == 0,
            "스폰서 목록 범위 오류"
        );
        let slots = capture_guard(&p, &mut guards, begin, (end - begin) as usize)?;
        let mut seen = HashSet::new();
        for (slot, chunk) in slots.chunks_exact(8).enumerate() {
            let address = q(chunk, 0);
            ensure!(
                pointer(address) && seen.insert(address),
                "스폰서 레코드 주소 오류/중복"
            );
            let raw = p.read(address, 24)?;
            let parsed = (|| -> Result<SponsorRecord> {
                let start = Date::unpack(d(&raw, 0))?;
                let end = Date::unpack(d(&raw, 4))?;
                let total_gbp = i32::from_le_bytes(raw[8..12].try_into().unwrap());
                let annual_gbp = i32::from_le_bytes(raw[12..16].try_into().unwrap());
                ensure!(
                    start < end && annual_gbp >= 0 && total_gbp >= annual_gbp,
                    "스폰서 금액/기간 검증 실패"
                );
                Ok(SponsorRecord {
                    slot,
                    kind: raw[20],
                    annual_gbp,
                    total_gbp,
                    start: start.text(),
                    end: end.text(),
                    address,
                    raw,
                })
            })();
            match parsed {
                Ok(record) => sponsors.push(record),
                Err(e) => warnings.push(format!("스폰서 계약 {} 제외: {e}", slot + 1)),
            }
        }
        Ok(())
    })();
    if let Err(e) = sponsor_result {
        sponsors.clear();
        warnings.push(format!("후원 계약 변경 비활성: {e}"));
    }
    backend::validate_session(session)?;
    ensure!(
        p.read(session.module_base + DATE_RVA, 4)? == date_raw,
        "게임 날짜가 바뀌었습니다. 다시 불러오세요"
    );
    Ok(EventContext {
        session: session.clone(),
        club: club_detail,
        city,
        sponsors,
        game_date,
        warnings,
        guards,
    })
}

fn validate_draft(draft: &EventDraft) -> Result<()> {
    for (name, text) in [
        ("제목", &draft.title),
        ("스폰서", &draft.sponsor_name),
        ("지역 사업", &draft.city_project),
    ] {
        ensure!(
            text.chars().count() <= 200 && !text.chars().any(|c| c.is_control()),
            "{name}: 200자 이내로 입력하세요"
        );
    }
    ensure!(!draft.title.trim().is_empty(), "이벤트 제목을 입력하세요");
    ensure!(
        (1..=30).contains(&draft.years),
        "계약은 1–30년으로 입력하세요"
    );
    for (label, money) in [
        ("연간 후원금", draft.annual_gbp),
        ("현금 지급", draft.cash_grant_gbp),
        ("이적 예산 증가", draft.transfer_budget_grant_gbp),
        ("주급 예산 증가", draft.weekly_wage_increase_gbp),
    ] {
        ensure!(
            (0..=2_000_000_000).contains(&money),
            "{label}은 £0–2,000,000,000 범위입니다"
        );
    }
    if draft.annual_gbp > 0 {
        ensure!(
            !draft.sponsor_name.trim().is_empty(),
            "스폰서 이름을 입력하세요"
        );
        ensure!(
            draft.sponsor_slot.is_some(),
            "교체할 기존 후원 계약을 선택하세요"
        );
    }
    if let Some(rate) = draft.krw_per_gbp {
        ensure!(
            rate.is_finite() && rate > 0. && rate <= 1_000_000.,
            "환율은 양수이며 1,000,000 이하로 입력하세요"
        );
    }
    if let Some(band) = draft.population_band {
        ensure!(band <= 15, "도시 인구 등급은 0–15입니다");
    }
    if let Some(value) = draft.attraction {
        ensure!(value <= 20, "도시 매력도는 0–20입니다");
    }
    Ok(())
}
fn field<'a>(detail: &'a Detail, key: &str) -> Result<&'a Field> {
    detail
        .fields
        .iter()
        .find(|f| f.key == key)
        .with_context(|| format!("{} 항목을 읽을 수 없습니다", key))
}
fn scalar_change(
    detail: &Detail,
    key: &str,
    value: f64,
    changes: &mut Vec<Change>,
    deltas: &mut Vec<EventDelta>,
) -> Result<()> {
    let f = field(detail, key)?;
    let bytes = transaction::encode_value(f, value)?;
    let b = f.binding.as_ref().context("편집할 수 없는 항목")?;
    if bytes == b.original {
        return Ok(());
    }
    let prepared = transaction::prepare(
        detail,
        &[Edit {
            key: key.into(),
            value,
        }],
    )?;
    changes.extend(prepared);
    deltas.push(EventDelta {
        section: f.section.clone(),
        label: f.label.clone(),
        before: format_number(f.value),
        after: format_number(value),
    });
    Ok(())
}
fn grant(
    detail: &Detail,
    key: &str,
    amount: i64,
    changes: &mut Vec<Change>,
    deltas: &mut Vec<EventDelta>,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let f = field(detail, key)?;
    scalar_change(detail, key, f.value + amount as f64, changes, deltas)
}
fn format_number(value: f64) -> String {
    let s = format!("{:.0}", value.abs());
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0. { format!("−{out}") } else { out }
}
fn sponsor_changes(
    s: &SponsorRecord,
    start: Date,
    end: Date,
    annual: i64,
    total: i64,
) -> Vec<Change> {
    [
        // The low word has a 9-bit ordinal and other, uninterpreted flags.
        // Replace the calendar date while preserving each record's flags.
        (
            "start",
            "계약 시작일",
            0,
            start.packed() | (d(&s.raw, 0) & 0xfe00),
        ),
        (
            "end",
            "계약 종료일",
            4,
            end.packed() | (d(&s.raw, 4) & 0xfe00),
        ),
        ("total", "계약 총액 (£)", 8, total as u32),
        ("annual", "연간 후원금 (£)", 12, annual as u32),
    ]
    .into_iter()
    .filter_map(|(key, label, offset, raw)| {
        let old = s.raw[offset..offset + 4].to_vec();
        let new = raw.to_le_bytes().to_vec();
        (old != new).then(|| Change {
            key: format!("sponsor.{}.{key}", s.slot),
            label: label.into(),
            before: d(&s.raw, offset) as f64,
            after: raw as f64,
            address: s.address + offset as u64,
            old_bytes: old,
            new_bytes: new,
        })
    })
    .collect()
}
pub fn plan(context: &EventContext, draft: &EventDraft) -> Result<EventPlan> {
    validate_draft(draft)?;
    let today = Date::parse(&context.game_date)?;
    let mut changes = vec![];
    let mut deltas = vec![];
    let mut guards = context.club.guards.clone();
    guards.extend(context.guards.clone());
    let mut warnings = context.warnings.clone();
    let mut narrative = format!(
        "{}\n\n{} · {}\n\n",
        draft.title, context.club.entity.name, context.game_date
    );
    if draft.annual_gbp > 0 {
        let slot = draft.sponsor_slot.context("후원 계약을 선택하세요")?;
        let sponsor = context
            .sponsors
            .iter()
            .find(|s| s.slot == slot)
            .context("선택한 기존 후원 계약이 없습니다")?;
        ensure!(sponsor.raw.len() == 24, "후원 계약 원본 크기 오류");
        let total = draft
            .annual_gbp
            .checked_mul(draft.years as i64)
            .context("후원 총액 범위 초과")?;
        ensure!(
            total <= i32::MAX as i64,
            "후원 총액이 FM 저장 한도 £2,147,483,647를 초과합니다"
        );
        let end = today.anniversary(draft.years)?;
        changes.extend(sponsor_changes(
            sponsor,
            today,
            end,
            draft.annual_gbp,
            total,
        ));
        guards.push(Guard {
            address: sponsor.address,
            expected: sponsor.raw.clone(),
        });
        deltas.extend([
            EventDelta {
                section: "스폰서".into(),
                label: format!("기존 후원 계약 {}", slot + 1),
                before: format!("{} — {}", sponsor.start, sponsor.end),
                after: format!("{} — {}", today.text(), end.text()),
            },
            EventDelta {
                section: "스폰서".into(),
                label: "연간 후원금 (£)".into(),
                before: format_number(sponsor.annual_gbp as f64),
                after: format_number(draft.annual_gbp as f64),
            },
            EventDelta {
                section: "스폰서".into(),
                label: "후원 총액 (£)".into(),
                before: format_number(sponsor.total_gbp as f64),
                after: format_number(total as f64),
            },
        ]);
        narrative.push_str(&format!("{}가 {}와 {}년 파트너십을 맺는 가상 시나리오입니다. 연간 £{}, 계약 총액 £{}의 후원으로 구단의 다음 단계를 뒷받침합니다.\n\n",context.club.entity.name,draft.sponsor_name,draft.years,format_number(draft.annual_gbp as f64),format_number(total as f64)));
        if let Some(rate) = draft.krw_per_gbp {
            narrative.push_str(&format!("입력한 환율 £1 = ₩{} 기준으로 연간 약 {}억 원입니다. 이 환율은 현재 세이브에서 읽은 값이 아닙니다.\n\n",format_number(rate),format_number(draft.annual_gbp as f64*rate/100_000_000.)));
        }
        warnings.push("기존 후원 계약 한 건의 금액·기간을 교체합니다. 계약 유형과 플래그는 유지하며 회사명·새 계약·FM 뉴스는 추가하지 않습니다.".into());
        warnings.push("후원 총액을 현금으로 지급하지 않습니다. 게임 진행 후의 후원 수익 반영은 별도로 확인해야 합니다.".into());
    }
    grant(
        &context.club,
        "club.cash",
        draft.cash_grant_gbp,
        &mut changes,
        &mut deltas,
    )?;
    // Both budget scalars track the same grant; cash is never implicitly altered.
    grant(
        &context.club,
        "club.transfer_base",
        draft.transfer_budget_grant_gbp,
        &mut changes,
        &mut deltas,
    )?;
    grant(
        &context.club,
        "club.transfer_current",
        draft.transfer_budget_grant_gbp,
        &mut changes,
        &mut deltas,
    )?;
    grant(
        &context.club,
        "club.weekly_wage",
        draft.weekly_wage_increase_gbp,
        &mut changes,
        &mut deltas,
    )?;
    if draft.cash_grant_gbp > 0
        || draft.transfer_budget_grant_gbp > 0
        || draft.weekly_wage_increase_gbp > 0
    {
        narrative.push_str(&format!("별도 지원: 구단 현금 +£{}, 이적 예산 +£{}, 주급 예산 +£{}/주. 이 금액은 후원 계약 총액과 독립적으로 지정한 추가분입니다.\n\n",format_number(draft.cash_grant_gbp as f64),format_number(draft.transfer_budget_grant_gbp as f64),format_number(draft.weekly_wage_increase_gbp as f64)));
    }
    if draft.population_band.is_some() || draft.attraction.is_some() {
        let city = context
            .city
            .as_ref()
            .context("도시 연결을 확인하지 못했습니다")?;
        guards.extend(city.guards.clone());
        if let Some(v) = draft.population_band {
            scalar_change(
                city,
                "city.population_band",
                v as f64,
                &mut changes,
                &mut deltas,
            )?;
        }
        if let Some(v) = draft.attraction {
            scalar_change(city, "city.attraction", v as f64, &mut changes, &mut deltas)?;
        }
        narrative.push_str(&format!("{}: {}. 연구·고용·정주 환경 개선을 배경으로 도시의 초기 성장 단계를 설정합니다. 인구 등급과 매력도만 반영하며 신규 시설 건설이나 이후 자동 성장은 포함하지 않습니다.\n\n",city.entity.name,draft.city_project));
        warnings.push("인구는 정확한 주민 수가 아닌 FM 인구 구간입니다. 도시 부유도는 변경하지 않으며 같은 도시의 다른 구단에도 영향을 줄 수 있습니다.".into());
    }
    narrative.push_str("FM 세이브용 가상 이벤트입니다. 이 기사와 기업명은 Touchline Editor의 로컬 기록으로 저장됩니다. 실제 기업 발표 또는 게임 내부 뉴스가 아닙니다.");
    ensure!(
        !changes.is_empty(),
        "현재 값과 같은 설정입니다. 적용할 변화가 없습니다"
    );
    validate_change_layout(&changes)?;
    changes.sort_by_key(|c| c.address);
    guards.retain(|g| !g.expected.is_empty());
    warnings.push("적용은 현재 실행 중인 세이브의 메모리에 반영됩니다. 게임 저장이나 날짜 진행은 자동으로 하지 않습니다.".into());
    Ok(EventPlan {
        id: new_id(),
        draft: draft.clone(),
        context: context.clone(),
        deltas,
        narrative,
        warnings,
        changes,
        guards,
    })
}
fn validate_change_layout(changes: &[Change]) -> Result<()> {
    ensure!(
        !changes.is_empty() && changes.len() <= 32,
        "이벤트 변경 개수 오류"
    );
    let mut keys = HashSet::new();
    let mut sorted: Vec<_> = changes.iter().collect();
    sorted.sort_by_key(|c| c.address);
    for c in &sorted {
        ensure!(
            keys.insert(&c.key)
                && pointer(c.address)
                && !c.old_bytes.is_empty()
                && c.old_bytes.len() <= 16
                && c.old_bytes.len() == c.new_bytes.len(),
            "이벤트 변경 항목 오류"
        );
    }
    for pair in sorted.windows(2) {
        ensure!(
            pair[0].address + pair[0].old_bytes.len() as u64 <= pair[1].address,
            "이벤트 변경 주소가 겹칩니다"
        );
    }
    Ok(())
}

fn folder(name: &str) -> Result<PathBuf> {
    let p = transaction::data_dir().join("events").join(name);
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() < 100 && id.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "이벤트 ID 형식 오류"
    );
    Ok(())
}
fn durable_json(path: &Path, value: &impl Serialize, create_only: bool) -> Result<()> {
    if create_only {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        f.write_all(&serde_json::to_vec_pretty(value)?)?;
        f.sync_all()?;
        return Ok(());
    }
    let tmp = path.with_extension("pending");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    drop(f);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
        }
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(tmp, path)?;
    Ok(())
}
pub fn save_draft(plan: &EventPlan) -> Result<PathBuf> {
    valid_id(&plan.id)?;
    let path = folder("drafts")?.join(format!("{}.json", plan.id));
    durable_json(&path, plan, false)?;
    let article = path.with_extension("txt");
    std::fs::write(article, &plan.narrative)?;
    Ok(path)
}
/// Saved scenario snapshots. Re-capture and re-plan before applying an old draft.
pub fn drafts() -> Vec<EventPlan> {
    let Ok(dir) = folder("drafts") else {
        return vec![];
    };
    let mut rows: Vec<EventPlan> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    rows.sort_by(|a, b| b.id.cmp(&a.id));
    rows.truncate(200);
    rows
}
pub fn load_draft(id: &str) -> Result<EventPlan> {
    valid_id(id)?;
    let path = folder("drafts")?.join(format!("{id}.json"));
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn history() -> Vec<EventReceipt> {
    let Ok(dir) = folder("history") else {
        return vec![];
    };
    let mut rows: Vec<EventReceipt> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    rows.sort_by(|a, b| b.time.cmp(&a.time));
    rows.truncate(200);
    rows
}

trait EventMemory {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>>;
    fn write(&self, address: u64, data: &[u8]) -> Result<()>;
}
impl EventMemory for Process {
    fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        Process::read(self, address, size)
    }
    fn write(&self, address: u64, data: &[u8]) -> Result<()> {
        Process::write(self, address, data)
    }
}
fn validate_current(memory: &impl EventMemory, guards: &[Guard], changes: &[Change]) -> Result<()> {
    ensure!(!guards.is_empty(), "대상 확인 정보가 없습니다");
    for g in guards {
        ensure!(
            pointer(g.address) && !g.expected.is_empty() && g.expected.len() <= 256,
            "검증 범위 오류"
        );
        ensure!(
            memory.read(g.address, g.expected.len())? == g.expected,
            "게임 데이터가 바뀌었습니다. 이벤트를 다시 미리보기 하세요"
        );
    }
    for c in changes {
        ensure!(
            memory.read(c.address, c.old_bytes.len())? == c.old_bytes,
            "{} 값이 바뀌었습니다. 이벤트를 다시 미리보기 하세요",
            c.label
        );
    }
    Ok(())
}
/// Caller suspends the target for validation, writes and any rollback.
fn write_atomic(memory: &impl EventMemory, changes: &[Change]) -> Result<()> {
    for (i, c) in changes.iter().enumerate() {
        let result = memory.write(c.address, &c.new_bytes).and_then(|()| {
            ensure!(
                memory.read(c.address, c.new_bytes.len())? == c.new_bytes,
                "{} 읽기 검증 실패",
                c.label
            );
            Ok(())
        });
        if let Err(error) = result {
            let mut failed = vec![];
            for old in changes[..=i].iter().rev() {
                if let Err(e) = memory.write(old.address, &old.old_bytes).and_then(|()| {
                    ensure!(
                        memory.read(old.address, old.old_bytes.len())? == old.old_bytes,
                        "복구 검증 실패"
                    );
                    Ok(())
                }) {
                    failed.push(format!("{}: {e}", old.label));
                }
            }
            if failed.is_empty() {
                bail!("이벤트 적용 실패; 변경한 모든 항목을 복원했습니다: {error:#}");
            }
            bail!(
                "이벤트 적용 실패 및 일부 복구 실패: {error:#}; {}",
                failed.join("; ")
            );
        }
    }
    Ok(())
}
fn same_changes(a: &[Change], b: &[Change]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.key == b.key
                && a.address == b.address
                && a.old_bytes == b.old_bytes
                && a.new_bytes == b.new_bytes
        })
}

fn execute(plan: EventPlan, success_status: &str) -> Result<EventReceipt> {
    crate::compatibility::require_legacy_feature(&plan.context.session, "후원·구단 이벤트")?;
    valid_id(&plan.id)?;
    validate_change_layout(&plan.changes)?;
    let session = &plan.context.session;
    backend::validate_session(session)?;
    let path = folder("history")?.join(format!("{}.json", plan.id));
    let mut receipt = EventReceipt {
        id: plan.id.clone(),
        time: now(),
        status: "prepared".into(),
        plan,
        game_saved: false,
        engine_accrual_verified: false,
    };
    // create_new both reserves this plan ID and prevents repeated cash grants.
    durable_json(&path, &receipt, true).context(
        "원본 백업을 만들 수 없거나 이미 처리한 이벤트입니다. 게임은 변경하지 않았습니다",
    )?;
    let mut resume_warning = None;
    let result = (|| -> Result<()> {
        let session = &receipt.plan.context.session;
        let process = Process::open(session.pid, true)?;
        ensure!(
            process.created()? == session.created,
            "FM 프로세스가 교체되었습니다"
        );
        let suspended = process.suspend()?;
        let result = (|| -> Result<()> {
            backend::validate_session(session)?;
            validate_current(&process, &receipt.plan.guards, &receipt.plan.changes)?;
            write_atomic(&process, &receipt.plan.changes)
        })();
        if let Err(e) = suspended.resume() {
            resume_warning = Some(format!("FM 재개 확인 실패: {e:#}"));
        }
        result
    })();
    match result {
        Ok(()) => {
            receipt.status = resume_warning
                .map(|w| format!("{success_status}_resume_warning: {w}"))
                .unwrap_or_else(|| success_status.into());
            if let Err(e) = durable_json(&path, &receipt, false) {
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
            let _ = durable_json(&path, &receipt, false);
            Err(e)
        }
    }
}
pub fn apply(preview: &EventPlan) -> Result<EventReceipt> {
    valid_id(&preview.id)?;
    let fresh = capture(&preview.context.session, &preview.context.club.entity)?;
    let mut checked = plan(&fresh, &preview.draft)?;
    ensure!(
        checked.guards == preview.guards && same_changes(&checked.changes, &preview.changes),
        "미리보기 이후 게임 정보가 바뀌었습니다. 다시 미리보기 하세요"
    );
    checked.id = preview.id.clone();
    execute(checked, "applied")
}

fn reverse_for_context(receipt: &EventReceipt, context: &EventContext) -> Result<EventPlan> {
    ensure!(
        receipt.plan.context.club.entity.id == context.club.entity.id
            && receipt.plan.context.club.entity.address == context.club.entity.address,
        "원본 이벤트와 다른 구단입니다"
    );
    let mut reversed = receipt.plan.clone();
    reversed.id = new_id();
    reversed.context = context.clone();
    reversed.guards = context.club.guards.clone();
    reversed.guards.extend(context.guards.clone());
    if let Some(city) = &context.city {
        reversed.guards.extend(city.guards.clone());
    }
    let mut changes = vec![];
    let mut sponsor_slots = HashSet::new();
    for old in &receipt.plan.changes {
        let (address, current, bytes) = if old.key.starts_with("sponsor.") {
            let parts: Vec<_> = old.key.split('.').collect();
            ensure!(parts.len() == 3, "후원 기록 키 오류");
            let slot: usize = parts[1].parse()?;
            let record = context
                .sponsors
                .iter()
                .find(|s| s.slot == slot)
                .context("기존 후원 기록을 찾을 수 없습니다")?;
            let original = receipt
                .plan
                .context
                .sponsors
                .iter()
                .find(|s| s.slot == slot)
                .context("원본 후원 계약 정보가 없습니다")?;
            ensure!(
                record.raw.len() == 24
                    && original.raw.len() == 24
                    && record.address == original.address
                    && record.raw[16..] == original.raw[16..],
                "후원 계약 유형이나 원본 레코드가 변경되었습니다"
            );
            let offset = match parts[2] {
                "start" => 0,
                "end" => 4,
                "total" => 8,
                "annual" => 12,
                _ => bail!("지원하지 않는 후원 필드"),
            };
            ensure!(
                old.before.is_finite()
                    && old.before >= 0.
                    && old.before <= i32::MAX as f64
                    && old.before.fract() == 0.,
                "복구 값 오류"
            );
            let target = (old.before as u32).to_le_bytes().to_vec();
            if offset < 8 {
                Date::unpack(old.before as u32)?;
            }
            if sponsor_slots.insert(slot) {
                reversed.guards.push(Guard {
                    address: record.address,
                    expected: record.raw.clone(),
                });
            }
            (
                record.address + offset as u64,
                record.raw[offset..offset + 4].to_vec(),
                target,
            )
        } else {
            let detail = if old.key.starts_with("city.") {
                let city = context.city.as_ref().context("도시 연결 없음")?;
                let original = receipt
                    .plan
                    .context
                    .city
                    .as_ref()
                    .context("원본 도시 없음")?;
                ensure!(
                    city.entity.id == original.entity.id
                        && city.entity.address == original.entity.address,
                    "원본 이벤트와 다른 도시입니다"
                );
                city
            } else {
                &context.club
            };
            let f = field(detail, &old.key)?;
            let binding = f.binding.as_ref().context("읽기 전용 필드")?;
            (
                binding.address,
                binding.original.clone(),
                transaction::encode_value(f, old.before)?,
            )
        };
        ensure!(
            address == old.address && current == old.new_bytes && bytes == old.old_bytes,
            "{}가 적용 후 바뀌어 되돌리기를 중단했습니다",
            old.label
        );
        changes.push(Change {
            key: old.key.clone(),
            label: old.label.clone(),
            before: old.after,
            after: old.before,
            address,
            old_bytes: current,
            new_bytes: bytes,
        });
    }
    // Reconstruct each complete sponsor tuple and check its invariants.
    for slot in sponsor_slots {
        let s = context.sponsors.iter().find(|s| s.slot == slot).unwrap();
        let mut raw = s.raw.clone();
        for c in &changes {
            if c.key.starts_with(&format!("sponsor.{slot}.")) {
                let offset = (c.address - s.address) as usize;
                raw[offset..offset + 4].copy_from_slice(&c.new_bytes);
            }
        }
        ensure!(
            Date::unpack(d(&raw, 0))? < Date::unpack(d(&raw, 4))?
                && d(&raw, 12) <= d(&raw, 8)
                && d(&raw, 8) <= i32::MAX as u32,
            "복구할 후원 계약의 기간/금액 오류"
        );
    }
    changes.sort_by_key(|c| c.address);
    validate_change_layout(&changes)?;
    reversed.changes = changes;
    reversed.deltas = receipt
        .plan
        .deltas
        .iter()
        .map(|d| EventDelta {
            section: d.section.clone(),
            label: d.label.clone(),
            before: d.after.clone(),
            after: d.before.clone(),
        })
        .collect();
    reversed.narrative = format!(
        "이벤트 되돌리기: {}\n원본 기록: {}",
        receipt.plan.draft.title, receipt.id
    );
    Ok(reversed)
}
pub fn undo(receipt: &EventReceipt) -> Result<EventReceipt> {
    ensure!(
        receipt.status.starts_with("applied"),
        "완료한 이벤트만 되돌릴 수 있습니다"
    );
    valid_id(&receipt.id)?;
    let stored_path = folder("history")?.join(format!("{}.json", receipt.id));
    let stored: EventReceipt = serde_json::from_slice(
        &std::fs::read(&stored_path).context("이벤트 백업을 찾을 수 없습니다")?,
    )?;
    ensure!(
        stored.status.starts_with("applied")
            && same_changes(&stored.plan.changes, &receipt.plan.changes),
        "이미 되돌렸거나 원본 이벤트 기록이 다릅니다"
    );
    let fresh = capture(
        &stored.plan.context.session,
        &stored.plan.context.club.entity,
    )?;
    let reversed = reverse_for_context(&stored, &fresh)?;
    let mut result = execute(reversed, &format!("undone:{}", receipt.id))?;
    let mut marked = stored;
    marked.status = format!("reverted:{}", result.id);
    if let Err(e) = durable_json(&stored_path, &marked, false) {
        result.status = format!("{}_origin_log_warning: {e}", result.status);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};
    fn entity(kind: EntityKind, address: u64) -> Entity {
        Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id: 1141,
            kind,
            name: "레체".into(),
            english_name: "Lecce".into(),
            club: "레체".into(),
            club_id: Some(1141),
            age: None,
            ca: None,
            pa: None,
            role: String::new(),
            group: String::new(),
            address,
            base: address,
            index: Some(1),
        }
    }
    fn scalar(key: &str, at: u64, value: i32, min: f64, max: f64) -> Field {
        Field {
            key: key.into(),
            label: key.into(),
            section: "test".into(),
            value: value as f64,
            min,
            max,
            step: 1.,
            hint: String::new(),
            binding: Some(Binding {
                address: at,
                storage: Storage::I32,
                scale: 1.,
                original: value.to_le_bytes().to_vec(),
            }),
        }
    }
    fn context() -> EventContext {
        let fields = vec![
            scalar("club.cash", 0x11000, 100, -2e9, 2e9),
            scalar("club.transfer_base", 0x11004, 50, -2e9, 2e9),
            scalar("club.transfer_current", 0x11008, 40, -2e9, 2e9),
            scalar("club.weekly_wage", 0x1100c, 30, 0., 2e9),
        ];
        let mut raw = vec![0; 24];
        raw[..4].copy_from_slice(&Date::parse("2025-01-01").unwrap().packed().to_le_bytes());
        raw[4..8].copy_from_slice(&Date::parse("2029-01-01").unwrap().packed().to_le_bytes());
        raw[8..12].copy_from_slice(&400i32.to_le_bytes());
        raw[12..16].copy_from_slice(&100i32.to_le_bytes());
        raw[20] = 4;
        raw[21] = 7;
        EventContext {
            session: Session {
                pid: 1,
                created: 1,
                module_base: 0x10000,
                module_size: 0x1000,
                module_stamp: 1,
                db: 1,
                registry_begin: 1,
                game_date: "2027-11-05".into(),
                profile: "fixture".into(),
                own_club_id: Some(1141),
            },
            club: Detail {
                entity: entity(EntityKind::Club, 0x10000),
                fields,
                guards: vec![Guard {
                    address: 0x10000,
                    expected: vec![7],
                }],
                notes: vec![],
            },
            city: Some(Detail {
                entity: entity(EntityKind::City, 0x30000),
                fields: vec![
                    scalar("city.population_band", 0x31000, 7, 0., 15.),
                    scalar("city.attraction", 0x31004, 11, 0., 20.),
                ],
                guards: vec![Guard {
                    address: 0x30000,
                    expected: vec![8],
                }],
                notes: vec![],
            }),
            sponsors: vec![SponsorRecord {
                slot: 0,
                kind: 4,
                annual_gbp: 100,
                total_gbp: 400,
                start: "2025-01-01".into(),
                end: "2029-01-01".into(),
                address: 0x20000,
                raw,
            }],
            game_date: "2027-11-05".into(),
            warnings: vec![],
            guards: vec![],
        }
    }
    #[test]
    fn dates_roundtrip_and_anniversary() {
        for text in ["2026-08-24", "2028-02-29", "2100-03-01"] {
            let date = Date::parse(text).unwrap();
            assert_eq!(Date::unpack(date.packed()).unwrap(), date);
        }
        assert_eq!(
            Date::parse("2028-02-29")
                .unwrap()
                .anniversary(10)
                .unwrap()
                .text(),
            "2038-02-28"
        );
        assert!(Date::parse("2100-02-29").is_err());
        assert!(Date::unpack((2027 << 16) | 366).is_err());
    }
    #[test]
    fn sponsorship_does_not_add_cash_and_preserves_flags() {
        let c = context();
        let p = plan(
            &c,
            &EventDraft {
                sponsor_slot: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(p.changes.iter().all(|c| c.key.starts_with("sponsor.")));
        assert_eq!(
            p.changes
                .iter()
                .find(|c| c.key.ends_with(".total"))
                .unwrap()
                .after,
            266_535_220.
        );
        assert!(
            p.changes
                .iter()
                .all(|v| v.address + v.new_bytes.len() as u64 <= 0x20010)
        );
        assert_eq!(c.sponsors[0].raw[21], 7);
    }
    #[test]
    fn sponsorship_date_changes_preserve_independent_low_word_flags() {
        let mut c = context();
        c.city = None;
        for (offset, flags) in [(0, 0x4200u32), (4, 0xa600u32)] {
            let raw = d(&c.sponsors[0].raw, offset) | flags;
            c.sponsors[0].raw[offset..offset + 4].copy_from_slice(&raw.to_le_bytes());
        }
        let p = plan(
            &c,
            &EventDraft {
                sponsor_slot: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        let start = p
            .changes
            .iter()
            .find(|c| c.key.ends_with(".start"))
            .unwrap();
        let end = p.changes.iter().find(|c| c.key.ends_with(".end")).unwrap();
        assert_eq!(d(&start.new_bytes, 0) & 0xfe00, 0x4200);
        assert_eq!(d(&end.new_bytes, 0) & 0xfe00, 0xa600);
        assert_eq!(
            Date::unpack(d(&start.new_bytes, 0)).unwrap().text(),
            "2027-11-05"
        );
        assert_eq!(
            Date::unpack(d(&end.new_bytes, 0)).unwrap().text(),
            "2037-11-05"
        );
        let m = memory(&p);
        write_atomic(&m, &p.changes).unwrap();
        let fresh = refreshed_context(&c, &m);
        let receipt = EventReceipt {
            id: p.id.clone(),
            time: now(),
            status: "applied".into(),
            plan: p,
            game_saved: false,
            engine_accrual_verified: false,
        };
        let reverse = reverse_for_context(&receipt, &fresh).unwrap();
        validate_current(&m, &reverse.guards, &reverse.changes).unwrap();
        write_atomic(&m, &reverse.changes).unwrap();
        assert_eq!(
            m.read(c.sponsors[0].address, 24).unwrap(),
            c.sponsors[0].raw
        );
    }
    #[test]
    fn grants_independent_and_not_doubled() {
        let c = context();
        let p = plan(
            &c,
            &EventDraft {
                annual_gbp: 0,
                cash_grant_gbp: 900,
                transfer_budget_grant_gbp: 10,
                weekly_wage_increase_gbp: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let val = |k: &str| p.changes.iter().find(|x| x.key == k).unwrap().after;
        assert_eq!(val("club.cash"), 1000.);
        assert_eq!(val("club.transfer_base"), 60.);
        assert_eq!(val("club.transfer_current"), 50.);
        assert_eq!(val("club.weekly_wage"), 32.);
        assert_eq!(p.changes.len(), 4);
    }
    #[test]
    fn validates_money_targets_and_city() {
        let c = context();
        for draft in [
            EventDraft::default(),
            EventDraft {
                sponsor_slot: Some(0),
                annual_gbp: 2_000_000_000,
                years: 2,
                ..Default::default()
            },
            EventDraft {
                annual_gbp: 0,
                cash_grant_gbp: -1,
                ..Default::default()
            },
            EventDraft {
                annual_gbp: 0,
                population_band: Some(16),
                ..Default::default()
            },
            EventDraft {
                annual_gbp: 0,
                attraction: Some(21),
                ..Default::default()
            },
            EventDraft {
                annual_gbp: 0,
                cash_grant_gbp: 10,
                krw_per_gbp: Some(f64::NAN),
                ..Default::default()
            },
            EventDraft {
                annual_gbp: 0,
                ..Default::default()
            },
        ] {
            assert!(plan(&c, &draft).is_err());
        }
    }
    #[test]
    fn city_does_not_imply_grants() {
        let p = plan(
            &context(),
            &EventDraft {
                annual_gbp: 0,
                population_band: Some(8),
                attraction: Some(13),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.changes.len(), 2);
        assert!(p.changes.iter().all(|v| v.key.starts_with("city.")));
    }
    struct Mock {
        bytes: RefCell<BTreeMap<u64, u8>>,
        fail: RefCell<Option<u64>>,
        writes: RefCell<usize>,
    }
    impl EventMemory for Mock {
        fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
            (0..size)
                .map(|i| {
                    self.bytes
                        .borrow()
                        .get(&(address + i as u64))
                        .copied()
                        .context("unmapped")
                })
                .collect()
        }
        fn write(&self, address: u64, bytes: &[u8]) -> Result<()> {
            *self.writes.borrow_mut() += 1;
            for (i, b) in bytes.iter().enumerate() {
                self.bytes.borrow_mut().insert(address + i as u64, *b);
            }
            if *self.fail.borrow() == Some(address) {
                self.fail.replace(None);
                bail!("partial");
            }
            Ok(())
        }
    }
    fn memory(p: &EventPlan) -> Mock {
        let m = Mock {
            bytes: RefCell::new(BTreeMap::new()),
            fail: RefCell::new(None),
            writes: RefCell::new(0),
        };
        for g in &p.guards {
            m.write(g.address, &g.expected).unwrap();
        }
        for c in &p.changes {
            m.write(c.address, &c.old_bytes).unwrap();
        }
        m.writes.replace(0);
        m
    }
    #[test]
    fn atomic_partial_failure_restores_sponsor_and_city() {
        let p = plan(
            &context(),
            &EventDraft {
                sponsor_slot: Some(0),
                population_band: Some(8),
                attraction: Some(13),
                ..Default::default()
            },
        )
        .unwrap();
        let m = memory(&p);
        m.fail.replace(Some(p.changes.last().unwrap().address));
        validate_current(&m, &p.guards, &p.changes).unwrap();
        assert!(write_atomic(&m, &p.changes).is_err());
        for c in &p.changes {
            assert_eq!(m.read(c.address, c.old_bytes.len()).unwrap(), c.old_bytes);
        }
    }
    #[test]
    fn stale_previews_and_repeated_cash_grant_rejected() {
        let p = plan(
            &context(),
            &EventDraft {
                annual_gbp: 0,
                cash_grant_gbp: 50,
                ..Default::default()
            },
        )
        .unwrap();
        let m = memory(&p);
        validate_current(&m, &p.guards, &p.changes).unwrap();
        write_atomic(&m, &p.changes).unwrap();
        assert!(validate_current(&m, &p.guards, &p.changes).is_err());
        assert_eq!(
            i32::from_le_bytes(m.read(0x11000, 4).unwrap().try_into().unwrap()),
            150
        );
    }
    #[test]
    fn reverse_refuses_relocated_fields() {
        let c = context();
        let p = plan(
            &c,
            &EventDraft {
                annual_gbp: 0,
                cash_grant_gbp: 50,
                ..Default::default()
            },
        )
        .unwrap();
        let receipt = EventReceipt {
            id: p.id.clone(),
            time: now(),
            status: "applied".into(),
            plan: p,
            game_saved: false,
            engine_accrual_verified: false,
        };
        let mut fresh = c;
        fresh.club.fields[0].value = 150.;
        fresh.club.fields[0].binding.as_mut().unwrap().original = 150i32.to_le_bytes().to_vec();
        assert!(reverse_for_context(&receipt, &fresh).is_ok());
        fresh.club.fields[0].binding.as_mut().unwrap().address = 0x12000;
        assert!(reverse_for_context(&receipt, &fresh).is_err());
    }
    #[test]
    fn game_date_low_word_flags_are_not_part_of_ordinal() {
        let date = Date::parse("2027-11-05").unwrap();
        assert_eq!(Date::unpack(date.packed() | 0x7e00).unwrap(), date);
    }
    #[test]
    fn changed_guard_rejects_with_zero_writes() {
        let p = plan(
            &context(),
            &EventDraft {
                annual_gbp: 0,
                cash_grant_gbp: 50,
                ..Default::default()
            },
        )
        .unwrap();
        let m = memory(&p);
        m.bytes.borrow_mut().insert(p.guards[0].address, 99);
        assert!(validate_current(&m, &p.guards, &p.changes).is_err());
        assert_eq!(*m.writes.borrow(), 0);
    }
    fn refreshed_context(c: &EventContext, memory: &Mock) -> EventContext {
        let mut out = c.clone();
        for detail in std::iter::once(&mut out.club).chain(out.city.iter_mut()) {
            for f in &mut detail.fields {
                if let Some(b) = f.binding.as_mut() {
                    if let Ok(raw) = memory.read(b.address, b.original.len()) {
                        f.value = i32::from_le_bytes(raw[..4].try_into().unwrap()) as f64;
                        b.original = raw;
                    }
                }
            }
        }
        for s in &mut out.sponsors {
            s.raw = memory.read(s.address, 24).unwrap();
            s.start = Date::unpack(d(&s.raw, 0)).unwrap().text();
            s.end = Date::unpack(d(&s.raw, 4)).unwrap().text();
            s.total_gbp = d(&s.raw, 8) as i32;
            s.annual_gbp = d(&s.raw, 12) as i32;
        }
        out
    }
    #[test]
    fn complete_scenario_apply_and_undo_roundtrip() {
        let c = context();
        let p = plan(
            &c,
            &EventDraft {
                sponsor_slot: Some(0),
                cash_grant_gbp: 50,
                transfer_budget_grant_gbp: 20,
                population_band: Some(8),
                attraction: Some(13),
                ..Default::default()
            },
        )
        .unwrap();
        let m = memory(&p);
        validate_current(&m, &p.guards, &p.changes).unwrap();
        write_atomic(&m, &p.changes).unwrap();
        let fresh = refreshed_context(&c, &m);
        let receipt = EventReceipt {
            id: p.id.clone(),
            time: now(),
            status: "applied".into(),
            plan: p,
            game_saved: false,
            engine_accrual_verified: false,
        };
        let reverse = reverse_for_context(&receipt, &fresh).unwrap();
        validate_current(&m, &reverse.guards, &reverse.changes).unwrap();
        write_atomic(&m, &reverse.changes).unwrap();
        for c in &receipt.plan.changes {
            assert_eq!(m.read(c.address, c.old_bytes.len()).unwrap(), c.old_bytes);
        }
        assert_eq!(m.read(0x20000, 24).unwrap(), c.sponsors[0].raw);
    }
    #[test]
    fn undo_rejects_record_reuse_and_forged_fields() {
        let c = context();
        let p = plan(
            &c,
            &EventDraft {
                sponsor_slot: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        let m = memory(&p);
        write_atomic(&m, &p.changes).unwrap();
        let mut fresh = refreshed_context(&c, &m);
        let mut receipt = EventReceipt {
            id: p.id.clone(),
            time: now(),
            status: "applied".into(),
            plan: p,
            game_saved: false,
            engine_accrual_verified: false,
        };
        fresh.sponsors[0].raw[20] = 9;
        assert!(reverse_for_context(&receipt, &fresh).is_err());
        fresh.sponsors[0].raw[20] = 4;
        receipt.plan.changes[0].key = "arbitrary.pointer".into();
        assert!(reverse_for_context(&receipt, &fresh).is_err());
    }
}
