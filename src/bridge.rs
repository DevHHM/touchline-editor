//! Native WPF transport. Engine-owned identities, bindings and previews never
//! cross the process boundary; stdin is a semantic command stream, not a memory API.
use crate::{assets::NativeImagePaths, backend, events, fields, model::*, transaction};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
mod workspace;
mod relations;
mod staff_review;
mod name_localization;
mod connection;
#[cfg(feature = "fm27-health-experiment")]
mod health_experiment;
pub(crate) mod api;

const MAX_LINE: usize = 1024 * 1024;
const TOKEN_TTL: Duration = Duration::from_secs(600);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    command: String,
    #[serde(default)]
    args: Value,
}
enum Preview {
    Edit { detail: Detail, edits: Vec<Edit> },
    Event(events::EventPlan),
    Batch(Vec<(Detail, Vec<Edit>)>),
    Freeze(Vec<(Detail, Vec<Edit>)>),
    Relation(crate::relations::RelationPlan),
    NameLocalization(crate::name_localization::Plan),
    #[cfg(feature = "fm27-health-experiment")]
    InjuryExperiment { plan: backend::health::InjuryPlan, backup: Option<health_experiment::SaveBackup> },
    #[cfg(feature = "fm27-health-experiment")]
    BanExperiment { plan: backend::bans::BanPlan, backup: Option<health_experiment::SaveBackup> },
    #[cfg(feature = "fm27-transfer-experiment")]
    TransferExperiment { plan: backend::transfer_experiment::TransferPlan, backup: Option<health_experiment::SaveBackup> },
}
struct Pending {
    created: Instant,
    preview: Preview,
}

#[derive(Default)]
struct Bridge {
    index: Option<Index>,
    snapshot: Option<PathBuf>,
    live: bool,
    details: HashMap<(EntityKind, u32), Detail>,
    contexts: HashMap<u32, events::EventContext>,
    pending: HashMap<String, Pending>,
    // Preview/cache revision changes after edits; it is not a save identity.
    epoch: u64,
    connection_generation: u64,
    sequence: u64,
    images: Option<NativeImagePaths>,
    work: workspace::WorkState,
    relation_state: relations::RelationState,
    staff_review_state: staff_review::ReviewState,
    connection: connection::Monitor,
    #[cfg(test)]
    staff_refresh_hook:Option<fn(&mut Index)->Result<()>>,
}

fn entity(e: &Entity) -> Value {
    json!({"id":e.id,"kind":e.kind,"name":e.name,"english_name":e.english_name,
        "nationality":e.nationality,"nationality_id":e.nationality_id,
        "club":e.club,"club_id":e.club_id,"age":e.age,"ca":e.ca,"pa":e.pa,"role":e.role,"group":e.group,"football_category":e.football_category})
}
fn field(f: &Field) -> Value {
    json!({"key":f.key,"label":f.label,"section":f.section,"value":f.value,
        "min":f.min,"max":f.max,"step":f.step,"hint":f.hint,"editable":f.binding.is_some()})
}
fn change(c: &Change) -> Value {
    json!({"key":c.key,"label":c.label,"before":c.before,"after":c.after})
}
fn receipt(r: &Receipt) -> Value {
    json!({"id":r.id,"time":r.time,"status":r.status,"entity":entity(&r.entity),"changes":r.changes.iter().map(change).collect::<Vec<_>>()})
}
fn plan(p: &events::EventPlan, live: bool) -> Value {
    json!({"id":p.id,"draft":p.draft,"club":entity(&p.context.club.entity),
        "city":p.context.city.as_ref().map(|d|entity(&d.entity)),"deltas":p.deltas,
        "narrative":p.narrative,"warnings":p.warnings,"live":live,"can_apply":live})
}
fn event_receipt(r: &events::EventReceipt, live: bool) -> Value {
    json!({"id":r.id,"time":r.time,"status":r.status,"game_saved":r.game_saved,
        "engine_accrual_verified":r.engine_accrual_verified,"plan":plan(&r.plan,live)})
}
fn required_str<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("문자열 항목 필요: {key}"))
}
fn required_id(v: &Value, key: &str) -> Result<u32> {
    u32::try_from(
        v.get(key)
            .and_then(Value::as_u64)
            .with_context(|| format!("ID 항목 필요: {key}"))?,
    )
    .context("ID 범위 초과")
}
fn kind(v: &Value) -> Result<EntityKind> {
    Ok(serde_json::from_value(
        v.get("kind").cloned().context("종류를 선택하세요")?,
    )?)
}
fn identity(v: &Value) -> Result<(EntityKind, u32)> {
    Ok((kind(v)?, required_id(v, "id")?))
}
fn same_session(a: &Session, b: &Session) -> bool {
    a.pid == b.pid
        && a.created == b.created
        && a.module_base == b.module_base
        && a.module_stamp == b.module_stamp
        && a.module_size == b.module_size
        && a.db == b.db
        && a.registry_begin == b.registry_begin
}

