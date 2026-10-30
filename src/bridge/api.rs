//! Stateful, address-free command API shared by stdio and local transports.
//! The transport owns authentication and serialization. This layer owns the
//! same retained baselines, preview tokens and guarded mutations as the UI.
use super::*;

#[derive(Default)]
pub(crate) struct ApiSession {
    bridge: Bridge,
}

impl ApiSession {
    pub(crate) fn handle(&mut self, value: Value) -> Value {
        let id = value.get("id").and_then(Value::as_u64).map_or(Value::Null, |id| json!(id));
        let request = serde_json::from_value::<Request>(value);
        let response = request.map_err(anyhow::Error::from).and_then(|mut request| {
            if request.args.is_null() { request.args = json!({}); }
            ensure!(request.args.is_object(), "args must be a JSON object");
            ensure!(COMMANDS.iter().any(|command| command.name == request.command),
                "Unknown command: {}. Call capabilities for supported commands.", request.command);
            // Reads must not keep using an old save's cached identities either.
            // The mutation path also performs its own existing preflight check.
            if self.bridge.connection.enabled && self.bridge.live
                && !matches!(request.command.as_str(), "connect" | "load_snapshot" | "connection_poll" | "connection_reconnect" | "capabilities")
            {
                let observed = self.bridge.connection_poll();
                ensure!(matches!(request.command.as_str(), "status" | "summary") || self.bridge.live,
                    "FM session changed: {}. Call connection_poll and connection_reconnect, then read and preview again.", observed["reason"]);
            }
            self.bridge.dispatch(&request.command, &request.args)
        });
        match response {
            Ok(data) => json!({"id":id,"ok":true,"data":data}),
            Err(error) => json!({"id":id,"ok":false,"error":format!("{error:#}")}),
        }
    }
}

struct Command {
    name: &'static str,
    effect: &'static str,
    description: &'static str,
    required: &'static str,
    optional: &'static str,
}

macro_rules! command {
    ($name:literal, $effect:literal, $description:literal, $required:literal, $optional:literal) => {
        Command { name:$name, effect:$effect, description:$description, required:$required, optional:$optional }
    };
}

