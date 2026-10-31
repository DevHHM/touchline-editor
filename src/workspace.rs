//! Address-free search rules and local scouting/preset library.
use crate::model::*;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
mod search_conditions;
mod shortlists;
pub mod fm_shortlists;
pub use shortlists::ShortlistAction;
pub use search_conditions::SearchCondition;

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub age_min: Option<u32>,
    pub age_max: Option<u32>,
    pub ca_min: Option<u16>,
    pub ca_max: Option<u16>,
    pub pa_min: Option<u16>,
    pub pa_max: Option<u16>,
    pub role: String,
    pub club_query: String,
    pub group: Option<String>,
    /// Exact primary nationality, never the employing club's nation or a secondary nationality.
    pub nationality_id: Option<u32>,
    pub football_category: Option<FootballCategory>,
    pub employed: bool,
    pub unemployed: bool,
    pub fields: Vec<Range>,
    /// Ordered expression: AND binds within groups separated by OR.
    pub conditions: Vec<SearchCondition>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub key: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}
impl Filter {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.fields.len() <= 64, "능력치 조건은 최대 64개입니다");
        ensure!(self.conditions.len() <= 64, "상세 검색 조건은 최대 64개입니다");
        for condition in &self.conditions { condition.validate()?; }
        ensure!(
            self.nationality_id.is_none_or(|id| id > 0),
            "주 국적 ID는 양수여야 합니다"
        );
        ensure!(
            !self.employed || !self.unemployed,
            "소속 있음과 무소속을 동시에 선택할 수 없습니다"
        );
        ensure!(
            self.age_min.is_none_or(|v| v <= 129) && self.age_max.is_none_or(|v| v <= 129),
            "나이 범위는 0–129입니다"
        );
        ensure!(
            [self.ca_min, self.ca_max, self.pa_min, self.pa_max]
                .into_iter()
                .all(|v| v.is_none_or(|v| v <= 200)),
            "CA·PA 범위는 0–200입니다"
        );
        ensure!(
            self.role.chars().count() <= 100
                && self.club_query.chars().count() <= 200
                && self.group.as_ref().is_none_or(|g| g.chars().count() <= 100),
            "검색 문자열이 너무 깁니다"
        );
        ensure!(
            self.age_min.zip(self.age_max).is_none_or(|(a, b)| a <= b),
            "최소 나이가 최대보다 큽니다"
        );
        ensure!(
            self.ca_min.zip(self.ca_max).is_none_or(|(a, b)| a <= b),
            "CA 범위 오류"
        );
        ensure!(
            self.pa_min.zip(self.pa_max).is_none_or(|(a, b)| a <= b),
            "PA 범위 오류"
        );
        let mut keys = HashSet::new();
        for r in &self.fields {
            ensure!(!r.key.is_empty() && r.key.len() <= 100, "필드 이름 오류");
            ensure!(keys.insert(&r.key), "중복 능력치 조건: {}", r.key);
            ensure!(
                r.min.is_none_or(f64::is_finite)
                    && r.max.is_none_or(f64::is_finite)
                    && r.min.zip(r.max).is_none_or(|(a, b)| a <= b),
                "{} 조건 범위 오류",
                r.key
            );
        }
        Ok(())
    }
    pub fn matches(&self, e: &Entity) -> bool {
        fn within<T: PartialOrd + Copy>(v: Option<T>, lo: Option<T>, hi: Option<T>) -> bool {
            if lo.is_none() && hi.is_none() {
                return true;
            }
            v.is_some_and(|v| lo.is_none_or(|x| v >= x) && hi.is_none_or(|x| v <= x))
        }
        within(e.age, self.age_min, self.age_max)
            && self.nationality_id.is_none_or(|id| {
                matches!(e.kind, EntityKind::Player | EntityKind::Staff | EntityKind::Person)
                    && e.nationality_id == Some(id)
            })
            && self
                .football_category
                .is_none_or(|category| e.football_category == category)
            && within(e.ca, self.ca_min, self.ca_max)
            && within(e.pa, self.pa_min, self.pa_max)
            && (!self.unemployed || e.club_id.is_none())
            && (!self.employed || e.club_id.is_some())
            && (normalize(&self.role).is_empty() || role_matches(&e.role, &self.role))
            && tokens_match(&e.club, &self.club_query)
            && self.group.as_ref().is_none_or(|group| {
                normalize(group).is_empty() || normalize(&e.group) == normalize(group)
            })
            && search_conditions::evaluate(&self.conditions, e, None).is_none_or(|value| value)
    }
    pub fn matches_detail(&self, d: &Detail) -> bool {
        self.fields.iter().all(|r| {
            d.fields.iter().find(|f| f.key == r.key).is_some_and(|f| {
                f.value.is_finite()
                    && r.min.is_none_or(|v| f.value >= v)
                    && r.max.is_none_or(|v| f.value <= v)
            })
        }) && search_conditions::evaluate(&self.conditions, &d.entity, Some(d)) == Some(true)
    }
    pub fn validate_catalog(&self, catalog: &[Field]) -> Result<()> {
        self.validate()?;
        for condition in self.conditions.iter().filter(|c| c.enabled) {
            condition.validate_catalog(catalog)?;
        }
        for range in &self.fields {
            let field = catalog
                .iter()
                .find(|field| field.key == range.key)
                .with_context(|| format!("지원하지 않는 검색 필드입니다: {}", range.key))?;
            for bound in [range.min, range.max].into_iter().flatten() {
                ensure!(
                    bound >= field.min && bound <= field.max,
                    "{} 검색 범위는 {}–{}입니다",
                    field.label,
                    field.min,
                    field.max
                );
            }
        }
        Ok(())
    }
    pub fn condition_field_keys(&self) -> impl Iterator<Item = &str> {
        self.conditions.iter().flat_map(|c| c.field_keys())
    }
    pub fn proven_without_detail(&self, entity: &Entity) -> bool {
        self.fields.is_empty() && self.matches(entity)
            && search_conditions::evaluate(&self.conditions, entity, None) == Some(true)
    }
}

