//! File-based Korean player names. Clients choose semantic player IDs and text,
//! never paths or process pointers. Only apply/undo can write files.
use super::*;
use crate::name_localization as engine;
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextArgs { player_ids: Vec<u32> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewArgs { entries: Vec<engine::Input> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArgs { token: String, confirmed: bool }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UndoArgs { id: String, confirmed: bool }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

fn file_view(file: &engine::FileChange) -> Result<Value> {
    Ok(json!({"path":file.path,"operation":if file.before.is_none(){"create"}else if file.path.file_name().is_some_and(|p|p.eq_ignore_ascii_case(engine::MANAGED_FILE)){"update"}else{"remove_conflict"},
        "before_sha256":engine::optional_hash(&file.before)?,"after_sha256":engine::optional_hash(&file.after)?,"conflicts":file.conflicts}))
}
fn receipt_view(receipt: &engine::Receipt, current: Option<&engine::Installation>, undone: &BTreeSet<String>) -> Value {
    json!({"id":receipt.id,"time":receipt.time,"status":receipt.status,"entries":receipt.entries,"files":receipt.files.iter().map(|f|json!({"path":f.path,"operation":f.operation,"conflicts":f.conflicts,"before_sha256":f.before_sha256,"after_sha256":f.after_sha256})).collect::<Vec<_>>(),
        "warnings":receipt.warnings,"message":receipt.message,"undo_of":receipt.undo_of,"restart_required":true,
        "can_undo":receipt.status=="applied"&&receipt.undo_of.is_none()&&!undone.contains(&receipt.id)&&current.is_some_and(|i|*i==receipt.installation),
        "memory_modified":false,"game_saved":false})
}
impl Bridge {
    fn name_live_session(&mut self) -> Result<Session> {
        let session = self.require_live()?.clone();
        if let Err(error) = backend::validate_session(&session) {
            self.replace_connection();
            return Err(error.context("FM 연결이 변경돼 이름 검토를 중단했습니다. 다시 연결하세요"));
        }
        Ok(session)
    }
    fn name_context_with(
        &self, args: &Value, installation: Option<&engine::Installation>,
        read: impl Fn(&Entity) -> Result<engine::Parts>,
    ) -> Result<Value> {
        let args: ContextArgs = serde_json::from_value(args.clone())?;
        engine::validate_ids(&args.player_ids)?;
        let files = installation.map(engine::inventory).transpose()?;
        let mut rows = Vec::new();
        for id in args.player_ids {
            let entity = self.find(EntityKind::Player, id)?;
            let row = match read(&entity) {
                Ok(parts) => {
                    let suggestion = files.as_ref().map(|files| engine::suggestion(&parts, files)).transpose();
                    match suggestion {
                        Ok(suggestion) => {
                            let (suggested_name, suggestion_source, installed_name) = suggestion.unwrap_or_else(|| (String::new(), "none".into(), None));
                            json!({"id":id,"name":parts.name,"english_name":parts.english_name,"first_name":parts.first_name,"last_name":parts.last_name,"common_name":parts.common_name,
                                "suggested_name":suggested_name,"suggestion_source":suggestion_source,"installed_name":installed_name,"error":null})
                        }
                        Err(error) => json!({"id":id,"name":parts.name,"english_name":parts.english_name,"first_name":parts.first_name,"last_name":parts.last_name,"common_name":parts.common_name,
                            "suggested_name":"","suggestion_source":"none","installed_name":null,"error":format!("{error:#}")}),
                    }
                }
                Err(error) => json!({"id":id,"name":entity.name,"english_name":entity.english_name,"first_name":"","last_name":"","common_name":"",
                    "suggested_name":"","suggestion_source":"none","installed_name":null,"error":format!("{error:#}")}),
            };
            rows.push(row);
        }
        Ok(json!({"live":self.live,"can_install":self.live&&installation.is_some(),"reason":if !self.live {Some("저장된 조회 자료는 한글화 설치를 지원하지 않습니다. FM에 연결하세요.")}else if installation.is_none(){Some("현재 FM 설치 경로를 확인하지 못했습니다.")}else{None},
            "rows":rows,"targets":installation.map(|i|i.targets().into_iter().map(|path|json!({"path":path})).collect::<Vec<_>>()).unwrap_or_default(),
            "restart_required":true,"max_characters":40,"max_utf8_bytes":120}))
    }
    fn name_preview_with(&mut self, args: &Value, installation: &engine::Installation, read: impl Fn(&Session, &Entity) -> Result<engine::Parts>) -> Result<Value> {
        let args: PreviewArgs = serde_json::from_value(args.clone())?;
        let ids: Vec<_> = args.entries.iter().map(|x| x.id).collect();
        engine::validate_ids(&ids)?;
        let session = self.require_live()?.clone();
        let parts: Vec<_> = ids.iter().map(|id| read(&session, &self.find(EntityKind::Player, *id)?)).collect::<Result<_>>()?;
        let plan = engine::prepare(&session, installation, &parts, &args.entries)?;
        let files = plan.files.iter().map(file_view).collect::<Result<Vec<_>>>()?;
        let result = json!({"can_apply":true,"entries":plan.entries,"files":files,"warnings":plan.warnings,"restart_required":true,"live":true});
        // A newly reviewed name draft invalidates older name drafts only.
        self.pending.retain(|_, p| !matches!(p.preview, Preview::NameLocalization(_)));
        let token = self.token(Preview::NameLocalization(plan));
        let mut result = result; result["token"] = json!(token); Ok(result)
    }
    fn name_apply_with(&mut self, args: &Value, install: impl Fn(&Session) -> Result<engine::Installation>, read: impl Fn(&Session, &Entity) -> Result<engine::Parts>, apply: impl FnOnce(&engine::Plan) -> Result<engine::Receipt>) -> Result<Value> {
        let args: ApplyArgs = serde_json::from_value(args.clone())?;
        ensure!(args.confirmed, "검토한 이름 파일 변경을 확인하세요");
        let session = self.require_live()?.clone();
        let Preview::NameLocalization(plan) = self.consume(&args.token)? else { bail!("선수명 한글화 검토가 아닙니다"); };
        ensure!(same_session(&session, &plan.session), "FM 연결이 바뀌었습니다. 이름을 다시 검토하세요");
        ensure!(install(&session)? == plan.installation, "FM 설치 경로가 바뀌었습니다. 다시 검토하세요");
        for old in &plan.parts {
            let entity = self.find(EntityKind::Player, old.id)?;
            ensure!(read(&session, &entity)? == *old, "검토 후 선수 이름이 변경됐습니다. 다시 검토하세요");
        }
        let receipt = apply(&plan)?;
        // Do not rewrite the in-memory search/display name. The LNC loader has
        // not run yet; a later FM reconnect must supply the actual game name.
        self.pending.retain(|_, p| !matches!(p.preview, Preview::NameLocalization(_)));
        Ok(receipt_view(&receipt, Some(&plan.installation), &BTreeSet::new()))
    }
    pub(super) fn name_localization_command(&mut self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "name_localization_context" => {
                if !self.live { return self.name_context_with(args, None, |_| bail!("현재 FM에서 원본 이름을 다시 읽어야 합니다")); }
                let session = self.name_live_session()?;
                match engine::Installation::discover(&session) {
                    Ok(installation) => self.name_context_with(args, Some(&installation), |entity| engine::Parts::read(&session, entity)),
                    Err(error) => {
                        let mut context = self.name_context_with(args, None, |entity| engine::Parts::read(&session, entity))?;
                        context["reason"] = json!(format!("{error:#}")); Ok(context)
                    }
                }
            }
            "name_localization_preview" => {
                let session = self.name_live_session()?;
                let installation = engine::Installation::discover(&session)?;
                self.name_preview_with(args, &installation, engine::Parts::read)
            }
            "name_localization_apply" => {
                self.name_live_session()?;
                self.name_apply_with(args, engine::Installation::discover, engine::Parts::read, engine::apply)
            }
            "name_localization_history" => {
                let _: NoArgs = serde_json::from_value(args.clone())?;
                let installation = if self.live { engine::Installation::discover(&self.name_live_session()?).ok() } else { None };
                let history = engine::history(&engine::store_dir());
                let undone = history.iter().filter(|r|r.status=="applied").filter_map(|r|r.undo_of.clone()).collect();
                Ok(json!({"items":history.iter().map(|r|receipt_view(r,installation.as_ref(),&undone)).collect::<Vec<_>>(),"restart_required":true}))
            }
            "name_localization_undo" => {
                let args: UndoArgs = serde_json::from_value(args.clone())?;
                ensure!(args.confirmed, "이름 파일 되돌리기를 확인하세요");
                let session = self.name_live_session()?;
                let installation = engine::Installation::discover(&session)?;
                let receipt = engine::undo(&installation, &session, &args.id)?;
                self.pending.retain(|_, p| !matches!(p.preview, Preview::NameLocalization(_)));
                Ok(receipt_view(&receipt, Some(&installation), &BTreeSet::new()))
            }
            _ => bail!("알 수 없는 선수명 한글화 명령입니다"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name_localization::tests::Fixture;
    fn bridge(f: &Fixture) -> Bridge {
        let entity = Entity { nationality:String::new(),nationality_id:None,id:f.parts.id,kind:EntityKind::Player,name:f.parts.name.clone(),english_name:f.parts.english_name.clone(),club:"Test club".into(),club_id:Some(1141),age:Some(20),ca:Some(100),pa:Some(150),role:"ST".into(),group:"1군".into(),football_category:FootballCategory::Men,address:0x10000,base:0xf000,index:Some(0) };
        Bridge { index:Some(Index{session:f.session.clone(),entities:vec![entity],warnings:Vec::new()}),live:true,..Default::default() }
    }
    fn preview(b: &mut Bridge,f: &Fixture,name: &str) -> Value {
        b.name_preview_with(&json!({"entries":[{"id":7,"korean_name":name}]}),&f.installation,|_,_|Ok(f.parts.clone())).unwrap()
    }
    #[test]
    fn context_has_semantic_rows_exact_native_parts_and_no_process_coordinates() {
        let f=Fixture::new();let b=bridge(&f);
        let value=b.name_context_with(&json!({"player_ids":[7]}),Some(&f.installation),|_|Ok(f.parts.clone())).unwrap();
        assert_eq!(value["can_install"],true);assert_eq!(value["rows"][0]["first_name"],"Viktor Bjarki");
        assert_eq!(value["targets"].as_array().unwrap().len(),2);
        let encoded=value.to_string();assert!(!encoded.contains("address"));assert!(!encoded.contains("module_base"));
        assert!(b.name_context_with(&json!({"player_ids":[7],"path":"C:/bad"}),Some(&f.installation),|_|panic!()).is_err());
    }
    #[test]
    fn snapshot_context_is_unavailable_and_preview_apply_rejected_before_memory_or_file_access() {
        let f=Fixture::new();let mut b=bridge(&f);b.live=false;
        let context=b.dispatch("name_localization_context",&json!({"player_ids":[7]})).unwrap();
        assert_eq!(context["can_install"],false);assert_eq!(context["live"],false);assert!(context["rows"][0]["error"].is_string());
        for (command,args) in [
            ("name_localization_preview",json!({"entries":[{"id":7,"korean_name":"한글"}]})),
            ("name_localization_apply",json!({"token":"x","confirmed":true})),
            ("name_localization_undo",json!({"id":"123","confirmed":true})),
        ] {
            assert!(b.dispatch(command,&args).unwrap_err().to_string().contains("읽기 전용"));
        }
        assert!(engine::inventory(&f.installation).unwrap().is_empty());
    }
    #[test]
    fn duplicates_unknown_ids_and_unknown_fields_cannot_make_a_preview() {
        let f=Fixture::new();let mut b=bridge(&f);
        for entries in [json!([{ "id":7,"korean_name":"한글"},{"id":7,"korean_name":"다른 이름"}]),json!([{ "id":8,"korean_name":"한글"}]),json!([{ "id":7,"korean_name":"한글","destination":"bad"}])] {
            assert!(b.name_preview_with(&json!({"entries":entries}),&f.installation,|_,_|panic!("must reject before reader")).is_err());
        }
        assert!(b.pending.is_empty());
    }
    #[test]
    fn confirmation_stale_names_changed_session_and_replay_block_install() {
        let f=Fixture::new();let mut b=bridge(&f);
        let p=preview(&mut b,&f,"빅토르 다다손");let token=p["token"].as_str().unwrap();
        assert!(b.name_apply_with(&json!({"token":token,"confirmed":false}),|_|panic!(),|_,_|panic!(),|_|panic!()).is_err());
        assert!(b.pending.contains_key(token));
        let mut changed=f.parts.clone();changed.first_name="Changed".into();
        assert!(b.name_apply_with(&json!({"token":token,"confirmed":true}),|_|Ok(f.installation.clone()),|_,_|Ok(changed.clone()),|_|panic!()).is_err());
        assert!(!b.pending.contains_key(token));
        assert!(b.name_apply_with(&json!({"token":token,"confirmed":true}),|_|panic!(),|_,_|panic!(),|_|panic!()).is_err());
        let p=preview(&mut b,&f,"한글 이름");b.index.as_mut().unwrap().session.created+=1;
        assert!(b.name_apply_with(&json!({"token":p["token"],"confirmed":true}),|_|panic!(),|_,_|panic!(),|_|panic!()).is_err());
        assert!(engine::inventory(&f.installation).unwrap().is_empty());
    }
    #[test]
    fn expired_and_replaced_preview_tokens_never_apply() {
        let f=Fixture::new();let mut b=bridge(&f);
        let first=preview(&mut b,&f,"첫 이름");let second=preview(&mut b,&f,"둘째 이름");
        assert!(!b.pending.contains_key(first["token"].as_str().unwrap()));
        b.pending.get_mut(second["token"].as_str().unwrap()).unwrap().created=Instant::now()-TOKEN_TTL;
        assert!(b.name_apply_with(&json!({"token":second["token"],"confirmed":true}),|_|panic!(),|_,_|panic!(),|_|panic!()).is_err());
        assert!(b.pending.is_empty());
    }
    #[test]
    fn apply_creates_file_receipt_but_keeps_index_names_until_game_reload() {
        let f=Fixture::new();let mut b=bridge(&f);let index_before=serde_json::to_vec(b.index.as_ref().unwrap()).unwrap();
        let p=preview(&mut b,&f,"빅토르 다다손");
        let receipt=b.name_apply_with(&json!({"token":p["token"],"confirmed":true}),|_|Ok(f.installation.clone()),|_,_|Ok(f.parts.clone()),|plan|f.apply(plan)).unwrap();
        assert_eq!(receipt["status"],"applied");assert_eq!(receipt["restart_required"],true);assert_eq!(receipt["memory_modified"],false);
        assert_eq!(serde_json::to_vec(b.index.as_ref().unwrap()).unwrap(),index_before);
        assert!(b.pending.is_empty());assert!(f.installation.targets().iter().all(|p|p.exists()));
    }
}