impl Bridge {
    fn invalidate(&mut self) {
        self.details.clear();
        self.contexts.clear();
        self.pending.clear();
        self.work = workspace::WorkState::default();
        self.relation_state = relations::RelationState::default();
        self.staff_review_state = staff_review::ReviewState::default();
        self.epoch += 1;
    }
    fn replace_connection(&mut self) {
        self.invalidate();
        // Player and club IDs may be reused across editions. Never reuse an
        // image index after switching FM builds or loading another snapshot.
        self.images = None;
        self.connection_generation += 1;
        self.index = None;
        self.live = false;
        self.snapshot = None;
    }
    fn index(&self) -> Result<&Index> {
        self.index
            .as_ref()
            .context("FM에 연결하거나 저장된 조회 자료를 여세요.")
    }
    fn require_live(&self) -> Result<&Session> {
        ensure!(
            self.live,
            "저장된 조회 자료는 읽기 전용입니다. FM에 직접 연결한 뒤 다시 검토하세요."
        );
        Ok(&self.index()?.session)
    }
    fn find(&self, k: EntityKind, id: u32) -> Result<Entity> {
        let mut matches = self
            .index()?
            .entities
            .iter()
            .filter(|e| e.kind == k && e.id == id);
        let result = matches
            .next()
            .with_context(|| format!("대상을 찾을 수 없습니다: {k:?} {id}"))?;
        ensure!(
            matches.next().is_none(),
            "같은 종류와 ID의 대상이 중복되었습니다. 다시 연결하세요."
        );
        Ok(result.clone())
    }
    fn summary(&self) -> Value {
        let mut counts = serde_json::Map::new();
        for k in EntityKind::ALL {
            counts.insert(
                format!("{k:?}"),
                json!(
                    self.index.as_ref().map_or(0, |i| i
                        .entities
                        .iter()
                        .filter(|e| e.kind == k)
                        .count())
                ),
            );
        }
        match &self.index {
            Some(i) => {
                json!({"connected":true,"live":self.live,"own_club":i.entities.iter().find(|e|e.kind==EntityKind::Club && Some(e.id)==i.session.own_club_id).map(entity),
                "game_date":i.session.game_date,"counts":counts,"pid":if self.live {Some(i.session.pid)} else {None},"profile":i.session.profile,"warnings":i.warnings,
                "build_capabilities":crate::compatibility::capabilities(&i.session),
                "diagnostic_context":if self.live {Some(backend::watch::DiagnosticContext::from_pe(i.session.module_stamp,i.session.module_size))} else {None}})
            }
            None => {
                json!({"connected":false,"live":false,"own_club":null,"game_date":null,"counts":counts,"pid":null,"profile":backend::PROFILE,"warnings":[],
                    "diagnostic_context":self.connection.diagnostic_context})
            }
        }
    }
    fn load(&mut self, path: &Path) -> Result<Value> {
        self.connection = connection::Monitor::default();
        self.replace_connection();
        let absolute = std::fs::canonicalize(path).context("조회 자료를 찾지 못했습니다")?;
        let index: Index = serde_json::from_slice(&std::fs::read(&absolute)?)?;
        self.index = Some(index);
        self.snapshot = Some(absolute);
        Ok(self.summary())
    }
    fn detail_json(&mut self, d: &Detail) -> Value {
        let profile = self.index.as_ref().map(|i| i.session.profile.as_str()).unwrap_or(backend::PROFILE);
        let paths = self.images.get_or_insert_with(|| NativeImagePaths::load_for_profile(profile));
        json!({"entity":entity(&d.entity),"fields":d.fields.iter().filter(|f|!f.key.starts_with("score_")).map(field).collect::<Vec<_>>(),
            "scores":d.fields.iter().filter(|f|f.key.starts_with("score_")).map(field).collect::<Vec<_>>(),"notes":d.notes,
            "image_path":paths.path(d.entity.kind,d.entity.id),
            "badge_path":d.entity.club_id.or_else(||(d.entity.kind==EntityKind::Club).then_some(d.entity.id)).and_then(|id|paths.path(EntityKind::Club,id)),
            "city_id":d.fields.iter().find(|f|f.key=="club.city_id").map(|f|f.value as u32),"live":self.live})
    }
    fn read_detail(&mut self, k: EntityKind, id: u32) -> Result<Value> {
        let e = self.find(k, id)?;
        let mut d = if self.live {
            backend::detail(&self.index()?.session, &e)?
        } else {
            load_snapshot_detail(
                self.snapshot.as_ref().context("조회 자료 경로 없음")?,
                k,
                id,
            )?
        };
        self.remember_staff_affiliation(&mut d.entity);
        // A new detail baseline invalidates existing apply tokens for this entity.
        self.pending.retain(|_,p|!matches!(&p.preview,Preview::Edit{detail,..} if detail.entity.kind==k && detail.entity.id==id));
        let result = self.detail_json(&d);
        self.details.insert((k, id), d);
        Ok(result)
    }
    fn refresh_staff_metadata(&mut self, kind:EntityKind)->Result<()> {
        if self.live && kind==EntityKind::Staff {
            #[cfg(test)]
            if let Some(refresh)=self.staff_refresh_hook {return refresh(self.index.as_mut().context("연결 없음")?);}
            backend::refresh_staff_affiliations(self.index.as_mut().context("연결 없음")?)?;
        }
        Ok(())
    }
    fn remember_staff_affiliation(&mut self, entity:&mut Entity) {
        if !self.live || entity.kind!=EntityKind::Staff {return;}
        let Some(index)=self.index.as_mut() else {return;};
        if let Some(club)=index.entities.iter().find(|e|e.kind==EntityKind::Club&&Some(e.id)==entity.club_id) {
            entity.football_category=club.football_category;
        }
        if let Some(stored)=index.entities.iter_mut().find(|e|e.kind==EntityKind::Staff&&e.id==entity.id&&e.address==entity.address&&e.base==entity.base&&e.index==entity.index) {
            stored.club_id=entity.club_id;stored.club=entity.club.clone();stored.role=entity.role.clone();
            stored.group=entity.group.clone();stored.football_category=entity.football_category;
        }
    }
    fn token(&mut self, preview: Preview) -> String {
        self.pending.retain(|_, p| p.created.elapsed() < TOKEN_TTL);
        if self.pending.len() >= 100 {
            self.pending.clear();
        }
        self.sequence += 1;
        let token = format!("{}-{}-{}", std::process::id(), self.epoch, self.sequence);
        self.pending.insert(
            token.clone(),
            Pending {
                created: Instant::now(),
                preview,
            },
        );
        token
    }
    fn consume(&mut self, token: &str) -> Result<Preview> {
        let p = self
            .pending
            .remove(token)
            .context("검토 결과가 없거나 이미 사용되었습니다. 다시 검토하세요.")?;
        ensure!(
            p.created.elapsed() < TOKEN_TTL,
            "검토 유효 시간이 지났습니다. 다시 검토하세요."
        );
        Ok(p.preview)
    }
    fn search(&mut self, a: &Value) -> Result<Value> {
        let filter: crate::workspace::Filter = serde_json::from_value(a.get("filter").cloned().unwrap_or(json!({})))?;
        filter.validate()?;
        ensure!(filter.fields.is_empty() && filter.condition_field_keys().next().is_none(),"능력치 조건 검색은 고급 검색에서 실행하세요");
        let k = match a.get("kind").filter(|v| !v.is_null()) {
            Some(v) => Some(serde_json::from_value::<EntityKind>(v.clone())?),
            None => None,
        };
        let query = a
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let club = a
            .get("club_id")
            .filter(|v| !v.is_null())
            .map(|_| required_id(a, "club_id"))
            .transpose()?;
        let group = a.get("group").and_then(Value::as_str).unwrap_or("");
        if a.get("offset").and_then(Value::as_u64).unwrap_or(0)==0 && (k==Some(EntityKind::Staff)||k.is_none()) {
            self.refresh_staff_metadata(EntityKind::Staff)?;
        }
        // A contract's team is not necessarily the player's active squad.
        // Refresh one selected club at the start of a paged roster read; later
        // pages use that same index rather than regrouping halfway through it.
        if self.live && a.get("offset").and_then(Value::as_u64).unwrap_or(0) == 0
            && matches!(k, Some(EntityKind::Player))
        {
            if let Some(club_id) = club {
                backend::refresh_club_squad(self.index.as_mut().context("FM에 연결하세요")?, club_id)?;
            }
        }
        let mut rows: Vec<&Entity> = self
            .index()?
            .entities
            .iter()
            .filter(|e| {
                filter.matches(e) && k.is_none_or(|k| e.kind == k)
                    && club.is_none_or(|id| e.club_id == Some(id))
                    && (group.is_empty() || e.group == group)
                    && (query.is_empty()
                        || e.name.to_lowercase().contains(&query)
                        || e.english_name.to_lowercase().contains(&query)
                        || e.id.to_string() == query)
            })
            .collect();
        let sort = a.get("sort").and_then(Value::as_str).unwrap_or("name");
        ensure!(
            ["name", "ca", "pa", "age", "club", "role", "nationality"].contains(&sort),
            "지원하지 않는 정렬 항목"
        );
        let descending = a.get("descending")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if sort == "role" {
            rows.sort_by(|a, b| crate::workspace::compare_roles(a, b, descending));
        } else if sort == "nationality" {
            rows.sort_by(|a, b| crate::workspace::compare_nationalities(a, b, descending));
        } else {
            rows.sort_by(|a, b| {
                match sort {
                    "ca" => a.ca.cmp(&b.ca),
                    "pa" => a.pa.cmp(&b.pa),
                    "age" => a.age.cmp(&b.age),
                    "club" => a.club.cmp(&b.club),
                    _ => a.name.cmp(&b.name),
                }
                .then(a.id.cmp(&b.id))
            });
            if descending {
                rows.reverse();
            }
        }
        let total = rows.len();
        let offset = a
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(usize::MAX as u64) as usize;
        let limit = a
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(100)
            .clamp(1, 500) as usize;
        Ok(
            json!({"items":rows.into_iter().skip(offset).take(limit).map(entity).collect::<Vec<_>>(),"total":total,"offset":offset,"limit":limit}),
        )
    }
    fn event_context(&mut self, id: u32) -> Result<Value> {
        let club = self.find(EntityKind::Club, id)?;
        let ctx = if self.live {
            events::capture(&self.index()?.session, &club)?
        } else {
            let folder = self
                .snapshot
                .as_ref()
                .and_then(|p| p.parent())
                .context("조회 자료 경로 없음")?;
            let paths = [
                folder.join(format!("event-context-{id}.json")),
                folder.join("event-context.json"),
            ];
            let mut found = None;
            for p in paths {
                if let Ok(bytes) = std::fs::read(p) {
                    let c: events::EventContext = serde_json::from_slice(&bytes)?;
                    if c.club.entity.kind == EntityKind::Club && c.club.entity.id == id {
                        found = Some(c);
                        break;
                    }
                }
            }
            found.context("이 구단의 저장된 이벤트 자료가 없습니다. FM에 연결해 읽어 주세요.")?
        };
        self.pending.retain(
            |_, p| !matches!(&p.preview,Preview::Event(plan) if plan.context.club.entity.id==id),
        );
        let club = self.detail_json(&ctx.club);
        let city = ctx.city.as_ref().map(|d| self.detail_json(d));
        let sponsors=ctx.sponsors.iter().map(|s|json!({"slot":s.slot,"kind":s.kind,"annual_gbp":s.annual_gbp,"total_gbp":s.total_gbp,"start":s.start,"end":s.end})).collect::<Vec<_>>();
        let result = json!({"club":club,"city":city,"sponsors":sponsors,"game_date":ctx.game_date,"warnings":ctx.warnings,"draft_defaults":events::EventDraft::default(),"live":self.live});
        self.contexts.insert(id, ctx);
        Ok(result)
    }
    fn dispatch(&mut self, command: &str, a: &Value) -> Result<Value> {
        if (connection::is_mutation(command) || command == "career_targets") && self.connection.enabled {
            self.connection_preflight()?;
        }
        let result = self.dispatch_inner(command, a);
        // Invalidating a detected session change is distinct from a failed
        // business validation. Preserve previews for ordinary input errors.
        if result.is_err() && self.connection.enabled && self.live {
            self.connection_poll();
        }
        result
    }
    fn dispatch_inner(&mut self, command: &str, a: &Value) -> Result<Value> {
        match command {
            #[cfg(feature = "fm27-health-experiment")]
            "injury_test_preview" | "injury_test_apply" | "ban_test_preview" | "ban_test_apply" => self.health_experiment_command(command, a),
            #[cfg(feature = "fm27-transfer-experiment")]
            "transfer_test_preview" | "transfer_test_apply" => self.health_experiment_command(command, a),
            "capabilities" => Ok(api::capabilities()),
            "connection_poll" => Ok(self.connection_poll()),
            "connection_reconnect" => self.connection_reconnect(),
            "name_localization_context" | "name_localization_preview" | "name_localization_apply" | "name_localization_history" | "name_localization_undo" => self.name_localization_command(command, a),
            "relation_detail" | "relation_preview" | "relation_apply" | "relation_history" | "relation_undo" | "career_targets" => self.relation_command(command, a),
            "staff_review_start" | "staff_review_step" | "staff_review_cancel" => self.staff_review_command(command, a),
            "status" | "summary" => Ok(self.summary()),
            "load_snapshot" => self.load(Path::new(required_str(a, "path")?)),
            "connect" => {
                self.connection_connect()
            }
            "refresh" => {
                if self.live {
                    self.invalidate();
                    backend::refresh_squad(self.index.as_mut().context("연결 없음")?)?;
                    Ok(self.summary())
                } else {
                    let p = self.snapshot.clone().context("조회 자료 없음")?;
                    self.load(&p)
                }
            }
            "search" => self.search(a),
            "club_growth" => {
                self.require_live()?;
                let club=self.find(EntityKind::Club,required_id(a,"club_id")?)?;
                let result=crate::growth::club_growth(self.require_live()?,&club)?;
                Ok(serde_json::to_value(result)?)
            }
            "detail" => {
                let (k, id) = identity(a)?;
                self.read_detail(k, id)
            }
            "overlay_detail" => {
                let id = required_id(a, "id")?;
                let e = self.find(EntityKind::Staff, id)?;
                let mut d = if self.live {
                    backend::detail(&self.index()?.session, &e)?
                } else {
                    load_snapshot_detail(
                        self.snapshot.as_ref().context("조회 자료 경로 없음")?,
                        EntityKind::Staff,
                        id,
                    )?
                };
                self.remember_staff_affiliation(&mut d.entity);
                // A polling overlay must never replace the editor's retained
                // pre-edit baseline or invalidate its reviewed changes.
                Ok(self.detail_json(&d))
            }
            "preview" => {
                let key = identity(a)?;
                let d = self
                    .details
                    .get(&key)
                    .context("상세 정보를 먼저 불러오세요.")?
                    .clone();
                let edits: Vec<Edit> =
                    serde_json::from_value(a.get("edits").cloned().context("변경 항목 없음")?)?;
                let changes = transaction::prepare(&d, &edits)?;
                let mut preview = d.clone();
                for e in &edits {
                    if let Some(f) = preview.fields.iter_mut().find(|f| f.key == e.key) {
                        f.value = e.value;
                    }
                }
                if preview.entity.kind == EntityKind::Staff {
                    preview.fields.retain(|f| !f.key.starts_with("score_"));
                    fields::append_scores(&mut preview);
                }
                let scores = preview
                    .fields
                    .iter()
                    .filter(|f| f.key.starts_with("score_"))
                    .map(field)
                    .collect::<Vec<_>>();
                let ent = entity(&d.entity);
                let token = self.token(Preview::Edit { detail: d, edits });
                Ok(
                    json!({"token":token,"entity":ent,"changes":changes.iter().map(change).collect::<Vec<_>>(),"scores":scores,"live":self.live,"can_apply":self.live}),
                )
            }
            "apply" => {
                let pending = self.consume(required_str(a, "token")?)?;
                self.require_live()?;
                let Preview::Edit { detail, edits } = pending else {
                    bail!("선수·스태프 수정 검토가 아닙니다")
                };
                let r = transaction::apply(&self.index()?.session, &detail, &edits)?;
                self.invalidate();
                Ok(receipt(&r))
            }
            "history" => Ok(json!(
                transaction::history()
                    .iter()
                    .map(receipt)
                    .collect::<Vec<_>>()
            )),
            "undo" => {
                let s = self.require_live()?.clone();
                let id = required_str(a, "receipt_id")?;
                let r = transaction::history()
                    .into_iter()
                    .find(|r| r.id == id)
                    .context("변경 기록 없음")?;
                ensure!(
                    same_session(&s, &r.session),
                    "현재 게임 연결과 기록이 다릅니다"
                );
                let result = transaction::undo(&r)?;
                self.invalidate();
                Ok(receipt(&result))
            }
            "event_context" => self.event_context(required_id(a, "club_id")?),
            "event_preview" => {
                let id = required_id(a, "club_id")?;
                let c = self
                    .contexts
                    .get(&id)
                    .context("이벤트 구단 자료를 먼저 불러오세요")?;
                let mut v = serde_json::to_value(events::EventDraft::default())?;
                let draft = a
                    .get("draft")
                    .and_then(Value::as_object)
                    .context("이벤트 입력 없음")?;
                for (k, val) in draft {
                    ensure!(v.get(k).is_some(), "알 수 없는 이벤트 항목: {k}");
                    v[k] = val.clone();
                }
                let p = events::plan(c, &serde_json::from_value(v)?)?;
                let mut result = plan(&p, self.live);
                result["token"] = json!(self.token(Preview::Event(p)));
                Ok(result)
            }
            "event_apply" => {
                let pending = self.consume(required_str(a, "token")?)?;
                self.require_live()?;
                let Preview::Event(p) = pending else {
                    bail!("이벤트 검토가 아닙니다")
                };
                ensure!(
                    same_session(&self.index()?.session, &p.context.session),
                    "현재 연결과 이벤트가 다릅니다"
                );
                let r = events::apply(&p)?;
                self.invalidate();
                Ok(event_receipt(&r, self.live))
            }
            "event_save" => {
                let p = self
                    .pending
                    .get(required_str(a, "token")?)
                    .context("이벤트 검토 결과 없음")?;
                ensure!(
                    p.created.elapsed() < TOKEN_TTL,
                    "검토 유효 시간이 지났습니다"
                );
                let Preview::Event(plan) = &p.preview else {
                    bail!("이벤트 검토가 아닙니다")
                };
                let path = events::save_draft(plan)?;
                Ok(json!({"narrative_path":path.with_extension("txt"),"path":path}))
            }
            "event_history" => Ok(json!(
                events::history()
                    .iter()
                    .map(|r| event_receipt(r, self.live))
                    .collect::<Vec<_>>()
            )),
            "event_undo" => {
                let s = self.require_live()?.clone();
                let id = required_str(a, "receipt_id")?;
                let r = events::history()
                    .into_iter()
                    .find(|r| r.id == id)
                    .context("이벤트 기록 없음")?;
                ensure!(
                    same_session(&s, &r.plan.context.session),
                    "현재 연결과 이벤트 기록이 다릅니다"
                );
                let result = events::undo(&r)?;
                self.invalidate();
                Ok(event_receipt(&result, self.live))
            }
            "export_snapshot" => {
                let path = PathBuf::from(required_str(a, "path")?);
                let i = self.index()?;
                // Do not silently overwrite the source snapshot or arbitrary existing files.
                use std::fs::OpenOptions;
                let f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .context("새 파일 경로를 선택하세요")?;
                // The full index contains hundreds of thousands of records.
                // Buffer JSON tokens instead of issuing a disk write per token.
                let mut writer = std::io::BufWriter::with_capacity(256 * 1024, f);
                serde_json::to_writer(&mut writer, i)?;
                writer.flush()?;
                writer.get_ref().sync_all()?;
                Ok(json!({"path":path,"records":i.entities.len()}))
            }
            _ => self.workspace_command(command, a),
        }
    }
}