/// Normalize Unicode whitespace and case without changing name spelling.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
pub fn tokens_match(text: &str, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let haystack = normalize(text);
    let needle = normalize(query);
    needle
        .split_whitespace()
        .all(|token| haystack.contains(token))
}
pub fn name_matches(entity: &Entity, query: &str) -> bool {
    let query = normalize(query);
    if query.is_empty() {
        return true;
    }
    if !query.is_empty() && query.chars().all(|ch| ch.is_ascii_digit()) {
        return entity.id.to_string() == query;
    }
    tokens_match(&format!("{} {}", entity.name, entity.english_name), &query)
}
pub fn role_matches(role: &str, query: &str) -> bool {
    let query = normalize(query);
    normalize(role) == query || role.split('·').any(|part| normalize(part) == query)
}
/// Tactical order for players, independent of how a compound role is written.
/// Staff job titles can contain position-like text and must never enter this parser.
pub fn player_position_rank(entity: &Entity) -> Option<usize> {
    if entity.kind != EntityKind::Player {
        return None;
    }
    const POSITIONS: [&str; 15] = [
        "GK", "SW", "DR", "DC", "DL", "WBR", "WBL", "DM", "MR", "MC", "ML", "AMR", "AMC", "AML",
        "ST",
    ];
    entity
        .role
        .split(|c: char| c.is_whitespace() || matches!(c, '·' | '/' | ','))
        .filter_map(|token| {
            POSITIONS
                .iter()
                .position(|position| token.eq_ignore_ascii_case(position))
        })
        .min()
}