const COMMANDS: &[Command] = &[
    command!("capabilities", "read", "Discover supported commands, limits and safe edit workflows.", "", ""),
    command!("status", "read", "Connection and indexed record counts; also observes save changes.", "", ""),
    command!("summary", "read", "Alias of status.", "", ""),
    command!("connect", "session", "Connect to supported running FM and build the reusable index. No game writes.", "", ""),
    command!("connection_poll", "session", "Probe process/save identity; invalidate stale baselines and previews.", "", ""),
    command!("connection_reconnect", "session", "Rebuild the index after the monitor reports can_reconnect=true. No edit is replayed.", "", ""),
    command!("load_snapshot", "session", "Open a local snapshot strictly read-only, even if its original FM process still exists.", "path", ""),
    command!("refresh", "session", "Invalidate previews and refresh the live squad, or reload the snapshot.", "", ""),
    command!("search", "read", "Paged indexed search. City group is its actual nation name (discover with search_facets); player/staff group is the squad. Numeric field predicates use query_start/query_step instead.", "", "kind,query,club_id,group,offset,limit,sort,descending,filter"),
    command!("detail", "read", "Read a kind-qualified entity and editable fields; retain the baseline for preview.", "kind,id", ""),
    command!("overlay_detail", "read", "Fresh staff detail without replacing the editor's retained baseline.", "id", ""),
    command!("club_growth", "read", "Read club squad growth reference values and their limitations.", "club_id", ""),
    command!("field_catalog", "read", "Discover one matching entity's fields plus bounded verified column_fields metadata for the column chooser; column metadata has no player values or editability.", "kind", "id"),
    command!("search_facets", "read", "Discover actual roles, squad groups and football categories.", "kind", ""),
    command!("query_start", "session", "Start incremental advanced search; request fields and field ranges using catalogue keys.", "kind", "filter,club_id,query,fields,sort,sort_field,descending"),
    command!("query_step", "read", "Read the next bounded page. Continue until complete=true; inspect failed/errors.", "query_id", ""),
    command!("query_cancel", "session", "Discard an advanced-search cursor (latest cursor if omitted).", "", "query_id"),
    command!("compare", "read", "Compare up to four kind-qualified entities with fresh details.", "targets", ""),
    command!("resolve_targets", "read", "Resolve up to 10,000 references into found entities and missing references.", "targets", ""),
    command!("preview", "preview", "Prepare field edits against the retained detail baseline; never writes FM.", "kind,id,edits", ""),
    command!("apply", "game_write", "Consume a scalar preview token and execute existing backup/guard/readback transaction.", "token", ""),
    command!("history", "read", "Read sanitized scalar-edit receipts.", "", ""),
    command!("undo", "game_write", "Guarded undo of a receipt from the same unchanged session.", "receipt_id", ""),
    command!("batch_preview", "preview", "Prepare common rules for 1–200 explicit targets; returns actual changes and unchanged rows.", "targets,rules", ""),
    command!("batch_apply", "game_write", "Consume a batch preview token; guarded changes and receipts reuse the editor engine.", "token", ""),
    command!("batch_history", "read", "Read sanitized batch receipts.", "", ""),
    command!("batch_undo", "game_write", "Guarded undo of a batch receipt from the same session.", "receipt_id", ""),
    command!("freeze_preview", "preview", "Prepare fixed desired values for 1–50 targets, including already matching values.", "targets,rules", ""),
    command!("freeze_enable", "session", "Consume only a freeze preview token and retain desired values; does not itself write FM.", "token", ""),
    command!("freeze_status", "read", "Read retained freeze targets and completed write cycles.", "", ""),
    command!("freeze_tick", "game_write", "Explicitly run one guarded freeze cycle; no background ticks run automatically.", "", ""),
    command!("freeze_stop", "session", "Discard active freeze targets.", "", ""),
    command!("relation_detail", "read", "Read relations, career plans and per-item editability, retaining a baseline.", "kind,id", ""),
    command!("career_targets", "read", "Search engine-backed career targets: 3 club, 5 continent, 9 nation, 25 competition.", "object_type", "query,limit"),
    command!("relation_preview", "preview", "Prepare a supported relation/career operation against the retained baseline.", "kind,id,operation", ""),
    command!("relation_apply", "game_write", "Consume a relation preview and apply with native ownership checks and backup.", "token", ""),
    command!("relation_history", "read", "Read sanitized relation/career receipts and available undo status.", "", ""),
    command!("relation_undo", "game_write", "Guarded undo of a relation receipt from the same session.", "receipt_id", ""),
    command!("event_context", "read", "Read retained club/city/sponsor context and event draft defaults.", "club_id", ""),
    command!("event_preview", "preview", "Prepare a sponsorship/club/city scenario from retained event context. Money is GBP.", "club_id,draft", ""),
    command!("event_apply", "game_write", "Consume an event preview and apply existing guarded financial/city changes.", "token", ""),
    command!("event_save", "file_write", "Save a reviewed scenario locally without changing FM.", "token", ""),
    command!("event_history", "read", "Read sanitized event receipts.", "", ""),
    command!("event_undo", "game_write", "Guarded undo of an event receipt from the same session.", "receipt_id", ""),
    command!("staff_review_start", "session", "Start incremental staff recommendation review. Actual hiring interest is not confirmed.", "club_id", "max_candidates"),
    command!("staff_review_step", "read", "Continue staff review until complete, retaining partial results and failures.", "review_id", ""),
    command!("staff_review_cancel", "session", "Cancel staff review and return its partial results.", "review_id", ""),
    command!("name_localization_context", "read", "Read original names and installed localization suggestions for player IDs.", "player_ids", ""),
    command!("name_localization_preview", "preview", "Prepare Korean display-name file changes; input entries are {id,korean_name}.", "entries", ""),
    command!("name_localization_apply", "file_write", "Install a reviewed localization plan; confirmed=true required. Does not rewrite FM memory names.", "token,confirmed", ""),
    command!("name_localization_history", "read", "Read localization file receipts.", "", ""),
    command!("name_localization_undo", "file_write", "Undo reviewed localization files; confirmed=true required.", "id,confirmed", ""),
    command!("library_get", "read", "Read the local scouting/search-preset library.", "", ""),
    command!("library_put", "file_write", "Persist the complete validated local scouting/search-preset library.", "library", ""),
    command!("library_import", "read", "Read a library file for review; does not save it.", "path", ""),
    command!("library_export", "file_write", "Export library to a new path, refusing to overwrite an existing file.", "path", ""),
    command!("export_snapshot", "file_write", "Write the native index to a new local file. Snapshot contains internal addresses; API entity responses do not.", "path", ""),
    command!("export_rows", "file_write", "Export 1–200 explicit entity references to a new CSV path.", "targets,path", ""),
];