fn load_snapshot_detail(index_path: &Path, k: EntityKind, id: u32) -> Result<Detail> {
    let folder = index_path.parent().context("조회 자료 경로 없음")?;
    let paths = [
        folder.join(format!("detail-{k:?}-{id}.json")),
        folder.join(format!("detail-{k:?}.json")),
    ];
    for p in paths {
        if let Ok(bytes) = std::fs::read(p) {
            let d: Detail = serde_json::from_slice(&bytes)?;
            if d.entity.kind == k && d.entity.id == id {
                return Ok(d);
            }
        }
    }
    bail!("{k:?} {id}의 상세 조회 자료가 없습니다. FM 연결 후 불러올 수 있습니다.")
}

/// Inherited anonymous stdin/stdout pipes work in the Windows subsystem binary.
/// The caller must redirect both streams. No console or window is allocated here.
pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut session = api::ApiSession::default();
    loop {
        let mut bytes = Vec::new();
        let size = (&mut input)
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if size == 0 {
            break;
        }
        if bytes.len() > MAX_LINE {
            bail!("Bridge request exceeds 1 MiB");
        }
        let response = match serde_json::from_slice::<Value>(&bytes) {
            Ok(request) => session.handle(request),
            Err(e) => {
                crate::runtime_log::event("error","command","invalid_json","Malformed bridge JSON rejected");
                json!({"id":Value::Null,"ok":false,"error":format!("Invalid request: {e}")})
            },
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> (Bridge, Detail) {
        let e = Entity {
            nationality: String::new(),
            nationality_id: None,
            football_category: crate::model::FootballCategory::Unknown,
            id: 7,
            kind: EntityKind::Player,
            name: "Example".into(),
            english_name: "Example".into(),
            club: "Club".into(),
            club_id: Some(5),
            age: Some(20),
            ca: Some(100),
            pa: Some(150),
            role: "ST".into(),
            group: "First".into(),
            address: 0x10000,
            base: 0x11000,
            index: None,
        };
        let d = Detail {
            entity: e.clone(),
            fields: vec![
                Field {
                    key: "ca".into(),
                    label: "CA".into(),
                    section: "Ability".into(),
                    value: 100.,
                    min: 1.,
                    max: 200.,
                    step: 1.,
                    hint: String::new(),
                    binding: Some(Binding {
                        address: 0x12000,
                        storage: Storage::U16,
                        scale: 1.,
                        original: 100u16.to_le_bytes().to_vec(),
                    }),
                },
                Field {
                    key: "pa".into(),
                    label: "PA".into(),
                    section: "Ability".into(),
                    value: 150.,
                    min: 1.,
                    max: 200.,
                    step: 1.,
                    hint: String::new(),
                    binding: Some(Binding {
                        address: 0x12002,
                        storage: Storage::U16,
                        scale: 1.,
                        original: 150u16.to_le_bytes().to_vec(),
                    }),
                },
            ],
            guards: vec![],
            notes: vec![],
        };
        let session = Session {
            pid: 0,
            created: 1,
            module_base: 0,
            module_size: 0,
            module_stamp: 0,
            db: 0,
            registry_begin: 0,
            game_date: "2027-01-01".into(),
            profile: "test".into(),
            own_club_id: Some(5),
        };
        let mut b = Bridge {
            index: Some(Index {
                club_leagues: Vec::new(),
                session,
                entities: vec![e],
                warnings: vec![],
            }),
            ..Bridge::default()
        };
        b.details.insert((EntityKind::Player, 7), d.clone());
        (b, d)
    }
    pub(super) fn staff_refresh_fixture()->Bridge {
        let (mut b,mut detail)=fixture();detail.entity.kind=EntityKind::Staff;
        detail.entity.club_id=Some(11);detail.entity.club="A".into();detail.entity.role="코치".into();detail.entity.group="1군".into();
        b.index.as_mut().unwrap().entities[0]=detail.entity.clone();b.details.clear();b.details.insert((EntityKind::Staff,7),detail);
        b.staff_refresh_hook=Some(|index| {
            index.warnings.push("synthetic staff metadata refresh".into());
            let e=&mut index.entities[0];e.club_id=Some(22);e.club="B".into();e.role="스카우트".into();e.group="U-20".into();Ok(())
        });b
    }
    #[test]
    fn staff_search_refreshes_first_page_and_global_scope_without_touching_scalar_baseline() {
        let mut b=staff_refresh_fixture();
        let preview=b.dispatch("preview",&json!({"kind":"Staff","id":7,"edits":[{"key":"ca","value":110}]})).unwrap();
        let token=preview["token"].as_str().unwrap();let baseline=serde_json::to_value(&b.details[&(EntityKind::Staff,7)]).unwrap();
        b.live=true;
        let source=b.search(&json!({"kind":"Staff","club_id":11})).unwrap();assert_eq!(source["total"],0);
        let destination=b.search(&json!({"kind":"Staff","club_id":22})).unwrap();assert_eq!(destination["total"],1);
        let calls=b.index.as_ref().unwrap().warnings.len();b.search(&json!({"kind":"Staff","offset":1})).unwrap();
        b.search(&json!({"kind":"Player"})).unwrap();assert_eq!(b.index.as_ref().unwrap().warnings.len(),calls);
        b.search(&json!({})).unwrap();assert_eq!(b.index.as_ref().unwrap().warnings.len(),calls+1);
        assert_eq!(serde_json::to_value(&b.details[&(EntityKind::Staff,7)]).unwrap(),baseline);assert!(b.pending.contains_key(token));
    }
    #[test]
    fn staff_detail_index_metadata_update_leaves_retained_baseline_and_token_untouched() {
        let mut b=staff_refresh_fixture();let baseline=b.details[&(EntityKind::Staff,7)].clone();
        let preview=b.dispatch("preview",&json!({"kind":"Staff","id":7,"edits":[{"key":"ca","value":110}]})).unwrap();let token=preview["token"].as_str().unwrap();
        let mut fresh=baseline.entity.clone();fresh.club_id=Some(22);fresh.club="B".into();fresh.role="스카우트".into();fresh.group="U-20".into();
        b.remember_staff_affiliation(&mut fresh);assert_eq!(b.index.as_ref().unwrap().entities[0].club_id,Some(11));
        b.live=true;b.remember_staff_affiliation(&mut fresh);
        assert_eq!(b.index.as_ref().unwrap().entities[0].club_id,Some(22));assert_eq!(b.index.as_ref().unwrap().entities[0].role,"스카우트");
        assert_eq!(serde_json::to_value(&b.details[&(EntityKind::Staff,7)]).unwrap(),serde_json::to_value(&baseline).unwrap());assert!(b.pending.contains_key(token));
        fresh.address+=8;fresh.club_id=Some(33);b.remember_staff_affiliation(&mut fresh);assert_eq!(b.index.as_ref().unwrap().entities[0].club_id,Some(22));
    }
    #[test]
    fn offline_preview_cannot_apply_and_is_single_use() {
        let (mut b, _) = fixture();
        let v = b
            .dispatch(
                "preview",
                &json!({"kind":"Player","id":7,"edits":[{"key":"ca","value":110}]}),
            )
            .unwrap();
        assert_eq!(v["can_apply"], false);
        assert_eq!(v["changes"][0]["before"], 100.);
        let token = v["token"].as_str().unwrap();
        assert!(
            b.dispatch("apply", &json!({"token":token}))
                .unwrap_err()
                .to_string()
                .contains("읽기 전용")
        );
        assert!(
            b.dispatch("apply", &json!({"token":token}))
                .unwrap_err()
                .to_string()
                .contains("이미 사용")
        );
    }
    #[test]
    fn identities_are_kind_qualified_and_payloads_hide_addresses() {
        let (mut b, d) = fixture();
        let mut staff = d.entity.clone();
        staff.kind = EntityKind::Staff;
        staff.name = "Coach".into();
        b.index.as_mut().unwrap().entities.push(staff);
        assert_eq!(b.find(EntityKind::Player, 7).unwrap().name, "Example");
        assert_eq!(b.find(EntityKind::Staff, 7).unwrap().name, "Coach");
        let s = b
            .search(&json!({"kind":"Staff","query":"Coach"}))
            .unwrap()
            .to_string();
        assert!(!s.contains("address"));
        assert!(!s.contains("base"));
        assert!(
            b.dispatch(
                "preview",
                &json!({"kind":"Staff","id":7,"edits":[{"key":"ca","value":110}]})
            )
            .is_err()
        );
    }
    #[test]
    fn preview_uses_retained_baseline_and_invalidates_on_connection_change() {
        let (mut b, _) = fixture();
        b.index.as_mut().unwrap().entities[0].ca = Some(120);
        let v = b
            .dispatch(
                "preview",
                &json!({"kind":"Player","id":7,"edits":[{"key":"ca","value":110}]}),
            )
            .unwrap();
        assert_eq!(v["changes"][0]["before"], 100.);
        b.replace_connection();
        assert!(b.consume(v["token"].as_str().unwrap()).is_err());
        assert!(b.details.is_empty());
        assert!(!b.live);
    }
    #[test]
    fn invalid_keys_duplicate_edits_and_ca_above_pa_fail_before_token() {
        let (mut b, _) = fixture();
        for edits in [
            json!([{"key":"address","value":123}]),
            json!([{"key":"ca","value":180}]),
            json!([{"key":"ca","value":110},{"key":"ca","value":111}]),
        ] {
            assert!(
                b.dispatch("preview", &json!({"kind":"Player","id":7,"edits":edits}))
                    .is_err()
            );
            assert!(b.pending.is_empty());
        }
    }
    #[test]
    fn snapshot_fallback_requires_exact_identity() {
        let (_, d) = fixture();
        let folder =
            std::env::temp_dir().join(format!("touchline-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("detail-Player.json"),
            serde_json::to_vec(&d).unwrap(),
        )
        .unwrap();
        assert!(load_snapshot_detail(&folder.join("index.json"), EntityKind::Player, 7).is_ok());
        assert!(load_snapshot_detail(&folder.join("index.json"), EntityKind::Player, 8).is_err());
        assert!(load_snapshot_detail(&folder.join("index.json"), EntityKind::Staff, 7).is_err());
        std::fs::remove_file(folder.join("detail-Player.json")).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn growth_snapshot_rejected_before_any_process_access() {
        let (mut b, d) = fixture();
        let mut club = d.entity;
        club.kind=EntityKind::Club;
        club.id=5;
        b.index.as_mut().unwrap().entities.push(club);
        // The fixture PID is zero. Reaching Process::open would produce an OS
        // error, whereas this command must reject the offline baseline first.
        let error=b.dispatch("club_growth",&json!({"club_id":5})).unwrap_err().to_string();
        assert!(error.contains("읽기 전용"));
        assert!(error.contains("FM에 직접 연결"));
        assert!(b.pending.is_empty());
    }
    #[test]
    fn expired_tokens_fail_and_do_not_survive_rejection() {
        let (mut b, d) = fixture();
        let token = b.token(Preview::Edit {
            detail: d,
            edits: vec![],
        });
        b.pending.get_mut(&token).unwrap().created = Instant::now() - TOKEN_TTL;
        assert!(b.consume(&token).unwrap_err_text().contains("유효 시간"));
        assert!(!b.pending.contains_key(&token));
    }
    trait ErrorText {
        fn unwrap_err_text(self) -> String;
    }
    impl ErrorText for Result<Preview> {
        fn unwrap_err_text(self) -> String {
            match self {
                Err(e) => e.to_string(),
                Ok(_) => panic!("expected error"),
            }
        }
    }
}