/// Both search transports use this comparison before pagination. Descending
/// reverses known positions/job text, keeping unknown players last and UID ties
/// ascending. Player and non-player partitions keep mixed-kind searches transitive.
pub fn compare_roles(a: &Entity, b: &Entity, descending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let direction = |order: Ordering| if descending { order.reverse() } else { order };
    let order = match (a.kind == EntityKind::Player, b.kind == EntityKind::Player) {
        (true, true) => match (player_position_rank(a), player_position_rank(b)) {
            (Some(a), Some(b)) => direction(a.cmp(&b)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => direction(a.role.cmp(&b.role)),
    };
    order.then(a.id.cmp(&b.id))
}
/// Primary country-name sorting is indexed, so it does not trigger detail reads.
/// Missing nationality is always last, including when reversing known countries.
pub fn compare_nationalities(a: &Entity, b: &Entity, descending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let an = a.nationality.trim();
    let bn = b.nationality.trim();
    let order = match (an.is_empty(), bn.is_empty()) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (true, true) => Ordering::Equal,
        (false, false) => {
            let order = an.cmp(bn);
            if descending { order.reverse() } else { order }
        }
    };
    order.then(a.id.cmp(&b.id))
}
/// Rebuild derived scores from complete inputs so old snapshots use current formulas.
/// Incomplete profiles cannot retain the legacy loan score, which omitted negotiation.
pub fn ensure_scores(detail: &mut Detail) {
    const INPUTS: &[&str] = &[
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
    ];
    if detail.entity.kind != EntityKind::Staff {
        return;
    }
    if INPUTS.iter().all(|key| {
        detail
            .fields
            .iter()
            .any(|field| field.key == *key && field.value.is_finite())
    }) {
        detail
            .fields
            .retain(|field| !field.key.starts_with("score_"));
        crate::fields::append_scores(detail);
    } else {
        // Preserve unrelated legacy scores, but never substitute zero for a
        // missing input to the revised loan-manager recommendation formula.
        detail
            .fields
            .retain(|field| field.key != "score_loan_manager");
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub key: String,
    pub operation: String,
    pub value: f64,
}
pub fn edits(d: &Detail, rules: &[Rule]) -> Result<Vec<Edit>> {
    ensure!(
        !rules.is_empty() && rules.len() <= 256,
        "변경 규칙은 1–256개입니다"
    );
    let mut keys = HashSet::new();
    let mut result = vec![];
    for r in rules {
        ensure!(keys.insert(&r.key), "중복 규칙: {}", r.key);
        ensure!(r.value.is_finite(), "유한한 숫자를 입력하세요");
        let f = d
            .fields
            .iter()
            .find(|f| f.key == r.key)
            .with_context(|| format!("{}: {} 필드 없음", d.entity.name, r.key))?;
        ensure!(f.binding.is_some(), "{}은 읽기 전용입니다", f.label);
        let v = match r.operation.as_str() {
            "set" => r.value,
            "add" => f.value + r.value,
            "percent" => f.value * (1. + r.value / 100.),
            "at_least" => f.value.max(r.value),
            "at_most" => f.value.min(r.value),
            _ => anyhow::bail!("지원하지 않는 연산: {}", r.operation),
        };
        ensure!(
            v.is_finite() && v >= f.min && v <= f.max,
            "{} · {} 결과 {}가 {}–{} 범위를 벗어납니다",
            d.entity.name,
            f.label,
            v,
            f.min,
            f.max
        );
        // Explicit percentage operations snap to the field's representable step.
        let value = if r.operation == "percent" {
            f.min + ((v - f.min) / f.step).round() * f.step
        } else {
            v
        };
        result.push(Edit {
            key: r.key.clone(),
            value,
        });
    }
    crate::transaction::prepare_allow_unchanged(d, &result)?;
    Ok(result)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct Ref {
    pub kind: EntityKind,
    pub id: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shortlist {
    pub name: String,
    pub members: Vec<Ref>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub kind: EntityKind,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Library {
    pub lists: Vec<Shortlist>,
    pub presets: Vec<Preset>,
}
impl Library {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.lists.len() <= 100 && self.presets.len() <= 100,
            "목록/프리셋은 각각 최대 100개입니다"
        );
        fn names<'a>(items: impl Iterator<Item = &'a str>) -> Result<()> {
            let mut names = HashSet::new();
            for n in items {
                ensure!(
                    !n.trim().is_empty()
                        && n.chars().count() <= 80
                        && !n.chars().any(char::is_control),
                    "이름은 1–80자입니다"
                );
                ensure!(names.insert(n.to_lowercase()), "중복 이름: {n}");
            }
            Ok(())
        }
        names(self.lists.iter().map(|l| l.name.as_str()))?;
        names(self.presets.iter().map(|p| p.name.as_str()))?;
        for l in &self.lists {
            ensure!(l.members.len() <= 10000, "명단은 최대 10,000명입니다");
            let mut seen = HashSet::new();
            for r in &l.members {
                ensure!(r.id > 0 && seen.insert(r), "명단 ID가 없거나 중복입니다");
            }
        }
        for p in &self.presets {
            ensure!(
                !p.rules.is_empty() && p.rules.len() <= 256,
                "프리셋 항목은 1–256개입니다"
            );
            let mut seen = HashSet::new();
            for r in &p.rules {
                ensure!(
                    !r.key.is_empty()
                        && r.key.len() <= 100
                        && seen.insert(&r.key)
                        && r.value.is_finite(),
                    "프리셋 필드 오류"
                );
                ensure!(
                    ["set", "add", "percent", "at_least", "at_most"]
                        .contains(&r.operation.as_str()),
                    "프리셋 연산 오류"
                );
            }
        }
        Ok(())
    }
    pub fn read(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        ensure!(
            path.metadata()?.len() < 4 * 1024 * 1024,
            "라이브러리 파일이 너무 큽니다"
        );
        let l: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        l.validate()?;
        Ok(l)
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        atomic_write(path, &serde_json::to_vec_pretty(self)?)
    }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.pending", std::process::id()));
    let mut f = std::fs::File::create(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
        }
        let a: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        ensure!(
            unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0x1 | 0x8) } != 0,
            "기록 교체 실패: {}",
            std::io::Error::last_os_error()
        );
    }
    #[cfg(not(windows))]
    std::fs::rename(temp, path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_reversed_ranges() {
        assert!(
            Filter {
                ca_min: Some(180),
                ca_max: Some(100),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn rejects_unknown_preset_operation() {
        let l = Library {
            presets: vec![Preset {
                name: "x".into(),
                kind: EntityKind::Player,
                rules: vec![Rule {
                    key: "ca".into(),
                    operation: "write_address".into(),
                    value: 1.,
                }],
            }],
            ..Default::default()
        };
        assert!(l.validate().is_err());
    }
    #[test]
    fn ignores_no_unknown_pointer_keys() {
        assert!(serde_json::from_str::<Ref>(r#"{"kind":"Player","id":1,"address":100}"#).is_err());
    }
    #[test]
    fn validates_list_identity_not_name() {
        let l = Library {
            lists: vec![Shortlist {
                name: "유망주".into(),
                members: vec![
                    Ref {
                        kind: EntityKind::Player,
                        id: 3,
                    },
                    Ref {
                        kind: EntityKind::Staff,
                        id: 3,
                    },
                ],
            }],
            ..Default::default()
        };
        assert!(l.validate().is_ok());
    }
    #[test]
    fn rejects_duplicate_members() {
        let r = Ref {
            kind: EntityKind::Player,
            id: 3,
        };
        let l = Library {
            lists: vec![Shortlist {
                name: "유망주".into(),
                members: vec![r.clone(), r],
            }],
            ..Default::default()
        };
        assert!(l.validate().is_err());
    }
    fn detail() -> Detail {
        Detail {
            entity: Entity {
                nationality: String::new(),
                nationality_id: None,
                football_category: crate::model::FootballCategory::Unknown,
                id: 7,
                kind: EntityKind::Player,
                name: "Fixture".into(),
                english_name: String::new(),
                club: String::new(),
                club_id: None,
                age: None,
                ca: Some(100),
                pa: Some(150),
                role: String::new(),
                group: String::new(),
                address: 0x10000,
                base: 0x10000,
                index: Some(7),
            },
            fields: [("ca", 100u16), ("pa", 150u16)]
                .into_iter()
                .enumerate()
                .map(|(n, (key, v))| Field {
                    key: key.into(),
                    label: key.into(),
                    section: String::new(),
                    value: v as f64,
                    min: 1.,
                    max: 200.,
                    step: 1.,
                    hint: String::new(),
                    binding: Some(Binding {
                        address: 0x20000 + n as u64 * 2,
                        storage: Storage::U16,
                        scale: 1.,
                        original: v.to_le_bytes().to_vec(),
                    }),
                })
                .collect(),
            guards: vec![],
            notes: vec![],
        }
    }
    #[test]
    fn tactical_position_rank_handles_compounds_exact_tokens_and_staff_jobs() {
        let mut e = detail().entity;
        for role in [
            "ST · DC",
            "DC · ST",
            "ST/DC",
            "DC,ST",
            "st\t dc\n",
            "unknown / DC",
        ] {
            e.role = role.into();
            assert_eq!(player_position_rank(&e), Some(3), "{role}");
        }
        for role in ["", "   ", "STRIKER", "STX", "AM", "COACH"] {
            e.role = role.into();
            assert_eq!(player_position_rank(&e), None, "{role}");
        }
        e.kind = EntityKind::Staff;
        for role in ["GK", "ST", "GK 코치", "AMC / MC"] {
            e.role = role.into();
            assert_eq!(player_position_rank(&e), None, "staff: {role}");
        }
    }
    #[test]
    fn position_selection_uses_any_numeric_proficiency_as_one_expression_operand() {
        let mut d=detail();
        d.entity.role="GK".into(); // Display text must not decide a proficiency match.
        for (key,value) in [("position_dl",20.),("position_dr",1.)] {
            let mut field=d.fields[0].clone();field.key=key.into();field.value=value;field.min=0.;field.max=20.;d.fields.push(field);
        }
        let filter:Filter=serde_json::from_value(serde_json::json!({"conditions":[
            {"source":"position","key":"any","op":"gte","value":20,"positions":["position_dl","position_dr"]},
            {"source":"entity","key":"ca","op":"gte","value":100}
        ]})).unwrap();
        filter.validate_catalog(&d.fields).unwrap();
        assert!(filter.matches(&d.entity) && !filter.proven_without_detail(&d.entity));
        assert!(filter.matches_detail(&d));
        assert_eq!(filter.condition_field_keys().collect::<Vec<_>>(),vec!["position_dl","position_dr"]);
        d.entity.ca=Some(99);assert!(!filter.matches_detail(&d)); // No OR leaks around CA.
        d.entity.ca=Some(100);d.fields.iter_mut().find(|f|f.key=="position_dl").unwrap().value=19.;
        assert!(!filter.matches_detail(&d));
        d.fields.iter_mut().find(|f|f.key=="position_dr").unwrap().value=20.;assert!(filter.matches_detail(&d));
        d.fields.retain(|f|!f.key.starts_with("position_"));assert!(!filter.matches_detail(&d));
        assert!(filter.validate_catalog(&d.fields).is_err());
        d.entity.kind=EntityKind::Staff;assert!(!filter.matches(&d.entity));
        let restored:Filter=serde_json::from_value(serde_json::to_value(&filter).unwrap()).unwrap();
        assert_eq!(restored.condition_field_keys().collect::<Vec<_>>(),filter.condition_field_keys().collect::<Vec<_>>());
        let legacy:Filter=serde_json::from_value(serde_json::json!({"conditions":[{"source":"field","key":"ca","op":"gte","value":100}]})).unwrap();
        assert!(legacy.validate().is_ok());
        assert!(serde_json::to_value(legacy).unwrap()["conditions"][0].get("positions").is_none());
    }
    #[test]
    fn position_selection_rejects_unverified_keys_and_invalid_thresholds() {
        let valid=serde_json::json!({"source":"position","key":"any","op":"gte","value":20,"positions":["position_dc"]});
        for (name,value) in [("value",serde_json::json!(0)),("value",serde_json::json!(21)),("value",serde_json::json!(19.5)),("op",serde_json::json!("ne")),("positions",serde_json::json!([])),("positions",serde_json::json!(["position_dc","position_dc"])),("positions",serde_json::json!(["role"])),("positions",serde_json::json!(["position_unknown"]))] {
            let mut row=valid.clone();row[name]=value;
            let condition:SearchCondition=serde_json::from_value(row).unwrap();assert!(condition.validate().is_err(),"{name}");
        }
    }
    #[test]
    fn current_value_rules_remain_valid_desired_state() {
        let d = detail();
        for (operation, value) in [
            ("set", 100.),
            ("add", 0.),
            ("percent", 0.),
            ("at_least", 90.),
            ("at_most", 110.),
        ] {
            let e = edits(
                &d,
                &[Rule {
                    key: "ca".into(),
                    operation: operation.into(),
                    value,
                }],
            )
            .unwrap();
            assert_eq!(e.len(), 1);
            assert_eq!(e[0].value, 100.);
            assert!(
                crate::transaction::prepare_allow_unchanged(&d, &e)
                    .unwrap()
                    .is_empty()
            );
            assert!(crate::transaction::prepare(&d, &e).is_err());
        }
    }
    #[test]
    fn football_category_is_exact_and_legacy_snapshots_remain_unknown() {
        let original = detail().entity;
        let mut serialized = serde_json::to_value(&original).unwrap();
        serialized
            .as_object_mut()
            .unwrap()
            .remove("football_category");
        let legacy: Entity = serde_json::from_value(serialized).unwrap();
        assert_eq!(legacy.football_category, FootballCategory::Unknown);
        let all: Filter = serde_json::from_value(serde_json::json!({})).unwrap();
        let men: Filter =
            serde_json::from_value(serde_json::json!({"football_category":"men"})).unwrap();
        let women: Filter =
            serde_json::from_value(serde_json::json!({"football_category":"women"})).unwrap();
        assert!(all.matches(&legacy));
        assert!(!men.matches(&legacy));
        assert!(!women.matches(&legacy));
        for kind in [EntityKind::Player, EntityKind::Staff, EntityKind::Club] {
            let mut e = original.clone();
            e.kind = kind;
            // Identical name, age, and role must not affect classification.
            e.football_category = FootballCategory::Men;
            assert!(men.matches(&e) && !women.matches(&e));
            e.football_category = FootballCategory::Women;
            assert!(women.matches(&e) && !men.matches(&e));
        }
        assert!(
            serde_json::from_value::<Filter>(serde_json::json!({"football_category":"male"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<Filter>(serde_json::json!({"football_category":1})).is_err()
        );
    }
    #[test]
    fn nationality_filter_is_exact_primary_id_and_preserves_legacy_filters() {
        let mut e = detail().entity;
        e.nationality = "대한민국".into();
        e.nationality_id = Some(776);
        e.club = "대한민국".into();
        e.club_id = Some(776);
        let selected: Filter = serde_json::from_value(serde_json::json!({"nationality_id":776})).unwrap();
        assert!(selected.validate().is_ok());
        for kind in [EntityKind::Player, EntityKind::Staff, EntityKind::Person] {
            e.kind = kind;
            assert!(selected.matches(&e));
            // A translated or absent label does not change a known identity.
            e.nationality = String::new();
            assert!(selected.matches(&e));
            e.nationality = "대한민국".into();
            e.nationality_id = Some(777);
            assert!(!selected.matches(&e));
            e.nationality_id = None;
            assert!(!selected.matches(&e));
            for value in [serde_json::json!({}), serde_json::json!({"nationality_id":null})] {
                let legacy: Filter = serde_json::from_value(value).unwrap();
                assert!(legacy.matches(&e));
            }
            e.nationality_id = Some(776);
        }
        for kind in [EntityKind::Club, EntityKind::City] {
            e.kind = kind;
            assert!(!selected.matches(&e));
        }
        let restored: Filter = serde_json::from_value(serde_json::to_value(&selected).unwrap()).unwrap();
        assert_eq!(restored.nationality_id, Some(776));
    }
    #[test]
    fn nationality_filter_rejects_invalid_id_values() {
        let zero: Filter = serde_json::from_value(serde_json::json!({"nationality_id":0})).unwrap();
        assert!(zero.validate().is_err());
        for invalid in [serde_json::json!(-1), serde_json::json!(1.5), serde_json::json!("776"), serde_json::json!(4294967296u64)] {
            assert!(serde_json::from_value::<Filter>(serde_json::json!({"nationality_id":invalid})).is_err());
        }
    }
    #[test]
    fn no_op_mode_still_validates_readonly_steps_and_final_ability_pair() {
        let mut d = detail();
        assert!(
            edits(
                &d,
                &[Rule {
                    key: "ca".into(),
                    operation: "set".into(),
                    value: 100.5
                }]
            )
            .is_err()
        );
        d.fields[0].binding = None;
        assert!(
            edits(
                &d,
                &[Rule {
                    key: "ca".into(),
                    operation: "set".into(),
                    value: 100.
                }]
            )
            .is_err()
        );
        let mut d = detail();
        d.fields[1].value = 90.;
        d.fields[1].binding.as_mut().unwrap().original = 90u16.to_le_bytes().to_vec();
        assert!(
            edits(
                &d,
                &[Rule {
                    key: "ca".into(),
                    operation: "set".into(),
                    value: 100.
                }]
            )
            .is_err()
        );
    }
    #[test]
    fn indexed_filters_use_inclusive_bounds_exact_role_tokens_and_employment() {
        let mut e = detail().entity;
        e.name = "이 강인".into();
        e.english_name = "Kang In Lee".into();
        e.club = "Paris Saint-Germain".into();
        e.club_id = Some(44);
        e.role = "AMC · MC".into();
        e.group = "1군".into();
        e.age = Some(24);
        let f = Filter {
            age_min: Some(24),
            age_max: Some(24),
            ca_min: Some(100),
            ca_max: Some(100),
            pa_min: Some(150),
            pa_max: Some(150),
            role: " mc ".into(),
            club_query: "GERMAIN  paris".into(),
            group: Some(" 1군 ".into()),
            employed: true,
            ..Default::default()
        };
        assert!(f.validate().is_ok());
        assert!(f.matches(&e));
        e.role = "AMC".into();
        assert!(!f.matches(&e));
        e.role = "MC".into();
        e.group = "U21".into();
        assert!(!f.matches(&e));
        e.group = "1군".into();
        e.club_id = None;
        assert!(!f.matches(&e));
        assert!(
            Filter {
                unemployed: true,
                ..Default::default()
            }
            .matches(&e)
        );
        assert!(
            Filter {
                employed: true,
                unemployed: true,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Filter {
                age_max: Some(130),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Filter {
                pa_max: Some(201),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn name_search_normalizes_unicode_whitespace_case_and_multiple_tokens() {
        let mut e = detail().entity;
        e.name = "이 강인".into();
        e.english_name = "Kang In Lee".into();
        for query in ["강인\u{2003}이", "LEE\tKang", " 7 ", ""] {
            assert!(name_matches(&e, query), "{query}");
        }
        for query in ["71", "이 손흥민", "Kang Missing"] {
            assert!(!name_matches(&e, query), "{query}");
        }
        assert!(role_matches("수석  코치", "수석\t코치"));
        assert!(!role_matches("수석 코치", "코치"));
    }
    #[test]
    fn numeric_filters_reject_unknown_keys_duplicates_and_out_of_catalog_bounds() {
        let d = detail();
        let range = |key: &str, min, max| Filter {
            fields: vec![Range {
                key: key.into(),
                min,
                max,
            }],
            ..Default::default()
        };
        assert!(
            range("ca", Some(100.), Some(100.))
                .validate_catalog(&d.fields)
                .is_ok()
        );
        assert!(range("ca", Some(100.), Some(100.)).matches_detail(&d));
        assert!(!range("ca", Some(101.), None).matches_detail(&d));
        assert!(
            range("unknown", None, None)
                .validate_catalog(&d.fields)
                .is_err()
        );
        assert!(
            range("ca", Some(0.), None)
                .validate_catalog(&d.fields)
                .is_err()
        );
        assert!(
            range("ca", None, Some(201.))
                .validate_catalog(&d.fields)
                .is_err()
        );
        let mut duplicate = range("ca", None, None);
        duplicate.fields.push(duplicate.fields[0].clone());
        assert!(duplicate.validate().is_err());
        let mut invalid = d;
        invalid.fields[0].value = f64::NAN;
        assert!(!range("ca", None, None).matches_detail(&invalid));
    }
    #[test]
    fn partial_staff_snapshot_does_not_invent_missing_score_inputs() {
        let mut d = detail();
        d.entity.kind = EntityKind::Staff;
        ensure_scores(&mut d);
        assert!(d.fields.iter().all(|f| !f.key.starts_with("score_")));
    }
    #[test]
    fn complete_staff_snapshot_refreshes_stale_scores_without_duplicates() {
        let mut d = crate::staff_review::tests::staff(1, None, "임대 매니저", "무소속", 15., 4000.);
        for field in &mut d.fields {
            if field.key.starts_with("score_") {
                field.value = 999.;
            } else if field.key == "staff_negotiation" {
                field.value = 20.;
            }
        }
        ensure_scores(&mut d);
        let loan = d
            .fields
            .iter()
            .find(|f| f.key == "score_loan_manager")
            .unwrap();
        let expected = (225. + 20. * 4.) * 300. / 380.;
        assert!((loan.value - expected).abs() < 1e-9);
        assert_eq!(
            d.fields
                .iter()
                .find(|f| f.key == "score_general")
                .unwrap()
                .value,
            225.
        );
        ensure_scores(&mut d);
        let scores = d
            .fields
            .iter()
            .filter(|f| f.key.starts_with("score_"))
            .collect::<Vec<_>>();
        assert_eq!(scores.len(), 27);
        assert_eq!(
            scores.iter().map(|f| &f.key).collect::<HashSet<_>>().len(),
            27
        );
        assert!(scores.iter().all(|f| (0. ..=300.).contains(&f.value)));
    }
    #[test]
    fn incomplete_staff_snapshot_discards_legacy_loan_score_only() {
        let original =
            crate::staff_review::tests::staff(1, None, "임대 매니저", "무소속", 15., 4000.);
        for missing in [true, false] {
            let mut d = original.clone();
            if missing {
                d.fields.retain(|f| f.key != "staff_negotiation");
            } else {
                d.fields
                    .iter_mut()
                    .find(|f| f.key == "staff_negotiation")
                    .unwrap()
                    .value = f64::NAN;
            }
            let unrelated = d
                .fields
                .iter()
                .filter(|f| f.key.starts_with("score_") && f.key != "score_loan_manager")
                .map(|f| (f.key.clone(), f.value))
                .collect::<Vec<_>>();
            ensure_scores(&mut d);
            assert!(!d.fields.iter().any(|f| f.key == "score_loan_manager"));
            for (key, value) in unrelated {
                assert_eq!(d.fields.iter().find(|f| f.key == key).unwrap().value, value);
            }
        }
    }
    #[test]
    fn numeric_filter_limit_accepts_sixty_four_and_rejects_sixty_five_or_duplicates() {
        let mut filter = Filter {
            fields: (0..64)
                .map(|n| Range {
                    key: format!("fixture_field_{n}"),
                    min: Some(1.),
                    max: Some(20.),
                })
                .collect(),
            ..Default::default()
        };
        assert!(filter.validate().is_ok());
        filter.fields.push(Range {
            key: "fixture_field_64".into(),
            min: None,
            max: None,
        });
        assert!(filter.validate().unwrap_err().to_string().contains("64"));
        filter.fields.pop();
        filter.fields[63].key = filter.fields[0].key.clone();
        let error = filter.validate().unwrap_err().to_string();
        assert!(error.contains("중복"));
        assert!(error.contains("fixture_field_0"));
    }
}
