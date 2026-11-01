//! Typed search predicates. Missing data never becomes zero or an inequality match.
use super::{normalize, tokens_match};
use crate::model::{Detail, Entity, EntityKind, Field};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source { #[default] Entity, Field, Position }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Join { #[default] And, Or }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Op { #[default] Eq, Ne, Gte, Lte, Gt, Lt, Between, Contains, NotContains }
fn enabled() -> bool { true }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchCondition {
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub join: Join,
    pub source: Source,
    pub key: String,
    pub op: Op,
    pub value: Value,
    #[serde(default)]
    pub value2: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions: Vec<String>,
}
#[derive(Clone, Copy)]
enum Kind { Number(f64, f64), Text, Boolean }
fn entity_kind(key: &str) -> Option<Kind> {
    Some(match key {
        "id" | "nationality_id" => Kind::Number(1., u32::MAX as f64),
        "age" => Kind::Number(0., 129.),
        "ca" | "pa" => Kind::Number(0., 200.),
        "name" | "club" | "role" | "group" | "football_category" => Kind::Text,
        "employed" => Kind::Boolean,
        _ => return None,
    })
}
impl SearchCondition {
    pub fn field_keys(&self) -> impl Iterator<Item = &str> {
        (self.enabled && self.source == Source::Field).then_some(self.key.as_str()).into_iter()
            .chain(self.positions.iter().filter(|_| self.enabled && self.source == Source::Position).map(String::as_str))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.key.is_empty() && self.key.len() <= 100, "상세 조건 필드 이름 오류");
        if self.source == Source::Position {
            const KEYS: [&str;14] = ["position_gk","position_dl","position_dc","position_dr","position_wbl","position_dm","position_wbr","position_ml","position_mc","position_mr","position_aml","position_amc","position_amr","position_st"];
            ensure!(self.key == "any" && self.op == Op::Gte && self.value2.is_none(), "포지션 조건은 최소 숙련도만 지원합니다");
            let minimum = self.value.as_f64().context("포지션 숙련도 숫자가 필요합니다")?;
            ensure!(minimum.is_finite() && minimum.fract() == 0. && (1. ..=20.).contains(&minimum), "포지션 숙련도는 1–20 정수입니다");
            let mut unique = std::collections::HashSet::new();
            ensure!(!self.positions.is_empty() && self.positions.len() <= KEYS.len()
                && self.positions.iter().all(|key| KEYS.contains(&key.as_str()) && unique.insert(key)), "포지션 목록이 비어 있거나 중복·미지원 항목이 있습니다");
            return Ok(());
        }
        ensure!(self.positions.is_empty(), "포지션 목록은 포지션 조건에서만 사용할 수 있습니다");
        let kind = if self.source == Source::Field { Kind::Number(f64::MIN, f64::MAX) }
            else { entity_kind(&self.key).context("지원하지 않는 기본 검색 필드입니다")? };
        match kind {
            Kind::Number(min, max) => {
                ensure!(matches!(self.op, Op::Eq|Op::Ne|Op::Gte|Op::Lte|Op::Gt|Op::Lt|Op::Between), "숫자 조건의 비교 방식이 올바르지 않습니다");
                let v = self.value.as_f64().filter(|v| v.is_finite()).context("숫자 조건에는 유한한 숫자를 입력하세요")?;
                ensure!(v >= min && v <= max, "{} 검색 범위는 {}–{}입니다", self.key, min, max);
                if self.source == Source::Entity { ensure!(v.fract() == 0., "기본 숫자 조건에는 정수를 입력하세요"); }
                if self.op == Op::Between {
                    let hi = self.value2.filter(|v| v.is_finite()).context("범위 조건의 최댓값이 필요합니다")?;
                    ensure!(hi >= v && hi <= max, "상세 조건의 최소·최대 범위가 올바르지 않습니다");
                    if self.source == Source::Entity { ensure!(hi.fract() == 0., "기본 숫자 조건에는 정수를 입력하세요"); }
                } else { ensure!(self.value2.is_none(), "범위 외 조건에는 두 번째 값을 사용할 수 없습니다"); }
            },
            Kind::Text => {
                ensure!(matches!(self.op, Op::Eq|Op::Ne|Op::Contains|Op::NotContains), "문자 조건의 비교 방식이 올바르지 않습니다");
                let text = self.value.as_str().context("문자 검색어가 필요합니다")?;
                ensure!(!text.trim().is_empty() && text.chars().count() <= 200 && !text.chars().any(char::is_control), "문자 검색어는 1–200자입니다");
                ensure!(self.value2.is_none(), "문자 조건에는 두 번째 값을 사용할 수 없습니다");
                if self.key == "football_category" {
                    ensure!(["men", "women", "unknown"].contains(&text), "남축·여축 검색값이 올바르지 않습니다");
                }
            },
            Kind::Boolean => {
                ensure!(matches!(self.op, Op::Eq|Op::Ne) && self.value.is_boolean() && self.value2.is_none(), "소속 여부 조건은 참·거짓 비교만 지원합니다");
            },
        }
        Ok(())
    }
    pub fn validate_catalog(&self, catalog: &[Field]) -> Result<()> {
        if self.source == Source::Position {
            for key in &self.positions {
                let field = catalog.iter().find(|f| &f.key == key).with_context(|| format!("지원하지 않는 포지션 필드입니다: {key}"))?;
                ensure!(field.min <= 1. && field.max >= 20., "포지션 숙련도 범위를 확인하지 못했습니다: {key}");
            }
            return Ok(());
        }
        if self.source != Source::Field { return Ok(()); }
        let f = catalog.iter().find(|f| f.key == self.key)
            .with_context(|| format!("지원하지 않는 검색 필드입니다: {}", self.key))?;
        for v in self.value.as_f64().into_iter().chain(self.value2) {
            ensure!(v >= f.min && v <= f.max, "{} 검색 범위는 {}–{}입니다", f.label, f.min, f.max);
        }
        Ok(())
    }
    fn number(&self, value: Option<f64>) -> bool {
        let Some(v) = value.filter(|v| v.is_finite()) else { return false; };
        let Some(target) = self.value.as_f64() else { return false; };
        match self.op {
            Op::Eq => v == target, Op::Ne => v != target, Op::Gte => v >= target,
            Op::Lte => v <= target, Op::Gt => v > target, Op::Lt => v < target,
            Op::Between => v >= target && self.value2.is_some_and(|hi| v <= hi),
            _ => false,
        }
    }
    fn text(&self, value: &str) -> bool {
        if value.trim().is_empty() { return false; }
        let Some(target) = self.value.as_str() else { return false; };
        match self.op {
            Op::Eq => normalize(value) == normalize(target),
            Op::Ne => normalize(value) != normalize(target),
            Op::Contains => tokens_match(value, target),
            Op::NotContains => !tokens_match(value, target),
            _ => false,
        }
    }
    fn evaluate(&self, e: &Entity, detail: Option<&Detail>) -> Option<bool> {
        if self.source == Source::Position {
            if e.kind != EntityKind::Player { return Some(false); }
            return detail.map(|d| self.positions.iter().any(|key| self.number(d.fields.iter().find(|f| &f.key == key).map(|f| f.value))));
        }
        if self.source == Source::Field {
            return detail.map(|d| self.number(d.fields.iter().find(|f| f.key == self.key).map(|f| f.value)));
        }
        if e.employment().is_none() && matches!(self.key.as_str(),"employed"|"club"|"role"|"group") {return Some(false);}
        Some(match self.key.as_str() {
            "id" => self.number(Some(e.id as f64)),
            "nationality_id" => self.number(e.nationality_id.map(|v| v as f64)),
            "age" => self.number(e.age.map(|v| v as f64)),
            "ca" => self.number(e.ca.map(|v| v as f64)), "pa" => self.number(e.pa.map(|v| v as f64)),
            "name" => {
                let matches = if matches!(self.op, Op::Contains|Op::NotContains) {
                    tokens_match(&format!("{} {}", e.name, e.english_name), self.value.as_str().unwrap_or(""))
                } else {
                    [e.name.as_str(), e.english_name.as_str()].iter().any(|name| !name.trim().is_empty() && normalize(name) == normalize(self.value.as_str().unwrap_or("")))
                };
                !(e.name.is_empty() && e.english_name.is_empty()) && if matches!(self.op, Op::Ne|Op::NotContains) { !matches } else { matches }
            },
            "club" => self.text(&e.club), "role" => self.text(&e.role), "group" => self.text(&e.group),
            "football_category" => self.text(e.football_category.key()),
            "employed" => e.employment().is_some_and(|employed|self.value.as_bool().is_some_and(|v| if self.op == Op::Ne { employed != v } else { employed == v })),
            _ => false,
        })
    }
}
// Three-valued evaluation preserves candidates until detailed fields are read.
// A false indexed AND operand can prune its group, but not a different OR group.
fn and(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a,b) { (Some(false),_)|(_,Some(false)) => Some(false), (Some(true),Some(true)) => Some(true), _ => None }
}
fn or(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a,b) { (Some(true),_)|(_,Some(true)) => Some(true), (Some(false),Some(false)) => Some(false), _ => None }
}
pub(super) fn evaluate(conditions: &[SearchCondition], e: &Entity, detail: Option<&Detail>) -> Option<bool> {
    let mut active = conditions.iter().filter(|c| c.enabled);
    let Some(first) = active.next() else { return Some(true); };
    let mut group = first.evaluate(e, detail);
    let mut result = Some(false);
    for c in active {
        if c.join == Join::Or { result = or(result, group); group = c.evaluate(e, detail); }
        else { group = and(group, c.evaluate(e, detail)); }
    }
    or(result, group)
}