pub(crate) fn capabilities() -> Value {
    let list = |s: &str| s.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>();
    json!({
        "api_version":"1.0",
        "engine":"Touchline",
        "engine_version":env!("CARGO_PKG_VERSION"),
        "profile":backend::PROFILE,
        "build_profiles":backend::profiles::ALL,
        "build_capabilities_source":"status.build_capabilities; recognized builds do not imply all editing features are supported",
        "request":{"id":"unsigned integer","command":"string","args":"object; defaults to {}"},
        "response":{"id":"request id or null","ok":"boolean","data":"on success","error":"on failure"},
        "kinds":["Player","Staff","Person","Club","City"],
        "indexed_search_sorts":["name","nationality","club","age","ca","pa","role"],
        "commands":COMMANDS.iter().map(|c|json!({"name":c.name,"effect":c.effect,"description":c.description,"required_args":list(c.required),"optional_args":list(c.optional)})).collect::<Vec<_>>(),
        "limits":{"request_bytes":MAX_LINE,"preview_ttl_seconds":TOKEN_TTL.as_secs(),"search_page":500,"batch_targets":200,"freeze_targets":50,"compare_targets":4,"query_cursors":4,"query_page_rows":200,"staff_review_cursors":2,"staff_review_candidates":20000},
        "types":{
            "EntityRef":{"kind":"Player|Staff|Person|Club|City","id":"u32"},
            "EntityRow":{"nationality":"primary nationality name; empty means unknown","nationality_id":"primary nation u32 ID|null; never inferred from club or secondary nationality"},
            "Edit":{"key":"editable key returned by detail","value":"finite number within field min/max"},
            "Rule":{"key":"editable key returned by detail","operation":"set|add|percent|at_least|at_most","value":"finite number"},
            "FieldRange":{"key":"field_catalog key","min":"number|null","max":"number|null"},
            "Filter":{"age_min":"u32|null","age_max":"u32|null","ca_min":"u16|null","ca_max":"u16|null","pa_min":"u16|null","pa_max":"u16|null","role":"string","club_query":"string","group":"string|null","football_category":"men|women|unknown|null","employed":"bool","unemployed":"bool","fields":"FieldRange[]; query_start only"},
            "RelationOperation":{"discriminator":"action","variants":["add_career_target","set_career_target","set_social_relation","add_social_relation","delete_social_relation","set_career_details","add_career_club","delete_career_plan","delete_favourite_club","delete_secondary_nationality","set_career_club","set_favourite_club","add_favourite_club","set_secondary_nationality","add_secondary_nationality","reuse_name"],"availability":"Read relation_detail first. Unknown/read-only types are not made editable by this API."}
        },
        "workflows":{
            "scalar_edit":["connect once","search","detail","preview with field keys","inspect changes and can_apply","apply token","history"],
            "batch_edit":["search or query_start/query_step","explicit targets, maximum 200 per batch","batch_preview","inspect all changes","batch_apply token"],
            "advanced_search":["field_catalog","query_start with filter.fields and requested fields","query_step repeatedly until complete=true","sort collected numeric field rows on client"],
            "relations":["relation_detail","relation_preview operation","inspect changes","relation_apply token"],
            "event":["event_context","event_preview draft","inspect deltas","event_apply token"],
            "save_reload":["connection_poll invalidates old state","wait until can_reconnect=true","connection_reconnect","search/detail again; never retry an old apply token"]
        },
        "semantics":{"arbitrary_memory_access":false,"game_saved_automatically":false,"offline_writes":false,"mutations_retried_automatically":false,"parallel_dispatch":false,"money_currency":"GBP","ca_rebalanced_automatically":false,"single_use_preview_tokens":true}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_rejects_malformed_requests_and_unknown_commands() {
        let mut s=ApiSession::default();
        for request in [json!([]),json!({"command":"status"}),json!({"id":-1,"command":"status"}),json!({"id":1,"command":"status","address":123}),json!({"id":1,"command":"status","args":[]}),json!({"id":1,"command":"write_memory","args":{}})] {
            assert_eq!(s.handle(request)["ok"],false);
        }
        let response=s.handle(json!({"id":42,"command":"status"}));
        assert_eq!(response["id"],42);assert_eq!(response["ok"],true);assert_eq!(response["data"]["connected"],false);
    }
    #[test]
    fn offline_preview_and_apply_use_existing_baseline_and_single_use_guard() {
        let (bridge,_) = super::super::tests::fixture();
        let mut s=ApiSession{bridge};
        let preview=s.handle(json!({"id":1,"command":"preview","args":{"kind":"Player","id":7,"edits":[{"key":"ca","value":110}]}}));
        assert_eq!(preview["ok"],true);assert_eq!(preview["data"]["can_apply"],false);
        assert_eq!(preview["data"]["changes"][0]["before"],100.0);
        let response=s.handle(json!({"id":2,"command":"apply","args":{"token":preview["data"]["token"]}}));
        assert_eq!(response["ok"],false);assert!(response["error"].as_str().unwrap().contains("읽기 전용"));
        let response=s.handle(json!({"id":3,"command":"apply","args":{"token":preview["data"]["token"]}}));
        assert_eq!(response["ok"],false);assert!(response["error"].as_str().unwrap().contains("이미 사용"));
    }
    #[test]
    fn catalogue_matches_every_top_level_dispatch_arm() {
        use std::collections::BTreeSet;
        let sources=[include_str!("../bridge.rs"),include_str!("workspace.rs"),include_str!("relations.rs"),include_str!("staff_review.rs"),include_str!("name_localization.rs")];
        let mut discovered=BTreeSet::new();
        for source in sources {
            let source=source.split("#[cfg(test)]").next().unwrap();
            for line in source.lines().filter(|line|line.starts_with("            \"")&&line.contains("=>")) {
                let arm=line.split("=>").next().unwrap();
                for (i,quoted) in arm.split('"').enumerate() { if i%2==1 { discovered.insert(quoted); } }
            }
        }
        let declared=COMMANDS.iter().map(|c|c.name).collect::<BTreeSet<_>>();
        assert_eq!(declared.len(),COMMANDS.len(),"Duplicate catalogue command");
        assert_eq!(declared,discovered);
        let mut s=ApiSession::default();
        assert_eq!(s.handle(json!({"id":5,"command":"capabilities"}))["data"],capabilities());
    }
    #[test]
    fn api_responses_do_not_expose_binding_addresses() {
        let (bridge,detail)=super::super::tests::fixture();
        let mut s=ApiSession{bridge};
        let view=s.bridge.detail_json(&detail);
        fn check(value:&Value) {
            match value {
                Value::Object(map)=>for (key,value) in map { assert!(!["address","base","binding","guards","original"].contains(&key.as_str()));check(value); },
                Value::Array(values)=>for value in values {check(value);},
                _=>{}
            }
        }
        check(&view);
        check(&s.handle(json!({"id":1,"command":"search","args":{"kind":"Player"}})));
    }
}
