//! Read-only staffing advice. Scores are the user's formulas, not FM stars or
//! a promise of recruitment interest. This module contains no process access.
use crate::model::{Detail, Entity, EntityKind, FootballCategory};
use serde::Serialize;

pub const SCORE_MAX: f64 = 300.;
pub const STRONG_SCORE: f64 = 255.;
pub const MIN_GAIN: f64 = 18.;

#[derive(Clone, Debug, Serialize)]
pub struct Person {
    pub id: u32,
    pub kind: EntityKind,
    pub name: String,
    pub english_name: String,
    pub club: String,
    pub club_id: Option<u32>,
    pub age: Option<u32>,
    pub ca: Option<u16>,
    pub pa: Option<u16>,
    pub role: String,
    pub group: String,
    pub football_category: FootballCategory,
}
impl From<&Entity> for Person {
    fn from(e: &Entity) -> Self {
        Self {
            id: e.id,
            kind: e.kind,
            name: e.name.clone(),
            english_name: e.english_name.clone(),
            club: e.club.clone(),
            club_id: e.club_id,
            age: e.age,
            ca: e.ca,
            pa: e.pa,
            role: e.role.clone(),
            group: e.group.clone(),
            football_category: e.football_category,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Coach,
    Assistant,
    Manager,
    Goalkeeper,
    Fitness,
    SetPieces,
    YouthHead,
    Scout,
    RecruitAnalyst,
    Analyst,
    HeadAnalyst,
    Physio,
    Scientist,
    Director,
    TechnicalDirector,
    Loan,
    Unknown,
}

pub fn family(e: &Entity) -> Family {
    let role = e.role.as_str();
    if role.contains("유소년 육성") {
        Family::YouthHead
    } else if role.contains("테크니컬") {
        Family::TechnicalDirector
    } else if role.contains("풋볼 디렉터") {
        Family::Director
    } else if role.contains("임대 매니저") {
        Family::Loan
    } else if role.contains("수석") && role.contains("분석") {
        Family::HeadAnalyst
    } else if role.contains("영입") && role.contains("분석") {
        Family::RecruitAnalyst
    } else if role.contains("분석") {
        Family::Analyst
    } else if role.contains("스카우트") {
        Family::Scout
    } else if role.contains("스포츠 과학") {
        Family::Scientist
    } else if role.contains("물리치료") || role.contains("의사") {
        Family::Physio
    } else if role.contains("골키퍼") {
        Family::Goalkeeper
    } else if role.contains("피트니스") || role.contains("체력 코치") {
        Family::Fitness
    } else if role.contains("세트피스") {
        Family::SetPieces
    } else if role.contains("수석 코치") {
        Family::Assistant
    } else if role.contains("감독") {
        Family::Manager
    } else if role.contains("코치") {
        Family::Coach
    } else {
        Family::Unknown
    }
}
pub fn youth(e: &Entity) -> bool {
    youth_role_group(&e.role, &e.group)
}
fn youth_role_group(role: &str, group: &str) -> bool {
    let group = group.to_uppercase();
    ["U-18", "U-19", "U-20", "U18", "U19", "U20", "유소년"]
        .iter()
        .any(|p| group.contains(p))
        || role.contains("유소년")
        || role.contains("U-19")
}
pub fn value(d: &Detail, key: &str) -> Option<f64> {
    d.fields
        .iter()
        .find(|f| f.key == key && f.value.is_finite())
        .map(|f| f.value)
}
pub fn reputation(d: &Detail) -> Option<f64> {
    value(d, "current_reputation")
        .or_else(|| value(d, "world_reputation"))
        .filter(|v| (0. ..=10000.).contains(v))
}
pub fn club_reputation(d: &Detail) -> Option<f64> {
    value(d, "club.reputation").filter(|v| (0. ..=10000.).contains(v))
}

const SPECIALISTS: &[&str] = &[
    "technical_attack",
    "tactical_attack",
    "technical_defense",
    "tactical_defense",
    "technical_mental",
    "tactical_mental",
];
pub fn score_key(d: &Detail) -> Option<String> {
    let e = &d.entity;
    let key = match family(e) {
        Family::Coach if youth(e) => "youth_coach",
        Family::Coach => {
            return SPECIALISTS
                .iter()
                .filter_map(|key| {
                    let key = format!("score_{key}");
                    value(d, &key).map(|v| (key, v))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|x| x.0);
        }
        Family::Assistant if youth(e) => "youth_assistant",
        Family::Assistant => "assistant",
        Family::Manager if youth(e) => "youth_manager",
        Family::Manager if e.group == "2군" => "reserve_manager",
        // There is no user formula for a first-team manager. Don't silently
        // substitute the reserve-team calculation and recommend dismissal.
        Family::Manager => return None,
        Family::Goalkeeper if youth(e) => "youth_goalkeeper",
        Family::Goalkeeper => {
            return ["score_gk_shot", "score_gk_distribution"]
                .iter()
                .filter_map(|key| value(d, key).map(|v| ((*key).to_string(), v)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|x| x.0);
        }
        Family::Fitness => "fitness",
        Family::SetPieces => "set_pieces",
        Family::YouthHead => "head_youth",
        Family::Scout => "scout",
        Family::RecruitAnalyst => "recruitment_analyst",
        Family::Analyst => "performance_analyst",
        Family::HeadAnalyst => "head_analyst",
        Family::Physio => "physio",
        Family::Scientist => "scientist",
        Family::Director => "dof",
        Family::TechnicalDirector => "td",
        Family::Loan => "loan_manager",
        Family::Unknown => return None,
    };
    Some(format!("score_{key}"))
}

#[derive(Clone, Debug, Serialize)]
pub struct Component {
    pub label: String,
    pub value: f64,
    pub weight: f64,
    pub contribution: f64,
}
pub fn components(d: &Detail, key: &str) -> Vec<Component> {
    let ddm = [
        ("discipline", 2.),
        ("determination", 2.),
        ("motivation", 2.),
    ];
    let five = [
        ("attack", 1.),
        ("defense", 1.),
        ("mental", 1.),
        ("technical", 1.),
        ("tactical", 1.),
    ];
    let mut weights: Vec<(&str, f64)> = vec![];
    let k = key.trim_start_matches("score_");
    if let Some((kind, focus)) = k.split_once('_').filter(|(a, b)| {
        ["technical", "tactical"].contains(a) && ["attack", "defense", "mental"].contains(b)
    }) {
        weights.extend([(kind, 5.), (focus, 4.)]);
        weights.extend(ddm);
    } else {
        match k {
            "fitness" | "gk_shot" | "physio" | "scientist" => {
                weights.push((
                    match k {
                        "physio" => "physiotherapy",
                        "scientist" => "sports_science",
                        _ => k,
                    },
                    9.,
                ));
                weights.extend(ddm);
            }
            "gk_distribution" => {
                weights.extend([("gk_distribution", 4.5), ("gk_handling", 4.5)]);
                weights.extend(ddm);
            }
            "set_pieces" => {
                weights.extend([("set_pieces", 5.), ("knowledge", 4.)]);
                weights.extend(ddm);
            }
            "assistant" | "youth_assistant" => {
                weights.extend(ddm);
                weights.extend(five);
                weights.extend([
                    ("knowledge", 1.),
                    ("judging_potential", 0.5),
                    ("judging_ability", 0.5),
                ]);
                weights.push((
                    if k == "assistant" {
                        "management"
                    } else {
                        "youth"
                    },
                    2.,
                ));
            }
            "general" | "youth_coach" => {
                weights.extend([
                    ("discipline", 3.),
                    ("determination", 3.),
                    ("motivation", 3.),
                ]);
                weights.extend(five);
                weights.push((
                    if k == "general" {
                        "management"
                    } else {
                        "youth"
                    },
                    1.,
                ));
            }
            "reserve_manager" => {
                weights.extend([
                    ("management", 2.),
                    ("judging_potential", 2.),
                    ("judging_ability", 2.),
                    ("discipline", 1.),
                    ("determination", 1.),
                    ("motivation", 1.),
                    ("knowledge", 1.),
                ]);
                weights.extend(five);
            }
            "youth_manager" => {
                weights.extend([
                    ("discipline", 1.),
                    ("determination", 1.),
                    ("motivation", 1.),
                    ("youth", 2.),
                    ("management", 2.),
                    ("judging_potential", 1.),
                    ("judging_ability", 1.),
                    ("knowledge", 1.),
                ]);
                weights.extend(five);
            }
            "youth_goalkeeper" => {
                weights.extend([
                    ("gk_shot", 3.),
                    ("gk_handling", 3.),
                    ("gk_distribution", 3.),
                ]);
                weights.extend(ddm);
            }
            "head_youth" => weights.extend([
                ("youth", 5.),
                ("judging_potential", 4.),
                ("judging_ability", 4.),
                ("discipline", 1.),
                ("determination", 1.),
            ]),
            "dof" => weights.extend([
                ("negotiation", 7.),
                ("judging_potential", 3.),
                ("judging_ability", 3.),
                ("discipline", 1.),
                ("determination", 1.),
            ]),
            "td" => weights.extend([
                ("judging_staff", 5.),
                ("negotiation", 5.),
                ("discipline", 2.),
                ("determination", 2.),
                ("management", 1.),
            ]),
            "loan_manager" => weights.extend(
                crate::fields::LOAN_MANAGER_WEIGHTS
                    .iter()
                    .map(|&(key, weight)| {
                        (key, weight * crate::fields::loan_manager_score_scale())
                    }),
            ),
            "scout" => weights.extend([
                ("judging_potential", 5.),
                ("judging_ability", 5.),
                ("adaptability", 3.),
                ("discipline", 1.),
                ("determination", 1.),
            ]),
            "head_analyst" => weights.extend([
                ("analysis", 6.),
                ("knowledge", 2.5),
                ("judging_ability", 2.5),
                ("management", 2.),
                ("discipline", 1.),
                ("determination", 1.),
            ]),
            "performance_analyst" | "recruitment_analyst" => weights.extend([
                ("analysis", 7.),
                (
                    if k == "performance_analyst" {
                        "knowledge"
                    } else {
                        "judging_ability"
                    },
                    6.,
                ),
                ("discipline", 1.),
                ("determination", 1.),
            ]),
            _ => {}
        }
    }
    weights
        .into_iter()
        .filter_map(|(k, w)| {
            let key = if k == "adaptability" {
                k.to_string()
            } else {
                format!("staff_{k}")
            };
            d.fields
                .iter()
                .find(|f| f.key == key && f.value.is_finite())
                .map(|f| Component {
                    label: f.label.clone(),
                    value: f.value.round(),
                    weight: w,
                    contribution: f.value.round() * w,
                })
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct Alternative {
    pub person: Person,
    pub score: f64,
    pub improvement: f64,
    pub reputation: Option<f64>,
    pub club_reputation: Option<f64>,
    pub reputation_gap: Option<f64>,
    pub weekly_wage: Option<f64>,
    pub contract_expires: Option<f64>,
    pub availability: String,
    pub availability_label: String,
    pub role_fit: String,
    pub reasons: Vec<String>,
    pub components: Vec<Component>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub person: Person,
    pub role: String,
    pub group: String,
    pub score_key: Option<String>,
    pub score_label: String,
    pub score: Option<f64>,
    pub max_score: f64,
    pub reputation: Option<f64>,
    pub weekly_wage: Option<f64>,
    pub contract_expires: Option<f64>,
    pub status: String,
    pub status_label: String,
    pub reasons: Vec<String>,
    pub components: Vec<Component>,
    pub alternatives: Vec<Alternative>,
}
pub fn incumbent(e: &Entity, d: Option<&Detail>) -> Row {
    let key = d.and_then(score_key);
    let score = key
        .as_ref()
        .and_then(|k| d.and_then(|d| value(d, k)))
        .filter(|v| (0. ..=SCORE_MAX).contains(v));
    let label = key
        .as_ref()
        .and_then(|k| d.and_then(|d| d.fields.iter().find(|f| &f.key == k)))
        .map(|f| f.label.clone())
        .unwrap_or_else(|| "직무 점수 미확인".into());
    Row {
        person: e.into(),
        role: e.role.clone(),
        group: e.group.clone(),
        score_key: key.clone(),
        score_label: label,
        score,
        max_score: SCORE_MAX,
        reputation: d.and_then(reputation),
        weekly_wage: d.and_then(|d| value(d, "contract.weekly_wage")),
        contract_expires: d.and_then(|d| value(d, "contract.expires")),
        status: "insufficient".into(),
        status_label: "자료 부족".into(),
        reasons: vec![],
        components: key
            .as_ref()
            .and_then(|k| d.map(|d| components(d, k)))
            .unwrap_or_default(),
        alternatives: vec![],
    }
}

fn fit(target: &Person, candidate: &Entity) -> &'static str {
    // Reconstruct only the semantic role for the already sanitized incumbent.
    let target_entity = Entity {
        nationality: String::new(),
        nationality_id: None,
        football_category: target.football_category,
        id: target.id,
        kind: target.kind,
        name: target.name.clone(),
        english_name: String::new(),
        club: String::new(),
        club_id: target.club_id,
        age: None,
        ca: None,
        pa: None,
        role: target.role.clone(),
        group: target.group.clone(),
        address: 0,
        base: 0,
        index: None,
    };
    let a = family(&target_entity);
    let b = family(candidate);
    if a == Family::Unknown || b == Family::Unknown {
        "unknown"
    }
    // A chief staff member may reject an ordinary job even at comparable
    // reputation. Similarly a club doctor is not a verified physio applicant.
    else if a == b
        && ((candidate.role.contains("수석") && !target.role.contains("수석"))
            || (candidate.role.contains("의사") != target.role.contains("의사")))
    {
        "adjacent"
    } else if a == b {
        "same"
    } else if matches!(
        (a, b),
        (
            Family::Coach | Family::Assistant | Family::Manager,
            Family::Coach | Family::Assistant | Family::Manager
        ) | (
            Family::RecruitAnalyst | Family::Analyst | Family::HeadAnalyst,
            Family::RecruitAnalyst | Family::Analyst | Family::HeadAnalyst
        ) | (
            Family::Fitness | Family::SetPieces | Family::Goalkeeper,
            Family::Coach
        ) | (Family::YouthHead, Family::Manager | Family::Assistant)
    ) {
        "adjacent"
    } else {
        "different"
    }
}
pub fn candidate_relevant(row: &Row, d: &Detail) -> bool {
    row.person.id != d.entity.id
        && fit(&row.person, &d.entity) != "different"
        && row
            .score_key
            .as_ref()
            .zip(row.score)
            .is_some_and(|(k, s)| value(d, k).is_some_and(|v| v > s && v <= SCORE_MAX))
}
pub fn offer(row: &mut Row, d: &Detail, target_rep: Option<f64>, employer_rep: Option<f64>) {
    if d.entity.employment().is_none() {return;}
    if !candidate_relevant(row, d) {
        return;
    }
    let score = value(d, row.score_key.as_ref().unwrap()).unwrap();
    let improvement = score - row.score.unwrap();
    let rep = reputation(d);
    let role_fit = fit(&row.person, &d.entity);
    // Use the same role/group evidence as score_key. A youth job may have a
    // generic contract-team label; that must not bypass youth suitability.
    let target_youth = youth_role_group(&row.role, &row.group);
    let youth_skill = value(d, "staff_youth");
    let youth_ok = !target_youth || youth_skill.is_some_and(|v| v.round() >= 14.);
    let youth_history = !target_youth || youth(&d.entity) || d.entity.club_id.is_none();
    let mut reasons = vec![format!(
        "같은 {} 계산식에서 +{improvement:.1}점",
        row.score_label
    )];
    let reputation_gap = rep.zip(target_rep).map(|(s, c)| s - c);
    let qualified =
        role_fit == "same" && youth_ok && youth_history && !d.entity.role.contains("의사");
    let (availability, label) = match (target_rep, rep) {
        (Some(club), Some(staff))
            if qualified && staff <= club + 1500. && d.entity.club_id.is_none() =>
        {
            ("approachable", "무직 · 접근 후보")
        }
        (Some(club), Some(staff))
            if qualified
                && staff <= club + 1000.
                && employer_rep.is_some_and(|e| e <= club + 500.) =>
        {
            ("approachable", "명성상 접근 후보")
        }
        (Some(club), Some(staff))
            if staff > club + 1500. || employer_rep.is_some_and(|e| e > club + 1500.) =>
        {
            ("difficult", "상위 명성 · 협상 난도 높음")
        }
        _ => ("uncertain", "직책·지원 의사 확인 필요"),
    };
    if d.entity.club_id.is_none() {
        reasons.push("현재 무직으로 조회됨. 원하는 직책과 급료는 확인 필요".into());
    } else if let Some(rep) = employer_rep {
        reasons.push(format!("소속 구단 명성 {rep:.0} / 10,000"));
    } else {
        reasons.push("소속 구단 명성을 읽지 못해 이직 난도를 판정하지 않음".into());
    }
    if let Some(gap) = reputation_gap {
        reasons.push(format!(
            "후보 명성 − 우리 구단 명성: {gap:+.0} (같은 0–10,000 척도)"
        ));
    }
    if role_fit != "same" {
        reasons.push("현재 등록 직책이 달라 해당 직책 수락 여부 미확인".into());
    }
    if d.entity.role.contains("의사") {
        reasons.push(
            "구단 의사 직책의 직접 채용 가능 여부가 확인되지 않아 접근 가능으로 판정하지 않음"
                .into(),
        );
    }
    if target_youth {
        reasons.push(match youth_skill {
            Some(s) => format!("유소년 육성 {s:.1} / 20"),
            None => "유소년 육성 능력 미확인".into(),
        });
        if !youth_history {
            reasons.push("현재 성인팀 재직자: 연령팀 직책으로 옮길 의사는 미확인".into());
        }
    }
    reasons.push("실제 지원 의사·보상금·요구 급료·언어·계약 제한은 이 점수로 알 수 없음".into());
    row.alternatives.push(Alternative {
        person: (&d.entity).into(),
        score,
        improvement,
        reputation: rep,
        club_reputation: employer_rep,
        reputation_gap,
        weekly_wage: value(d, "contract.weekly_wage"),
        contract_expires: value(d, "contract.expires"),
        availability: availability.into(),
        availability_label: label.into(),
        role_fit: role_fit.into(),
        reasons,
        components: components(d, row.score_key.as_ref().unwrap()),
    });
    // Keep an attainable alternative as well as the highest scoring stretch
    // candidate; otherwise several unattainable stars can erase useful advice.
    row.alternatives.sort_by(|a, b| {
        let rank = |a: &Alternative| match a.availability.as_str() {
            "approachable" => 0,
            "uncertain" => 1,
            _ => 2,
        };
        rank(a)
            .cmp(&rank(b))
            .then(b.score.total_cmp(&a.score))
            .then(a.person.id.cmp(&b.person.id))
    });
    row.alternatives.truncate(5);
}
pub fn finish(row: &mut Row, club_rep: Option<f64>, complete: bool) {
    row.reasons.clear();
    let Some(score) = row.score else {
        row.status = "insufficient".into();
        row.status_label = "자료 부족".into();
        row.reasons.push(
            "현재 직책에 대응하는 검증된 점수 또는 능력치가 없어 교체를 판단하지 않음".into(),
        );
        return;
    };
    row.reasons
        .push(format!("{} 기준 {score:.1} / 300", row.score_label));
    if row.role == "코치" || row.role.contains("골키퍼") {
        row.reasons
            .push("실제 훈련 배정 분야는 미확인: 현재 가장 높은 전문 분야를 기준으로 비교".into());
    }
    if score >= STRONG_SCORE {
        row.status = "retain".into();
        row.status_label = "유지".into();
        row.reasons.push(
            "255점 이상 우수 인력. 더 높은 점수의 후보가 있어도 해고 대상으로 올리지 않음".into(),
        );
    } else if club_rep.is_none() || row.reputation.is_none() {
        row.status = "insufficient".into();
        row.status_label = "자료 부족".into();
        row.reasons
            .push("구단 또는 현재 스태프 명성을 확인할 수 없어 교체 권고 보류".into());
    } else {
        let material = MIN_GAIN.max(score * 0.08);
        let replacement = row
            .alternatives
            .iter()
            .find(|a| a.availability == "approachable" && a.improvement >= material);
        if complete && replacement.is_some() {
            let best = replacement.unwrap();
            row.status = "replace_review".into();
            row.status_label = "교체 검토".into();
            row.reasons.push(format!(
                "{}가 +{:.1}점으로 최소 개선폭 {material:.1}점을 넘고 명성상 접근 가능한 후보",
                best.person.name, best.improvement
            ));
            row.reasons.push(
                "대체자 수락·총 급료·해지 보상금 확인 후에만 해고 검토. 자동 해고하지 않음".into(),
            );
        } else if !row.alternatives.is_empty() {
            row.status = "compare".into();
            row.status_label = "후보 비교".into();
            row.reasons.push(
                if !complete {
                    "검색 중에는 교체를 권고하지 않음"
                } else {
                    "상승폭이 작거나 영입 현실성이 미확인이라 해고를 권고하지 않음"
                }
                .into(),
            );
        } else {
            row.status = "retain".into();
            row.status_label = "유지".into();
            row.reasons.push(
                if complete {
                    "조회된 후보 중 현재 직무 점수가 더 높은 대체자를 찾지 못함"
                } else {
                    "후보 검색 중 · 현재 유지"
                }
                .into(),
            );
        }
    }
    if let Some(wage) = row.weekly_wage {
        row.reasons.push(format!(
            "현재 기본 주급 £{wage:.0}; 해지 보상금은 별도 확인 필요"
        ));
    } else {
        row.reasons
            .push("현재 계약 급료 미확인. 비용상 해고 이득을 추정하지 않음".into());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::Field;
    pub(crate) fn staff(
        id: u32,
        club: Option<u32>,
        role: &str,
        group: &str,
        skill: f64,
        rep: f64,
    ) -> Detail {
        let e = Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id,
            kind: EntityKind::Staff,
            name: format!("검증 스태프 {id}"),
            english_name: format!("Fixture Staff {id}"),
            club: club.map(|x| format!("Club {x}")).unwrap_or_default(),
            club_id: club,
            age: Some(43),
            ca: Some((skill * 10.) as u16),
            pa: Some(200),
            role: role.into(),
            group: group.into(),
            address: 0,
            base: 0,
            index: None,
        };
        let mut d = Detail {
            entity: e,
            fields: vec![],
            guards: vec![],
            notes: vec!["Synthetic staffing review test, not FM data".into()],
        };
        for key in [
            "adaptability",
            "staff_discipline",
            "staff_determination",
            "staff_motivation",
            "staff_attack",
            "staff_defense",
            "staff_fitness",
            "staff_mental",
            "staff_technical",
            "staff_tactical",
            "staff_gk_shot",
            "staff_gk_handling",
            "staff_gk_distribution",
            "staff_knowledge",
            "staff_management",
            "staff_judging_ability",
            "staff_judging_potential",
            "staff_youth",
            "staff_negotiation",
            "staff_judging_staff",
            "staff_analysis",
            "staff_physiotherapy",
            "staff_sports_science",
            "staff_set_pieces",
        ] {
            d.fields.push(field(key, skill));
        }
        d.fields.push(field("current_reputation", rep));
        d.fields.push(field("contract.weekly_wage", 1000.));
        crate::workspace::ensure_scores(&mut d);
        d
    }
    pub(crate) fn field(key: &str, v: f64) -> Field {
        let label = match key {
            "adaptability" => "적응력",
            "staff_discipline" => "기강",
            "staff_determination" => "승부욕",
            "staff_motivation" => "동기 부여",
            "staff_attack" => "공격 훈련",
            "staff_defense" => "수비 훈련",
            "staff_fitness" => "체력 훈련",
            "staff_mental" => "심리 훈련",
            "staff_technical" => "기술 훈련",
            "staff_tactical" => "전술 훈련",
            "staff_gk_shot" => "골키퍼 슈팅 방어",
            "staff_gk_handling" => "골키퍼 볼 핸들링",
            "staff_gk_distribution" => "골키퍼 배급",
            "staff_knowledge" => "전술 이해도",
            "staff_management" => "인력 관리",
            "staff_judging_ability" => "선수 능력 판단",
            "staff_judging_potential" => "선수 잠재력 판단",
            "staff_youth" => "유소년 육성",
            "staff_negotiation" => "협상",
            "staff_judging_staff" => "스태프 능력 판단",
            "staff_analysis" => "데이터 분석",
            "staff_physiotherapy" => "치료 능력",
            "staff_sports_science" => "스포츠 과학",
            "staff_set_pieces" => "세트피스 훈련",
            "current_reputation" => "현재 명성",
            "club.reputation" => "구단 명성",
            "contract.weekly_wage" => "계약 주급 (£/주)",
            _ => key,
        };
        Field {
            key: key.into(),
            label: label.into(),
            section: "검증용 합성 자료".into(),
            value: v,
            min: 0.,
            max: 10000000.,
            step: 1.,
            hint: String::new(),
            binding: None,
        }
    }
    #[test]
    fn weaker_reputation_never_causes_dismissal_by_itself() {
        let d = staff(1, Some(10), "수석 코치", "1군", 13., 100.);
        let mut row = incumbent(&d.entity, Some(&d));
        finish(&mut row, Some(9500.), true);
        assert_eq!(row.status, "retain");
    }
    #[test]
    fn strong_incumbent_is_retained_despite_perfect_candidate() {
        let d = staff(1, Some(10), "수석 코치", "1군", 18., 5000.);
        let alt = staff(2, None, "수석 코치", "무소속", 20., 4000.);
        let mut row = incumbent(&d.entity, Some(&d));
        offer(&mut row, &alt, Some(7000.), None);
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "retain");
        assert_eq!(row.alternatives.len(), 1);
    }
    #[test]
    fn material_accessible_upgrade_allows_review_only_after_scan_completes() {
        let d = staff(1, Some(10), "수석 코치", "1군", 13., 5000.);
        let alt = staff(2, None, "수석 코치", "무소속", 17., 4000.);
        let mut row = incumbent(&d.entity, Some(&d));
        offer(&mut row, &alt, Some(7000.), None);
        finish(&mut row, Some(7000.), false);
        assert_eq!(row.status, "compare");
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "replace_review");
        assert_eq!(row.alternatives[0].score, 255.);
        assert_eq!(row.alternatives[0].improvement, 60.);
    }
    #[test]
    fn high_reputation_star_or_other_role_cannot_trigger_dismissal() {
        let d = staff(1, Some(10), "수석 코치", "1군", 13., 4500.);
        let alt = staff(2, Some(50), "수석 코치", "1군", 19., 9500.);
        let mut row = incumbent(&d.entity, Some(&d));
        offer(&mut row, &alt, Some(6000.), Some(9500.));
        finish(&mut row, Some(6000.), true);
        assert_eq!(row.status, "compare");
        assert_eq!(row.alternatives[0].availability, "difficult");
        let alt = staff(3, None, "감독", "무소속", 20., 4000.);
        offer(&mut row, &alt, Some(6000.), None);
        finish(&mut row, Some(6000.), true);
        assert_eq!(row.status, "compare");
    }
    #[test]
    fn youth_team_rejects_adult_only_availability_assumption() {
        let d = staff(1, Some(10), "코치", "U-20", 12., 4000.);
        let alt = staff(2, Some(50), "코치", "1군", 18., 4000.);
        let mut row = incumbent(&d.entity, Some(&d));
        assert_eq!(row.score_key.as_deref(), Some("score_youth_coach"));
        offer(&mut row, &alt, Some(7000.), Some(5000.));
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "compare");
        let alt = staff(3, None, "코치", "무소속", 18., 4000.);
        offer(&mut row, &alt, Some(7000.), None);
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "replace_review");
    }
    #[test]
    fn youth_role_with_generic_team_keeps_youth_candidate_requirements() {
        let d = staff(1, Some(10), "U-19 감독", "1군", 12., 4000.);
        let mut row = incumbent(&d.entity, Some(&d));
        assert_eq!(row.score_key.as_deref(), Some("score_youth_manager"));

        let adult = staff(2, Some(50), "감독", "1군", 18., 4000.);
        offer(&mut row, &adult, Some(7000.), Some(5000.));
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.alternatives[0].availability, "uncertain");
        assert_eq!(row.status, "compare");

        let mut low_youth = staff(3, None, "감독", "무소속", 18., 4000.);
        low_youth.fields.retain(|f| !f.key.starts_with("score_"));
        low_youth
            .fields
            .iter_mut()
            .find(|f| f.key == "staff_youth")
            .unwrap()
            .value = 10.;
        crate::workspace::ensure_scores(&mut low_youth);
        offer(&mut row, &low_youth, Some(7000.), None);
        finish(&mut row, Some(7000.), true);
        assert!(
            row.alternatives
                .iter()
                .all(|a| a.availability == "uncertain")
        );
        assert_eq!(row.status, "compare");

        let qualified = staff(4, None, "감독", "무소속", 18., 4000.);
        offer(&mut row, &qualified, Some(7000.), None);
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "replace_review");
    }
    #[test]
    fn chief_and_doctor_candidates_do_not_imply_ordinary_job_acceptance() {
        let d = staff(1, Some(10), "물리치료사", "1군", 12., 4000.);
        let mut row = incumbent(&d.entity, Some(&d));
        for (id, role) in [(2, "수석 물리치료사"), (3, "구단 의사")] {
            offer(
                &mut row,
                &staff(id, None, role, "무소속", 20., 4000.),
                Some(7000.),
                None,
            );
        }
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "compare");
        assert!(row.alternatives.iter().all(|a| a.role_fit == "adjacent"));
    }
    #[test]
    fn no_data_and_first_team_manager_do_not_get_substitute_scores() {
        let d = staff(1, Some(10), "감독", "1군", 12., 4000.);
        let mut row = incumbent(&d.entity, Some(&d));
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "insufficient");
        let mut d = staff(2, Some(10), "수석 코치", "1군", 12., 4000.);
        d.fields.retain(|f| !f.key.contains("reputation"));
        let mut row = incumbent(&d.entity, Some(&d));
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "insufficient");
    }
    #[test]
    fn score_components_match_all_existing_formulas_with_unequal_attributes() {
        let mut d = staff(1, None, "코치", "무소속", 15., 4000.);
        d.fields.retain(|f| !f.key.starts_with("score_"));
        for (i, f) in d
            .fields
            .iter_mut()
            .filter(|f| f.key.starts_with("staff_") || f.key == "adaptability")
            .enumerate()
        {
            f.value = (5 + i % 16) as f64;
        }
        crate::workspace::ensure_scores(&mut d);
        let scores = d
            .fields
            .iter()
            .filter(|f| f.key.starts_with("score_"))
            .collect::<Vec<_>>();
        assert_eq!(scores.len(), 27);
        for f in scores {
            let sum = components(&d, &f.key)
                .iter()
                .map(|c| c.contribution)
                .sum::<f64>();
            assert!((sum - f.value).abs() < 1e-9, "{}", f.key);
        }
    }
    #[test]
    fn loan_negotiation_changes_candidate_ranking_on_the_shared_scale() {
        let mut low = staff(1, Some(10), "임대 매니저", "1군", 15., 4000.);
        let mut high = staff(2, None, "임대 매니저", "무소속", 15., 4000.);
        for (d, negotiation) in [(&mut low, 5.), (&mut high, 20.)] {
            d.fields.retain(|f| !f.key.starts_with("score_"));
            d.fields
                .iter_mut()
                .find(|f| f.key == "staff_negotiation")
                .unwrap()
                .value = negotiation;
            crate::workspace::ensure_scores(d);
        }
        let low_score = value(&low, "score_loan_manager").unwrap();
        let high_score = value(&high, "score_loan_manager").unwrap();
        assert!((low_score - 245. * 300. / 380.).abs() < 1e-9);
        assert!((high_score - 305. * 300. / 380.).abs() < 1e-9);
        let mut row = incumbent(&low.entity, Some(&low));
        offer(&mut row, &high, Some(7000.), None);
        finish(&mut row, Some(7000.), true);
        assert_eq!(row.status, "replace_review");
        assert!((row.alternatives[0].improvement - (high_score - low_score)).abs() < 1e-9);
        assert_eq!(
            row.components.iter().filter(|c| c.label == "협상").count(),
            1
        );
    }
    #[test]
    fn all_public_reports_omit_memory_details() {
        let d = staff(1, None, "수석 코치", "무소속", 15., 4000.);
        let row = incumbent(&d.entity, Some(&d));
        let json = serde_json::to_string(&row).unwrap();
        for token in ["address", "binding", "guards", "base", "original"] {
            assert!(!json.contains(token));
        }
    }
}
