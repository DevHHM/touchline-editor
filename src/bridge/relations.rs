//! Semantic person relation transport. Native pointers and saved bytes stay here.
use super::*;
use crate::relations as engine;
use crate::workspace::Ref;

#[derive(Default)]
pub(super) struct RelationState {
    baselines: HashMap<(EntityKind, u32), engine::PersonRelations>,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    AddCareerTarget { target: engine::targets::CareerTargetRef, condition: u8, scope: u8, job: u8 },
    SetCareerTarget { slot: usize, target: engine::targets::CareerTargetRef, condition: u8, scope: u8, job: u8 },
    SetSocialRelation { slot: usize, relation_kind: u8, target: Ref, reason: u16, level: u8 },
    AddSocialRelation { relation_kind: u8, target: Ref, reason: u16, level: u8 },
    DeleteSocialRelation { slot: usize },
    SetCareerDetails { slot: usize, condition: u8, scope: u8, job: u8 },
    AddCareerClub {
        club_id: u32,
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
        club_id: u32,
        condition: u8,
        job: u8,
    },
    SetFavouriteClub {
        slot: usize,
        club_id: u32,
        reason: u16,
        level: u8,
    },
    AddFavouriteClub {
        #[serde(default)]
        template_slot: Option<usize>,
        club_id: u32,
        reason: u16,
        level: u8,
    },
    SetSecondaryNationality {
        slot: usize,
        donor: Ref,
        expected_nation_uid: u32,
    },
    AddSecondaryNationality {
        donor: Ref,
        expected_nation_uid: u32,
    },
    ReuseName {
        field: engine::NameField,
        donor: Ref,
    },
}
fn linked(target: &engine::LinkedRecord) -> Value {
    json!({"uid":target.uid,"name":target.name,"kind":if target.object_type==1{Some("Club")}else{None}})
}
fn view(snapshot: &engine::PersonRelations, live: bool) -> Value {
    let relation_add_unavailable = engine::relation_add_unavailable_reason(snapshot);
    let relation_add_reason = relation_add_unavailable
        .unwrap_or(if snapshot.spare_slots == 0 {
            "기존 관계를 보존하고 검증된 게임용 배열로 용량을 늘려 추가합니다"
        } else {
            "기존 관계를 유지하며 여유 슬롯에 추가합니다"
        });
    let add_mode = snapshot.career.as_ref().and_then(engine::career_add_mode);
    let add_reason = match add_mode {
        Some("native_allocation") => "게임 할당 구조로 새 항목을 만들고 목록 용량을 확보합니다",
        Some("blank") => "이미 할당된 빈 항목에 커리어 계획을 추가합니다",
        Some("deleted_record") => "이 편집기에서 삭제 후 보존한 레코드를 재사용합니다",
        _ => snapshot.career.as_ref().and_then(|career|career.allocation_reason.as_deref()).unwrap_or("추가할 수 있는 커리어 항목을 확인하지 못했습니다. 다시 읽어 상태를 확인하세요"),
    };
    let mut career_notes = vec![
        "구단·대회·국가·대륙 계획의 추가·수정·삭제를 지원합니다. 모든 변경은 원본을 확인한 뒤 적용합니다"
            .to_string(),
    ];
    if snapshot.career.is_none() {
        career_notes.push("커리어 계획 조회 자료를 검증하지 못했습니다".into());
    } else if snapshot.career.as_ref().unwrap().rows.is_empty() {
        career_notes.push("이 인물에게 이미 할당된 커리어 계획 항목이 없습니다".into());
    }
    let career_rows=snapshot.career.as_ref().map(|career|career.rows.iter().map(|row| {
        let deletion = engine::career_delete_check(snapshot, row.slot);
        let delete_reason = match &deletion {
            Ok(()) => "목록에서 삭제하며 원본 레코드는 되돌리기를 위해 보존합니다".to_string(),
            Err(error) => format!("삭제 불가: {error:#}"),
        };
        json!({
        "slot":row.slot,"condition":row.condition,"condition_label":engine::career_condition_label(row.condition),
        "scope":row.scope,"scope_label":match row.scope{1=>"구단",2=>"국가대표",_=>"확인되지 않은 범위"},
        "job":row.job,"job_label":engine::career_job_label(row.job),"object_type":row.object_type,
        "object_type_label":engine::career_target_type_label(row.object_type),
        "target":row.target.as_ref().map(linked),"editable_club":row.editable_club,"can_delete":deletion.is_ok(),
        "editable_details":engine::career_details_check(snapshot,row.slot).is_ok(),
        "edit_reason":if engine::career_details_check(snapshot,row.slot).is_ok() { "계획·범위·직무와 구단·대회·국가·대륙 대상을 변경할 수 있습니다" } else { "원본 계획의 구조를 확인할 수 없습니다. 다시 읽어 주세요" },
        "delete_reason":delete_reason
    })}).collect::<Vec<_>>()).unwrap_or_default();
    json!({"entity":entity(&snapshot.entity),"names":snapshot.names.iter().map(|name|json!({"field":name.field,"text":name.text})).collect::<Vec<_>>(),
        "primary_nationality":snapshot.primary_nationality.as_ref().map(linked),
        "rows":snapshot.rows.iter().map(|row|json!({"slot":row.slot,"kind":row.kind,"kind_label":row.kind_label,"object_type":row.object_type,
            "reason":row.reason,"reason_label":engine::social_reason_label(row.kind,row.reason),"level":row.level,"target":row.target.as_ref().map(linked),"editable_favourite":row.editable_favourite,"editable_social":engine::social_editable(row),"editable_secondary_nationality":row.editable_secondary_nationality,"can_delete":engine::social_editable(row)||row.editable_secondary_nationality})).collect::<Vec<_>>(),
        "career":{"available":snapshot.career.is_some(),"rows":career_rows,"can_create":snapshot.career.as_ref().is_some_and(|c|c.allocation_available),"can_add":add_mode.is_some(),"add_mode":add_mode,"add_reason":add_reason,"notes":career_notes},
        "can_add_favourite":relation_add_unavailable.is_none(),"can_add_secondary_nationality":relation_add_unavailable.is_none(),
        "relation_add_reason":relation_add_reason,"favourite_add_reason":relation_add_reason,"secondary_nationality_add_reason":relation_add_reason,
        "spare_slots":snapshot.spare_slots,"notes":snapshot.notes,"live":live,
        "options":{"social_kinds":engine::SOCIAL_KINDS.iter().map(|(value,label)|json!({"value":value,"label":label})).collect::<Vec<_>>(),"career_conditions":engine::CAREER_CONDITIONS.iter().map(|(value,label)|json!({"value":value,"label":label})).collect::<Vec<_>>(),
            "career_jobs":engine::CAREER_JOBS.iter().map(|(value,label)|json!({"value":value,"label":label})).collect::<Vec<_>>()}})
}
fn plan_view(plan: &engine::RelationPlan, live: bool) -> Value {
    json!({"entity":entity(&plan.entity),"summary":plan.summary,"before":plan.before,"after":plan.after,"can_apply":live,"live":live})
}
fn receipt_view(receipt: &engine::RelationReceipt, session: Option<&Session>) -> Value {
    let live = session.is_some();
    let time = receipt
        .time
        .parse::<u128>()
        .map(|nanos| (nanos / 1_000_000).to_string())
        .unwrap_or_else(|_| receipt.time.clone());
    json!({"id":receipt.id,"time":time,"status":receipt.status,"entity":entity(&receipt.plan.entity),
        "summary":receipt.plan.summary,"before":receipt.plan.before,"after":receipt.plan.after,"undo_of":receipt.undo_of,"live":live,
        "changes":[{"key":"relation","label":receipt.plan.summary,"before":receipt.plan.before,"after":receipt.plan.after}],
        "can_undo":session.is_some_and(|session|same_session(session,&receipt.plan.session))&&receipt.status.starts_with("applied")&&receipt.undo_of.is_none()})
}
impl Bridge {
    fn find_social_target(&self, target: &Ref) -> Result<Entity> {
        if target.kind!=EntityKind::Person {return self.find(target.kind,target.id);}
        let mut found=self.index()?.entities.iter().filter(|e|e.id==target.id && matches!(e.kind,EntityKind::Player|EntityKind::Staff|EntityKind::Person));
        let entity=found.next().context("대상 인물을 찾을 수 없습니다")?;
        ensure!(found.next().is_none(),"대상 인물 ID가 중복되어 다시 연결해야 합니다");Ok(entity.clone())
    }
    fn finish_relation_mutation(
        &mut self,
        mut receipt: engine::RelationReceipt,
        refresh_names: impl FnOnce(&mut Index, &Entity) -> Result<Entity>,
    ) -> Result<Value> {
        self.invalidate();
        if matches!(receipt.plan.request, engine::RelationRequest::ReuseName { .. }) {
            let refresh = (|| {
                let index = self.index.as_mut().context("이름을 갱신할 연결이 없습니다")?;
                ensure!(same_session(&index.session, &receipt.plan.session), "이름 변경 기록의 세션이 다릅니다");
                refresh_names(index, &receipt.plan.entity)
            })();
            match refresh {
                Ok(entity) => receipt.plan.entity = entity,
                Err(error) => {
                    // The write has already committed. Do not claim it failed
                    // or leave a stale searchable index behind.
                    self.replace_connection();
                    let action = if receipt.undo_of.is_some() { "이름 되돌리기" } else { "이름 변경" };
                    bail!("{action}는 적용되었지만 이름 재조회에 실패했습니다. 기록 ID: {}. 검색 목록을 비우고 연결을 해제했습니다. FM에 다시 연결한 뒤 변경 기록을 확인하세요: {error:#}", receipt.id);
                }
            }
        }
        Ok(receipt_view(&receipt, Some(&self.index()?.session)))
    }
    fn apply_relation_with(
        &mut self,
        args: &Value,
        apply: impl FnOnce(&Session, &engine::RelationPlan) -> Result<engine::RelationReceipt>,
        refresh_names: impl FnOnce(&mut Index, &Entity) -> Result<Entity>,
    ) -> Result<Value> {
        let session = self.require_live()?.clone();
        let Preview::Relation(plan) = self.consume(required_str(args, "token")?)? else {
            bail!("관계 변경 검토가 아닙니다")
        };
        let receipt = apply(&session, &plan).map_err(|error| self.relation_mutation_error(error))?;
        self.finish_relation_mutation(receipt, refresh_names)
    }
    fn undo_relation_with(
        &mut self,
        args: &Value,
        lookup: impl FnOnce(&str) -> Result<engine::RelationReceipt>,
        undo: impl FnOnce(&str) -> Result<engine::RelationReceipt>,
        refresh_names: impl FnOnce(&mut Index, &Entity) -> Result<Entity>,
    ) -> Result<Value> {
        let session = self.require_live()?.clone();
        let id = required_str(args, "receipt_id")?;
        let receipt = lookup(id)?;
        ensure!(same_session(&session, &receipt.plan.session), "다른 게임 세션의 관계 변경 기록입니다");
        let result = undo(id).map_err(|error| self.relation_mutation_error(error))?;
        self.finish_relation_mutation(result, refresh_names)
    }
    fn relation_mutation_error(&mut self, error: anyhow::Error) -> anyhow::Error {
        if engine::requires_reconnect(&error) {
            self.replace_connection();
            error.context("관계 배열 쓰기 중 오류가 발생해 연결을 해제했습니다. FM에 다시 연결한 뒤 변경 기록을 확인하세요")
        } else {
            error
        }
    }
    fn read_relation_snapshot(&self, kind: EntityKind, id: u32) -> Result<engine::PersonRelations> {
        ensure!(
            matches!(
                kind,
                EntityKind::Player | EntityKind::Staff | EntityKind::Person
            ),
            "선수·스태프·인물을 선택하세요"
        );
        let e = self.find(kind, id)?;
        if self.live {
            return engine::inspect(&self.index()?.session, &e);
        }
        let folder = self
            .snapshot
            .as_ref()
            .context("관계 조회 자료 없음")?
            .parent()
            .context("조회 자료 경로 없음")?;
        for path in [
            folder.join(format!("relations-{kind:?}-{id}.json")),
            folder.join(format!("relations-{kind:?}.json")),
        ] {
            if !path.exists() {
                continue;
            }
            ensure!(
                path.metadata()?.len() <= 2 * 1024 * 1024,
                "관계 조회 자료가 너무 큽니다"
            );
            let snapshot: engine::PersonRelations = serde_json::from_slice(&std::fs::read(path)?)?;
            if snapshot.entity.kind == kind && snapshot.entity.id == id {
                return Ok(snapshot);
            }
        }
        bail!("이 인물의 관계 조회 자료가 없습니다. FM 연결 후 관계를 불러오세요")
    }
    fn resolve_operation(&self, operation: Operation) -> Result<engine::RelationRequest> {
        use engine::RelationRequest as R;
        Ok(match operation {
            Operation::AddCareerTarget{target,condition,scope,job}=>R::AddCareerTarget{target,condition,scope,job},
            Operation::SetCareerTarget{slot,target,condition,scope,job}=>R::SetCareerTarget{slot,target,condition,scope,job},
            Operation::SetSocialRelation {slot,relation_kind,target,reason,level} => R::SetSocialRelation {slot,kind:relation_kind,target:self.find_social_target(&target)?,reason,level},
            Operation::AddSocialRelation {relation_kind,target,reason,level} => R::AddSocialRelation {kind:relation_kind,target:self.find_social_target(&target)?,reason,level},
            Operation::DeleteSocialRelation {slot} => R::DeleteSocialRelation {slot},
            Operation::SetCareerDetails { slot, condition, scope, job } => R::SetCareerDetails { slot, condition, scope, job },
            Operation::AddCareerClub {
                club_id,
                condition,
                job,
            } => R::AddCareerClub {
                club: self.find(EntityKind::Club, club_id)?,
                condition,
                job,
            },
            Operation::DeleteCareerPlan { slot } => R::DeleteCareerPlan { slot },
            Operation::DeleteFavouriteClub { slot } => R::DeleteFavouriteClub { slot },
            Operation::DeleteSecondaryNationality { slot } => {
                R::DeleteSecondaryNationality { slot }
            }
            Operation::SetCareerClub {
                slot,
                club_id,
                condition,
                job,
            } => R::SetCareerClub {
                slot,
                club: self.find(EntityKind::Club, club_id)?,
                condition,
                job,
            },
            Operation::SetFavouriteClub {
                slot,
                club_id,
                reason,
                level,
            } => R::SetFavouriteClub {
                slot,
                club: self.find(EntityKind::Club, club_id)?,
                reason,
                level,
            },
            Operation::AddFavouriteClub {
                template_slot,
                club_id,
                reason,
                level,
            } => R::AddFavouriteClub {
                template_slot,
                club: self.find(EntityKind::Club, club_id)?,
                reason,
                level,
            },
            Operation::SetSecondaryNationality {
                slot,
                donor,
                expected_nation_uid,
            } => R::SetSecondaryNationality {
                slot,
                donor: self.find(donor.kind, donor.id)?,
                expected_nation_uid,
            },
            Operation::AddSecondaryNationality {
                donor,
                expected_nation_uid,
            } => R::AddSecondaryNationality {
                donor: self.find(donor.kind, donor.id)?,
                expected_nation_uid,
            },
            Operation::ReuseName { field, donor } => R::ReuseName {
                field,
                donor: self.find(donor.kind, donor.id)?,
            },
        })
    }
    fn offline_relation_plan(
        &self,
        baseline: &engine::PersonRelations,
        request: &engine::RelationRequest,
    ) -> Result<engine::RelationPlan> {
        use engine::RelationRequest as R;
        let (target, name, guards) = match request {
            R::AddCareerTarget{target,..} | R::SetCareerTarget{target,..} => {
                let linked=if target.object_type==3 {let club=self.find(EntityKind::Club,target.uid)?;engine::LinkedRecord{uid:club.id,name:club.name,address:club.address,object_type:3}}
                else {baseline.career.as_ref().and_then(|c|c.rows.iter().find(|r|r.object_type==target.object_type && r.target.as_ref().is_some_and(|t|t.uid==target.uid))).and_then(|r|r.target.clone()).context("스냅샷에 이 대상의 원본 정보가 없습니다")?};
                (Some(linked),None,vec![])
            },
            R::AddSocialRelation { target, .. } | R::SetSocialRelation { target, .. } => (Some(engine::LinkedRecord {uid:target.id,name:target.name.clone(),address:target.address,object_type:if target.kind==EntityKind::Club {1}else{3}}),None,vec![]),
            R::AddCareerClub { club, .. }
            | R::SetCareerClub { club, .. }
            | R::SetFavouriteClub { club, .. }
            | R::AddFavouriteClub { club, .. } => (
                Some(engine::LinkedRecord {
                    uid: club.id,
                    name: club.name.clone(),
                    address: club.address,
                    object_type: 1,
                }),
                None,
                vec![],
            ),
            R::SetSecondaryNationality { donor, .. } | R::AddSecondaryNationality { donor, .. } => {
                let source = self.read_relation_snapshot(donor.kind, donor.id)?;
                (
                    Some(
                        source
                            .primary_nationality
                            .context("원본 인물의 기본 국적 조회 자료가 없습니다")?,
                    ),
                    None,
                    source.guards,
                )
            }
            R::ReuseName { field, donor } => {
                let source = self.read_relation_snapshot(donor.kind, donor.id)?;
                let name = source
                    .names
                    .iter()
                    .find(|name| name.field == *field)
                    .cloned()
                    .context("원본 인물의 해당 이름 조회 자료가 없습니다")?;
                (None, Some(name), source.guards)
            }
            R::SetCareerDetails { .. }
            | R::DeleteSocialRelation { .. }
            | R::DeleteCareerPlan { .. }
            | R::DeleteFavouriteClub { .. }
            | R::DeleteSecondaryNationality { .. } => (None, None, vec![]),
        };
        engine::prepare_snapshot(
            &self.index()?.session,
            baseline,
            request,
            target.as_ref(),
            name.as_ref(),
            guards,
        )
    }
    pub(super) fn relation_command(&mut self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "career_targets" => {
                let typ=required_id(args,"object_type")?;ensure!([3,5,9,25].contains(&typ),"대상 종류를 확인하세요");
                let query=args.get("query").and_then(Value::as_str).unwrap_or("");
                let limit=args.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1,200) as usize;
                let rows=if self.live {engine::targets::search(self.require_live()?,typ as u8,query,limit)?}
                else {self.index()?.entities.iter().filter(|e|typ==3 && e.kind==EntityKind::Club && (e.name.to_lowercase().contains(&query.to_lowercase())||e.id.to_string()==query)).take(limit).map(|e|engine::targets::CareerTargetOption{object_type:3,uid:e.id,name:e.name.clone()}).collect()};
                Ok(json!({"items":rows,"limit":limit,"live":self.live}))
            },
            "relation_detail" => {
                let kind = kind(args)?;
                let id = required_id(args, "id")?;
                let snapshot = self.read_relation_snapshot(kind, id)?;
                let result = view(&snapshot, self.live);
                if self.relation_state.baselines.len() >= 128 {
                    self.relation_state.baselines.clear();
                }
                self.relation_state.baselines.insert((kind, id), snapshot);
                self.pending.retain(|_,pending|!matches!(&pending.preview,Preview::Relation(plan) if plan.entity.kind==kind&&plan.entity.id==id));
                Ok(result)
            }
            "relation_preview" => {
                let kind = kind(args)?;
                let id = required_id(args, "id")?;
                let baseline = self
                    .relation_state
                    .baselines
                    .get(&(kind, id))
                    .context("관계를 먼저 조회한 뒤 변경을 검토하세요")?;
                let operation: Operation = serde_json::from_value(
                    args.get("operation")
                        .cloned()
                        .context("관계 변경 내용이 없습니다")?,
                )?;
                let request = self.resolve_operation(operation)?;
                let plan = if self.live {
                    engine::prepare_retained(&self.index()?.session, baseline, &request)?
                } else {
                    self.offline_relation_plan(baseline, &request)?
                };
                let mut result = plan_view(&plan, self.live);
                result["token"] = json!(self.token(Preview::Relation(plan)));
                Ok(result)
            }
            "relation_apply" => {
                self.apply_relation_with(args, engine::apply, backend::refresh_person_names)
            }
            "relation_history" => {
                let session = if self.live {
                    Some(&self.index()?.session)
                } else {
                    None
                };
                Ok(json!(
                    engine::history()
                        .iter()
                        .map(|receipt| receipt_view(receipt, session))
                        .collect::<Vec<_>>()
                ))
            }
            "relation_undo" => {
                self.undo_relation_with(args, |id| engine::history().into_iter()
                    .find(|receipt| receipt.id == id).context("관계 변경 기록을 찾지 못했습니다"),
                    engine::undo, backend::refresh_person_names)
            }
            _ => bail!("알 수 없는 관계 명령: {command}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        bridge: Bridge,
        folder: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Ok(entries) = std::fs::read_dir(&self.folder) {
                for entry in entries.flatten() {
                    if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
            let _ = std::fs::remove_dir(&self.folder);
        }
    }
    fn fixture() -> Fixture {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "touchline-relations-{}-{stamp}",
            std::process::id()
        ));
        engine::fixtures::write(&folder).unwrap();
        let mut bridge = Bridge::default();
        bridge.load(&folder.join("index.json")).unwrap();
        Fixture { bridge, folder }
    }
    fn load(f: &mut Fixture, id: u32) -> Value {
        f.bridge
            .dispatch("relation_detail", &json!({"kind":"Player","id":id}))
            .unwrap()
    }
    fn preview(f: &mut Fixture, operation: Value) -> Value {
        f.bridge
            .dispatch(
                "relation_preview",
                &json!({"kind":"Player","id":7,"operation":operation}),
            )
            .unwrap()
    }
    fn prepared_name_change(f: &mut Fixture) -> (Value, engine::RelationReceipt) {
        load(f, 7);
        let preview = preview(f, json!({"action":"reuse_name","field":"first","donor":{"kind":"Staff","id":17}}));
        let Preview::Relation(plan) = &f.bridge.pending[preview["token"].as_str().unwrap()].preview else { panic!("wrong token") };
        (preview, engine::RelationReceipt {
            id: "synthetic-name-apply".into(), time: "1700000000000000000".into(),
            status: "applied".into(), plan: plan.clone(), undo_of: None,
        })
    }
    fn indexed_names(index: &mut Index, target: &Entity, name: &str, english: &str) -> Result<Entity> {
        backend::refresh_person_names_with(index, target, |_, _| Ok((name.into(), english.into())))
    }
    #[test]
    fn name_apply_and_undo_paths_refresh_search_link_entities_and_discard_old_queries() {
        let mut f = fixture();
        f.bridge.dispatch("detail", &json!({"kind":"Player","id":7})).unwrap();
        let old_query = f.bridge.dispatch("query_start", &json!({"kind":"Player"})).unwrap();
        let (preview, original_receipt) = prepared_name_change(&mut f);
        let original = original_receipt.plan.entity.clone();
        // Only the mutation and read boundaries are injected. These are the
        // production apply/undo transport paths; no Process is ever opened.
        f.bridge.live = true;
        let applied = f.bridge.apply_relation_with(&json!({"token":preview["token"]}),
            |session, plan| {
                assert!(same_session(session, &plan.session));
                Ok(original_receipt.clone())
            }, |index, target| indexed_names(index, target, "새 통용명", "New Surname")).unwrap();
        assert_eq!(applied["entity"]["name"], "새 통용명");
        assert_eq!(applied["entity"]["english_name"], "New Surname");
        assert!(f.bridge.details.is_empty() && f.bridge.pending.is_empty() && f.bridge.relation_state.baselines.is_empty());
        assert!(f.bridge.dispatch("query_step", &json!({"query_id":old_query["query_id"]})).is_err());
        let search = f.bridge.dispatch("search", &json!({"kind":"Player","query":"new surname"})).unwrap();
        assert_eq!(search["total"], 1);
        assert_eq!(search["items"][0]["name"], "새 통용명");
        assert_eq!(f.bridge.find(EntityKind::Player, 7).unwrap().english_name, "New Surname");
        assert_eq!(f.bridge.dispatch("search", &json!({"kind":"Player","query":original.english_name})).unwrap()["total"], 0);
        let query = f.bridge.dispatch("query_start", &json!({"kind":"Player","query":"New Surname"})).unwrap();
        assert_eq!(query["total"], 1);
        let mut undo_receipt = original_receipt.clone();
        undo_receipt.id = "synthetic-name-undo".into();
        undo_receipt.status = "undone".into();
        undo_receipt.undo_of = Some(original_receipt.id.clone());
        let undone = f.bridge.undo_relation_with(&json!({"receipt_id":original_receipt.id}),
            |_| Ok(original_receipt.clone()), |_| Ok(undo_receipt.clone()),
            |index, target| indexed_names(index, target, &original.name, &original.english_name)).unwrap();
        assert_eq!(undone["entity"]["name"], original.name);
        assert_eq!(undone["entity"]["english_name"], original.english_name);
        assert!(f.bridge.dispatch("query_step", &json!({"query_id":query["query_id"]})).is_err());
        assert_eq!(f.bridge.dispatch("search", &json!({"kind":"Player","query":"New Surname"})).unwrap()["total"], 0);
        assert_eq!(f.bridge.dispatch("search", &json!({"kind":"Player","query":original.english_name})).unwrap()["total"], 1);
    }
    #[test]
    fn applied_name_refresh_failure_disconnects_for_both_apply_and_undo_without_false_success() {
        for undo in [false, true] {
            let mut f = fixture();
            let (preview, receipt) = prepared_name_change(&mut f);
            f.bridge.live = true;
            let error = if undo {
                let mut reverse = receipt.clone(); reverse.id = "synthetic-name-undo".into(); reverse.status = "undone".into(); reverse.undo_of = Some(receipt.id.clone());
                f.bridge.undo_relation_with(&json!({"receipt_id":receipt.id}), |_| Ok(receipt.clone()), |_| Ok(reverse),
                    |_, _| bail!("synthetic name read failure"))
            } else {
                f.bridge.apply_relation_with(&json!({"token":preview["token"]}), |_, _| Ok(receipt.clone()),
                    |_, _| bail!("synthetic name read failure"))
            }.unwrap_err().to_string();
            assert!(error.contains("적용되었지만 이름 재조회에 실패"));
            assert!(error.contains(if undo { "synthetic-name-undo" } else { "synthetic-name-apply" }));
            let summary = f.bridge.dispatch("summary", &json!({})).unwrap();
            assert_eq!(summary["connected"], false);
            assert_eq!(summary["live"], false);
            assert_eq!(summary["counts"]["Player"], 0);
            assert!(f.bridge.dispatch("search", &json!({"kind":"Player"})).is_err());
            assert!(f.bridge.dispatch("query_start", &json!({"kind":"Player"})).is_err());
            assert!(f.bridge.find(EntityKind::Player, 7).is_err());
            assert!(f.bridge.pending.is_empty() && f.bridge.relation_state.baselines.is_empty());
        }
    }
    #[test]
    fn non_name_relations_do_not_refresh_names_and_failed_mutations_do_not_publish_names() {
        let mut f = fixture();
        let (preview, mut receipt) = prepared_name_change(&mut f);
        f.bridge.live = true;
        let before = serde_json::to_vec(f.bridge.index().unwrap()).unwrap();
        assert!(f.bridge.apply_relation_with(&json!({"token":preview["token"]}), |_, _| bail!("mutation rejected"),
            |_, _| panic!("failed writes must not refresh")).is_err());
        assert_eq!(serde_json::to_vec(f.bridge.index().unwrap()).unwrap(), before);
        receipt.plan.request = engine::RelationRequest::DeleteCareerPlan { slot: 0 };
        let token = f.bridge.token(Preview::Relation(receipt.plan.clone()));
        f.bridge.apply_relation_with(&json!({"token":token}), |_, _| Ok(receipt.clone()),
            |_, _| panic!("unrelated operation must not read names")).unwrap();
        assert_eq!(serde_json::to_vec(f.bridge.index().unwrap()).unwrap(), before);
        let mut reverse = receipt.clone(); reverse.undo_of = Some(receipt.id.clone());
        f.bridge.undo_relation_with(&json!({"receipt_id":receipt.id}), |_| Ok(receipt.clone()), |_| Ok(reverse),
            |_, _| panic!("unrelated undo must not read names")).unwrap();
        assert_eq!(serde_json::to_vec(f.bridge.index().unwrap()).unwrap(), before);
    }
    #[test]
    fn typed_array_write_failure_disconnects_apply_and_undo_but_stale_rejection_keeps_connection() {
        for undo in [false, true] {
            for started in [false, true] {
                let mut f = fixture();
                let (preview, receipt) = prepared_name_change(&mut f);
                f.bridge.live = true;
                let before = serde_json::to_vec(f.bridge.index().unwrap()).unwrap();
                let failure = || -> Result<engine::RelationReceipt> {
                    if started {
                        Err(anyhow::Error::new(engine::RelationWriteFailure {
                            message: "synthetic array write failed".into(),
                            receipt_id: "123-77".into(),
                        }).context("synthetic wrapped mutation"))
                    } else {
                        bail!("synthetic stale preview before writing")
                    }
                };
                let result = if undo {
                    f.bridge.undo_relation_with(&json!({"receipt_id":receipt.id}), |_| Ok(receipt.clone()), |_| failure(),
                        |_, _| panic!("failed mutation must not refresh names"))
                } else {
                    f.bridge.apply_relation_with(&json!({"token":preview["token"]}), |_, _| failure(),
                        |_, _| panic!("failed mutation must not refresh names"))
                };
                let error = result.unwrap_err();
                assert_eq!(engine::requires_reconnect(&error), started);
                let summary = f.bridge.dispatch("summary", &json!({})).unwrap();
                assert_eq!(summary["connected"], !started);
                assert_eq!(summary["live"], !started);
                if started {
                    let message = format!("{error:#}");
                    assert!(message.contains("연결을 해제했습니다") && message.contains("123-77"));
                    assert!(f.bridge.index.is_none());
                    assert!(f.bridge.pending.is_empty() && f.bridge.relation_state.baselines.is_empty());
                    assert!(f.bridge.dispatch("search", &json!({"kind":"Player"})).is_err());
                } else {
                    assert_eq!(serde_json::to_vec(f.bridge.index().unwrap()).unwrap(), before);
                    assert!(!format!("{error:#}").contains("연결을 해제했습니다"));
                }
            }
        }
    }
    fn assert_sanitized(value: &Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    assert!(
                        ![
                            "address",
                            "base",
                            "record",
                            "text_object",
                            "raw",
                            "guards",
                            "holder",
                            "begin",
                            "end",
                            "capacity",
                            "old_bytes",
                            "new_bytes",
                            "binding",
                            "session",
                            "request"
                        ]
                        .contains(&key.as_str()),
                        "private key: {key}"
                    );
                    assert_sanitized(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    assert_sanitized(value);
                }
            }
            _ => {}
        }
    }
    #[test]
    fn offline_relation_views_and_all_typed_previews_are_sanitized_and_readonly() {
        let mut f = fixture();
        let detail = load(&mut f, 7);
        assert_eq!(detail["career"]["available"], true);
        assert_eq!(detail["career"]["can_create"], false);
        assert_eq!(detail["career"]["rows"][0]["job_label"], "선수");
        assert_eq!(detail["rows"][0]["target"]["kind"], "Club");
        assert_eq!(
            detail["options"]["career_jobs"].as_array().unwrap().len(),
            39
        );
        assert_sanitized(&detail);
        for operation in [
            json!({"action":"set_social_relation","slot":0,"relation_kind":3,"target":{"kind":"Club","id":44},"reason":1,"level":91}),
            json!({"action":"add_social_relation","relation_kind":1,"target":{"kind":"Staff","id":17},"reason":9,"level":85}),
            json!({"action":"add_social_relation","relation_kind":4,"target":{"kind":"Club","id":45},"reason":0,"level":60}),
            json!({"action":"delete_social_relation","slot":0}),
            json!({"action":"set_career_details","slot":0,"condition":2,"scope":1,"job":16}),
            json!({"action":"set_career_target","slot":0,"target":{"object_type":3,"uid":45},"condition":2,"scope":1,"job":16}),
            json!({"action":"delete_career_plan","slot":0}),
            json!({"action":"delete_favourite_club","slot":0}),
            json!({"action":"delete_secondary_nationality","slot":1}),
            json!({"action":"set_career_club","slot":0,"club_id":45,"condition":2,"job":16}),
            json!({"action":"set_favourite_club","slot":0,"club_id":45,"reason":1,"level":90}),
            json!({"action":"add_favourite_club","template_slot":0,"club_id":45,"reason":2,"level":95}),
            json!({"action":"set_secondary_nationality","slot":1,"donor":{"kind":"Staff","id":17},"expected_nation_uid":111}),
            json!({"action":"add_secondary_nationality","donor":{"kind":"Staff","id":17},"expected_nation_uid":111}),
            json!({"action":"reuse_name","field":"first","donor":{"kind":"Staff","id":17}}),
        ] {
            let p = preview(&mut f, operation);
            assert_eq!(p["can_apply"], false);
            assert_eq!(p["live"], false);
            assert_ne!(p["before"], p["after"]);
            assert_sanitized(&p);
            assert!(
                f.bridge
                    .dispatch("relation_apply", &json!({"token":p["token"]}))
                    .is_err()
            );
            assert!(f.bridge.pending.contains_key(p["token"].as_str().unwrap()));
        }
        assert!(f.bridge.details.is_empty());
    }
    #[test]
    fn first_favourite_and_both_career_add_modes_have_real_readonly_plans() {
        let mut f = fixture();
        for (kind, id, operation, mode) in [
            (
                "Staff",
                17,
                json!({"action":"add_favourite_club","club_id":45,"reason":1,"level":90}),
                None,
            ),
            (
                "Staff",
                17,
                json!({"action":"add_secondary_nationality","donor":{"kind":"Player","id":7},"expected_nation_uid":113}),
                None,
            ),
            (
                "Person",
                9,
                json!({"action":"add_career_club","club_id":45,"condition":1,"job":16}),
                Some("blank"),
            ),
            (
                "Person",
                10,
                json!({"action":"add_career_club","club_id":45,"condition":2,"job":1}),
                Some("deleted_record"),
            ),
        ] {
            let detail = f
                .bridge
                .dispatch("relation_detail", &json!({"kind":kind,"id":id}))
                .unwrap();
            if let Some(mode) = mode {
                assert_eq!(detail["career"]["can_add"], true);
                assert_eq!(detail["career"]["add_mode"], mode);
            } else {
                assert_eq!(detail["rows"], json!([]));
                assert_eq!(detail["can_add_favourite"], true);
            }
            assert_sanitized(&detail);
            let preview = f
                .bridge
                .dispatch(
                    "relation_preview",
                    &json!({"kind":kind,"id":id,"operation":operation}),
                )
                .unwrap();
            assert_eq!(preview["can_apply"], false);
            assert_sanitized(&preview);
            let Preview::Relation(plan) = f
                .bridge
                .consume(preview["token"].as_str().unwrap())
                .unwrap()
            else {
                panic!("wrong token")
            };
            assert!(!plan.changes.is_empty());
            assert!(
                f.bridge
                    .consume(preview["token"].as_str().unwrap())
                    .is_err()
            );
        }
        let detail = load(&mut f, 7);
        assert_eq!(detail["career"]["can_add"], false);
        assert_eq!(detail["career"]["rows"][0]["can_delete"], true);
        assert_eq!(detail["rows"][0]["can_delete"], true);
        assert_eq!(detail["rows"][1]["can_delete"], true);
    }
    #[test]
    fn competition_plan_can_be_retargeted_or_deleted_without_losing_record_guards() {
        let mut f = fixture();
        let path = f.folder.join("relations-Player-7.json");
        let mut snapshot: engine::PersonRelations = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let row = &mut snapshot.career.as_mut().unwrap().rows[0];
        row.object_type = 25;
        row.raw[11] = 25;
        row.target = None;
        row.editable_club = false;
        let record = row.record;
        let raw = row.raw.clone();
        snapshot.guards.iter_mut().find(|g| g.address == record).unwrap().expected = raw;
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let detail = load(&mut f, 7);
        let row = &detail["career"]["rows"][0];
        assert_eq!(row["editable_club"], false);
        assert_eq!(row["can_delete"], true);
        assert_eq!(row["object_type_label"], "대회");
        assert!(row["delete_reason"].as_str().unwrap().contains("원본 레코드"));
        assert_sanitized(&detail);
        assert_eq!(row["editable_details"],true);
        let retargeted=preview(&mut f,json!({"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":1}));
        assert_eq!(retargeted["can_apply"],false); assert_sanitized(&retargeted);
        let deleted = preview(&mut f, json!({"action":"delete_career_plan","slot":0}));
        assert!(deleted["before"].as_str().unwrap().contains("대회"));
        assert!(!deleted["before"].as_str().unwrap().contains("빈 항목"));
        assert_sanitized(&deleted);
        snapshot.guards.retain(|g| g.address != record);
        let unavailable = view(&snapshot, false);
        assert_eq!(unavailable["career"]["rows"][0]["can_delete"], false);
        assert!(unavailable["career"]["rows"][0]["delete_reason"].as_str().unwrap().starts_with("삭제 불가:"));
    }
    #[test]
    fn relation_add_capability_matches_planner_and_explains_missing_preimage() {
        let (_, _, snapshots) = engine::fixtures::data();
        let empty = &snapshots[2];
        assert_eq!(view(empty, false)["can_add_favourite"], true);
        for mutation in 0..5 {
            let mut unavailable = empty.clone();
            match mutation {
                0 => unavailable.append_bytes = None,
                1 => unavailable.append_bytes = Some(vec![0; 15]),
                2 => unavailable.holder = 0,
                3 => unavailable.capacity = unavailable.end,
                _ => unavailable.spare_slots += 1,
            }
            let result = view(&unavailable, false);
            let reason = engine::relation_add_unavailable_reason(&unavailable).unwrap();
            assert_eq!(result["can_add_favourite"], false);
            assert_eq!(result["can_add_secondary_nationality"], false);
            assert_eq!(result["relation_add_reason"], reason);
            assert_eq!(result["favourite_add_reason"], reason);
            assert_eq!(result["secondary_nationality_add_reason"], reason);
        }
    }
    #[test]
    fn verified_native_growth_has_symbolic_readonly_preview_without_memory_addresses() {
        let mut f = fixture();
        let path = f.folder.join("relations-Player-7.json");
        let mut snapshot: engine::PersonRelations = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        snapshot.capacity = snapshot.end;
        snapshot.spare_slots = 0;
        snapshot.append_bytes = None;
        snapshot.vector_growth_available = true;
        snapshot.guards.retain(|g| g.address != snapshot.end);
        snapshot.guards.iter_mut().find(|g| g.address == snapshot.holder).unwrap().expected[16..24].copy_from_slice(&snapshot.end.to_le_bytes());
        std::fs::write(path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let detail = load(&mut f, 7);
        assert_eq!(detail["can_add_favourite"], true);
        assert_eq!(detail["can_add_secondary_nationality"], true);
        assert!(detail["relation_add_reason"].as_str().unwrap().contains("용량을 늘려"));
        let preview = preview(&mut f, json!({"action":"add_favourite_club","club_id":45,"reason":1,"level":90}));
        assert_eq!(preview["can_apply"], false);
        assert_sanitized(&detail);
        assert_sanitized(&preview);
        let Preview::Relation(plan) = f.bridge.consume(preview["token"].as_str().unwrap()).unwrap() else { panic!("wrong plan") };
        assert!(plan.growth.is_some());
        assert!(plan.allocation.is_none());
        assert!(plan.retained_original.is_none());
        assert!(plan.changes.is_empty());
    }
    #[test]
    fn relation_preview_requires_baseline_and_never_replaces_it_from_changed_snapshot_file() {
        let mut f = fixture();
        let operation =
            json!({"action":"set_career_club","slot":0,"club_id":45,"condition":2,"job":16});
        assert!(
            f.bridge
                .dispatch(
                    "relation_preview",
                    &json!({"kind":"Player","id":7,"operation":operation})
                )
                .is_err()
        );
        load(&mut f, 7);
        let path = f.folder.join("relations-Player-7.json");
        let mut stored: engine::PersonRelations =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored.career.as_mut().unwrap().rows[0].job = 2;
        std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        let p = preview(&mut f, operation);
        assert!(p["before"].as_str().unwrap().contains("선수"));
        let (_, details, _) = engine::fixtures::data();
        let scalar = f.bridge.token(Preview::Edit {
            detail: details[0].clone(),
            edits: vec![],
        });
        load(&mut f, 7);
        assert!(!f.bridge.pending.contains_key(p["token"].as_str().unwrap()));
        assert!(f.bridge.pending.contains_key(&scalar));
    }
    #[test]
    fn relation_tokens_are_single_use_and_connection_changes_clear_baselines() {
        let mut f = fixture();
        load(&mut f, 7);
        let p = preview(
            &mut f,
            json!({"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":16}),
        );
        let token = p["token"].as_str().unwrap();
        let Preview::Relation(plan) = f.bridge.consume(token).unwrap() else {
            panic!("wrong token type")
        };
        assert_eq!(plan.changes.len(), 1);
        assert!(f.bridge.consume(token).is_err());
        f.bridge.replace_connection();
        assert!(f.bridge.relation_state.baselines.is_empty());
        assert!(f.bridge.pending.is_empty());
    }
    #[test]
    fn invalid_typed_operations_and_missing_career_records_fail_before_token_creation() {
        let mut f = fixture();
        load(&mut f, 7);
        for operation in [
            json!({"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":255}),
            json!({"action":"set_career_club","slot":9,"club_id":45,"condition":1,"job":1}),
            json!({"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":1,"address":65536}),
            json!({"action":"add_secondary_nationality","donor":{"kind":"Staff","id":17},"expected_nation_uid":999}),
        ] {
            assert!(
                f.bridge
                    .dispatch(
                        "relation_preview",
                        &json!({"kind":"Player","id":7,"operation":operation})
                    )
                    .is_err()
            );
            assert!(f.bridge.pending.is_empty());
        }
        let unavailable = load(&mut f, 8);
        assert_eq!(unavailable["career"]["available"], false);
        assert!(f.bridge.dispatch("relation_preview",&json!({"kind":"Player","id":8,"operation":{"action":"reuse_name","field":"last","donor":{"kind":"Staff","id":17}}})).is_ok());
        let donor = f
            .bridge
            .dispatch("relation_detail", &json!({"kind":"Staff","id":17}))
            .unwrap();
        assert_eq!(donor["career"]["rows"], json!([]));
        assert!(f.bridge.dispatch("relation_preview",&json!({"kind":"Staff","id":17,"operation":{"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":1}})).is_err());
        f.bridge
            .dispatch("relation_detail", &json!({"kind":"Person","id":9}))
            .unwrap();
        let blank=f.bridge.dispatch("relation_preview",&json!({"kind":"Person","id":9,"operation":{"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":16}})).unwrap();
        assert!(blank["before"].as_str().unwrap().contains("빈"));
    }
    #[test]
    fn relation_receipt_uses_public_milliseconds_and_semantic_change_strings() {
        let mut f = fixture();
        load(&mut f, 7);
        let p = preview(
            &mut f,
            json!({"action":"set_career_club","slot":0,"club_id":45,"condition":1,"job":16}),
        );
        let Preview::Relation(plan) = f.bridge.consume(p["token"].as_str().unwrap()).unwrap()
        else {
            panic!("wrong token")
        };
        let receipt = engine::RelationReceipt {
            id: "123-0".into(),
            time: "1700000000123456789".into(),
            status: "applied".into(),
            plan,
            undo_of: None,
        };
        let public = receipt_view(&receipt, None);
        assert_eq!(public["time"], "1700000000123");
        assert_eq!(public["can_undo"], false);
        assert_eq!(public["changes"][0]["before"], public["before"]);
        assert_sanitized(&public);
    }
}
