use super::*;
use crate::batch;
use crate::workspace::{Filter, Library, Ref, Rule};
#[derive(Default)]
pub(super) struct WorkState {
    queries: HashMap<u64, Query>,
    latest_query: Option<u64>,
    freeze: Option<Freeze>,
}
impl WorkState {
    fn take_validated_query(
        &mut self,
        id: u64,
        validate_session: impl FnOnce() -> Result<()>,
    ) -> Result<Query> {
        let query = self
            .queries
            .remove(&id)
            .context("검색이 없거나 완료·중지되었습니다")?;
        // A failed session ends this cursor before any candidate reads. Other
        // independent cursors remain owned by their respective callers.
        validate_session()?;
        Ok(query)
    }
}
struct Query {
    id: u64,
    candidates: Vec<Entity>,
    filter: Filter,
    cursor: usize,
    matched: usize,
    failed: usize,
    value_keys: Vec<String>,
    sort_field: Option<String>,
}
struct Freeze {
    targets: Vec<(Entity, Vec<Edit>, Vec<Guard>)>,
    last: Option<Instant>,
    writes: usize,
}
fn references(a: &Value) -> Result<Vec<Ref>> {
    let r: Vec<Ref> =
        serde_json::from_value(a.get("targets").cloned().context("대상을 선택하세요")?)?;
    ensure!(
        !r.is_empty() && r.len() <= 200,
        "대상은 1–200개를 선택하세요"
    );
    let mut seen = std::collections::HashSet::new();
    ensure!(
        r.iter().all(|x| seen.insert((x.kind, x.id))),
        "대상이 중복되었습니다"
    );
    Ok(r)
}
fn batch_receipt(r: &batch::BatchReceipt) -> Value {
    json!({"id":r.id,"time":r.time,"status":r.status,"items":r.items.iter().map(receipt).collect::<Vec<_>>()})
}
impl Bridge {
    fn inspect_uncached(&self, k: EntityKind, id: u32) -> Result<Detail> {
        let e = self.find(k, id)?;
        self.inspect_search_candidate(&e)
    }
    pub(super) fn inspect_search_candidate(&self, e: &Entity) -> Result<Detail> {
        // Search candidates already have unique identities checked at start.
        // Looking them up by UID again would scan the whole index per row.
        let mut detail = if self.live {
            backend::detail(&self.index()?.session, e)
        } else {
            load_snapshot_detail(
                self.snapshot.as_ref().context("조회 자료 없음")?,
                e.kind,
                e.id,
            )
        }?;
        crate::workspace::ensure_scores(&mut detail);
        Ok(detail)
    }
    fn catalog_detail(&self, k: EntityKind, id: Option<u32>) -> Result<Detail> {
        if let Some(id) = id {
            return self.inspect_uncached(k, id);
        }
        if !self.live {
            let folder = self
                .snapshot
                .as_ref()
                .context("조회 자료 없음")?
                .parent()
                .context("조회 자료 경로 없음")?;
            let fallback = folder.join(format!("detail-{k:?}.json"));
            let prefix = format!("detail-{k:?}-");
            let mut paths = std::fs::read_dir(folder)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".json"))
                })
                .collect::<Vec<_>>();
            paths.sort();
            paths.insert(0, fallback);
            for path in paths {
                if !path
                    .metadata()
                    .is_ok_and(|meta| meta.len() <= 2 * 1024 * 1024)
                {
                    continue;
                }
                let Some(mut detail) = std::fs::read(path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Detail>(&bytes).ok())
                else {
                    continue;
                };
                if detail.entity.kind != k || self.find(k, detail.entity.id).is_err() {
                    continue;
                }
                crate::workspace::ensure_scores(&mut detail);
                return Ok(detail);
            }
            bail!("이 종류의 상세 조회 자료가 없습니다. 해당 프로필을 저장하거나 FM에 연결하세요");
        }
        let index = self.index()?;
        let candidates = index
            .entities
            .iter()
            .filter(|e| e.kind == k && e.club_id == index.session.own_club_id)
            .take(8)
            .chain(index.entities.iter().filter(|e| e.kind == k).take(8));
        let mut seen = std::collections::HashSet::new();
        let mut errors = vec![];
        for candidate in candidates {
            if !seen.insert(candidate.id) {
                continue;
            }
            match self.inspect_uncached(k, candidate.id) {
                Ok(detail) => return Ok(detail),
                Err(error) => errors.push(error.to_string()),
            }
        }
        bail!("검색 필드 정보를 읽을 수 없습니다: {}", errors.join("; "))
    }
    fn query_catalog_fields(&self, k: EntityKind, keys: &[String]) -> Result<Vec<Field>> {
        let sample = self.catalog_detail(k, None)?;
        let sample_id = sample.entity.id;
        let mut catalog = sample.fields;
        let mut missing = keys.iter().filter(|key| !catalog.iter().any(|f| f.key == **key))
            .cloned().collect::<std::collections::HashSet<_>>();
        if missing.is_empty() { return Ok(catalog); }
        // Contract fields legitimately disappear for unemployed people. A
        // different sample must not invalidate a field selected from a verified
        // profile. Supplement metadata only from bounded real detail reads;
        // never accept arbitrary keys or fabricate a zero for an absent value.
        let entities = &self.index()?.entities;
        let candidates = entities.iter().filter(|e| e.kind == k && e.club_id.is_some())
            .chain(entities.iter().filter(|e| e.kind == k && e.club_id.is_none()))
            .filter(|e| e.id != sample_id).take(16);
        for candidate in candidates {
            if let Ok(detail) = self.inspect_search_candidate(candidate) {
                for mut field in detail.fields {
                    if missing.remove(&field.key) {
                        field.binding = None;
                        catalog.push(field);
                    }
                }
                if missing.is_empty() { break; }
            }
        }
        Ok(catalog)
    }
    fn column_catalog_fields(&self, sample: &Detail) -> Result<Vec<Field>> {
        let mut catalog = sample.fields.clone();
        let mut known = catalog.iter().map(|f| f.key.clone()).collect::<std::collections::HashSet<_>>();
        let has_contract = |keys: &std::collections::HashSet<String>| {
            keys.contains("contract.weekly_wage") && keys.contains("contract.expires")
        };
        if !matches!(sample.entity.kind, EntityKind::Player | EntityKind::Staff | EntityKind::Person)
            || has_contract(&known) { return Ok(catalog); }
        // An unemployed sample must not hide contract columns in the chooser.
        // This is metadata discovery, separate from the unchanged sample values.
        for candidate in self.index()?.entities.iter()
            .filter(|e| e.kind == sample.entity.kind && e.id != sample.entity.id && e.club_id.is_some())
            .take(16)
        {
            if let Ok(detail) = self.inspect_search_candidate(candidate) {
                for mut field in detail.fields {
                    if known.insert(field.key.clone()) {
                        field.binding = None;
                        catalog.push(field);
                    }
                }
                if has_contract(&known) { break; }
            }
        }
        Ok(catalog)
    }
    pub(super) fn workspace_command(&mut self, command: &str, a: &Value) -> Result<Value> {
        match command {
            "query_start" => {
                ensure!(
                    self.work.queries.len() < 4,
                    "검색은 동시에 최대 4개입니다. 이전 검색을 중지하세요"
                );
                let filter: Filter =
                    serde_json::from_value(a.get("filter").cloned().unwrap_or(json!({})))?;
                filter.validate()?;
                let k = kind(a)?;
                self.refresh_staff_metadata(k)?;
                let club = a
                    .get("club_id")
                    .filter(|value| !value.is_null())
                    .map(|_| required_id(a, "club_id"))
                    .transpose()?;
                let needle = a.get("query").and_then(Value::as_str).unwrap_or("");
                ensure!(
                    needle.chars().count() <= 200,
                    "이름 검색어는 최대 200자입니다"
                );
                let mut value_keys: Vec<String> = serde_json::from_value(
                    a.get("fields")
                        .filter(|v| !v.is_null())
                        .cloned()
                        .unwrap_or(json!([])),
                )?;
                ensure!(value_keys.len() <= 64, "표시할 숫자 필드는 최대 64개입니다");
                let sort = a.get("sort").and_then(Value::as_str).unwrap_or("pa");
                let indexed_sort = ["name", "ca", "pa", "age", "club", "role", "nationality"].contains(&sort);
                let sort_field = a
                    .get("sort_field")
                    .and_then(Value::as_str)
                    .filter(|key| !key.is_empty())
                    .map(str::to_string)
                    .or_else(|| (!indexed_sort).then(|| sort.to_string()));
                value_keys.extend(filter.fields.iter().map(|range| range.key.clone()));
                value_keys.extend(filter.condition_field_keys().map(str::to_string));
                value_keys.extend(sort_field.iter().cloned());
                let mut unique = std::collections::HashSet::new();
                value_keys.retain(|key| unique.insert(key.clone()));
                if !value_keys.is_empty() {
                    let catalog = self.query_catalog_fields(k, &value_keys)?;
                    filter.validate_catalog(&catalog)?;
                    for key in &value_keys {
                        ensure!(
                            catalog.iter().any(|field| &field.key == key),
                            "지원하지 않는 검색/정렬 필드입니다: {key}"
                        );
                    }
                }
                let mut rows = Vec::new();
                let mut identities = std::collections::HashSet::new();
                for e in self.index()?.entities.iter().filter(|e| e.kind == k) {
                    ensure!(
                        identities.insert(e.id),
                        "같은 종류와 ID의 대상이 중복되었습니다. 다시 연결하세요."
                    );
                    if filter.matches(e)
                        && club.is_none_or(|id| e.club_id == Some(id))
                        && crate::workspace::name_matches(e, needle)
                    {
                        rows.push(e.clone());
                    }
                }
                // Field values arrive incrementally. The client sorts accumulated
                // rows by the requested field; this is only the scan order.
                let descending = a.get("descending").and_then(Value::as_bool).unwrap_or(false);
                rows.sort_by(|a, b| match sort {
                    "ca" => b.ca.cmp(&a.ca).then(a.id.cmp(&b.id)),
                    "age" => a.age.cmp(&b.age).then(a.id.cmp(&b.id)),
                    "pa" => b.pa.cmp(&a.pa).then(a.id.cmp(&b.id)),
                    "club" => a
                        .club
                        .cmp(&b.club)
                        .then(a.name.cmp(&b.name))
                        .then(a.id.cmp(&b.id)),
                    "role" => crate::workspace::compare_roles(a, b, descending),
                    "nationality" => crate::workspace::compare_nationalities(a, b, descending),
                    _ => a.name.cmp(&b.name).then(a.id.cmp(&b.id)),
                });
                self.sequence += 1;
                let id = self.sequence;
                let total = rows.len();
                self.work.queries.insert(
                    id,
                    Query {
                        id,
                        candidates: rows,
                        filter,
                        cursor: 0,
                        matched: 0,
                        failed: 0,
                        value_keys: value_keys.clone(),
                        sort_field: sort_field.clone(),
                    },
                );
                self.work.latest_query = Some(id);
                Ok(
                    json!({"query_id":id,"total":total,"field_keys":value_keys,"sort_field":sort_field,"sorting":if sort_field.is_some(){"client"}else{"index"}}),
                )
            }
            "query_step" => {
                let id = a
                    .get("query_id")
                    .and_then(Value::as_u64)
                    .context("검색 ID가 필요합니다")?;
                let session = self.index()?.session.clone();
                let live = self.live;
                let mut q = self.work.take_validated_query(id, || {
                    if live {
                        backend::validate_session(&session)
                    } else {
                        Ok(())
                    }
                })?;
                let result = (|| -> Result<Value> {
                    let start = Instant::now();
                    let mut rows = vec![];
                    let mut errors = vec![];
                    while q.cursor < q.candidates.len()
                        && rows.len() < 200
                        && start.elapsed() < Duration::from_millis(180)
                    {
                        let e = &q.candidates[q.cursor];
                        q.cursor += 1;
                        if q.value_keys.is_empty() {
                            rows.push(entity(e));
                            q.matched += 1;
                            continue;
                        }
                        match self.inspect_search_candidate(e) {
                            Ok(d) if q.filter.matches_detail(&d) => {
                                let mut row = entity(e);
                                row["values"] = json!(
                                    q.value_keys
                                        .iter()
                                        .filter_map(|key| d
                                            .fields
                                            .iter()
                                            .find(|f| &f.key == key && f.value.is_finite())
                                            .map(|f| (f.key.clone(), f.value)))
                                        .collect::<HashMap<_, _>>()
                                );
                                row["missing_values"] = json!(
                                    q.value_keys
                                        .iter()
                                        .filter(|key| !d
                                            .fields
                                            .iter()
                                            .any(|field| &field.key == *key
                                                && field.value.is_finite()))
                                        .collect::<Vec<_>>()
                                );
                                rows.push(row);
                                q.matched += 1;
                            }
                            Ok(_) => {}
                            Err(err) => {
                                q.failed += 1;
                                if errors.len() < 8 {
                                    errors.push(format!("{}: {err:#}", e.name));
                                }
                                // An already true indexed OR branch does not
                                // depend on a failed profile read. Keep the row
                                // with explicitly missing detail columns.
                                if q.filter.proven_without_detail(e) {
                                    let mut row = entity(e);
                                    row["values"] = json!({});
                                    row["missing_values"] = json!(q.value_keys);
                                    rows.push(row);
                                    q.matched += 1;
                                }
                            }
                        }
                    }
                    Ok(
                        json!({"query_id":q.id,"items":rows,"scanned":q.cursor,"total":q.candidates.len(),"matched":q.matched,"failed":q.failed,"errors":errors,"complete":q.cursor==q.candidates.len(),"field_keys":q.value_keys,"sort_field":q.sort_field,"sorting":if q.sort_field.is_some(){"client"}else{"index"}}),
                    )
                })();
                if q.cursor < q.candidates.len() {
                    self.work.queries.insert(id, q);
                }
                result
            }
            "query_cancel" => {
                let id = if a.get("query_id").is_some_and(|v| !v.is_null()) {
                    Some(
                        a.get("query_id")
                            .and_then(Value::as_u64)
                            .context("검색 ID 형식 오류")?,
                    )
                } else {
                    self.work.latest_query
                };
                let stopped = id.is_some_and(|id| self.work.queries.remove(&id).is_some());
                Ok(json!({"stopped":stopped,"query_id":id}))
            }
            "field_catalog" => {
                let k = kind(a)?;
                let id = a
                    .get("id")
                    .filter(|v| !v.is_null())
                    .map(|_| required_id(a, "id"))
                    .transpose()?;
                let d = self.catalog_detail(k, id)?;
                let columns = self.column_catalog_fields(&d)?;
                Ok(
                    json!({"sample":entity(&d.entity),"fields":d.fields.iter().map(field).collect::<Vec<_>>(),
                        "column_fields":columns.iter().map(|f| json!({"key":f.key,"label":f.label,"section":f.section,
                            "min":f.min,"max":f.max,"step":f.step,"hint":f.hint})).collect::<Vec<_>>(),
                        "column_fields_source":"bounded_verified_metadata","live":self.live,"numeric_sort":"client"}),
                )
            }
            "search_facets" => {
                let k = kind(a)?;
                self.refresh_staff_metadata(k)?;
                let mut roles = std::collections::BTreeMap::<String, usize>::new();
                let mut groups = std::collections::BTreeMap::<String, usize>::new();
                let mut categories = std::collections::BTreeMap::<String, usize>::new();
                let mut nations = std::collections::BTreeMap::<u32, (String, usize)>::new();
                let mut total = 0;
                let mut employed = 0;
                let mut employment_unknown = 0;
                for e in self.index()?.entities.iter().filter(|e| e.kind == k) {
                    total += 1;
                    *categories.entry(e.football_category.key().into()).or_default() += 1;
                    match e.employment() {Some(true)=>employed+=1,None=>employment_unknown+=1,_=>{}}
                    let clean = |text: &str| {
                        text.split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .collect::<String>()
                    };
                    let mut seen = std::collections::HashSet::new();
                    for role in e.role.split('·').map(clean).filter(|role| !role.is_empty()&&e.employment().is_some()) {
                        if seen.insert(role.clone()) {
                            *roles.entry(role).or_default() += 1;
                        }
                    }
                    let group = clean(&e.group);
                    if !group.is_empty()&&e.employment().is_some() {
                        *groups.entry(group).or_default() += 1;
                    }
                    if matches!(k, EntityKind::Player | EntityKind::Staff | EntityKind::Person) {
                        if let Some(id) = e.nationality_id.filter(|id| *id > 0) {
                            let name = clean(&e.nationality);
                            let nation = nations.entry(id).or_default();
                            // Count every matching ID, even if one legacy row lacks the name.
                            // Do not invent a label when no indexed person supplies one.
                            nation.1 += 1;
                            if !name.is_empty() && (nation.0.is_empty() || name < nation.0) {
                                nation.0 = name;
                            }
                        }
                    }
                }
                let mut nations = nations.into_iter()
                    .filter(|(_, (name, _))| !name.is_empty())
                    .map(|(id, (name, count))| (id, name, count))
                    .collect::<Vec<_>>();
                nations.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
                let nations = nations.into_iter()
                    .map(|(id, name, count)| json!({"id":id,"name":name,"count":count}))
                    .collect::<Vec<_>>();
                let facet = |values: std::collections::BTreeMap<String, usize>| {
                    values
                        .into_iter()
                        .map(|(value, count)| json!({"value":value,"count":count}))
                        .collect::<Vec<_>>()
                };
                Ok(
                    json!({"roles":facet(roles),"groups":facet(groups),"football_categories":facet(categories),"nations":nations,"total":total,"employed":employed,"unemployed":total-employed-employment_unknown,"employment_unknown":employment_unknown}),
                )
            }
            "league_catalog" => {
                let refresh = match a.get("refresh") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(value)) => *value,
                    _ => bail!("리그 새로고침 조건은 참/거짓이어야 합니다"),
                };
                if refresh && self.live {
                    crate::leagues::refresh(self.index.as_mut().context("연결 없음")?)?;
                }
                let catalog = crate::leagues::catalog(self.index()?);
                let leagues = catalog.groups.iter().map(|group| json!({
                    "key":group.key,"id":group.id,"name":group.name,
                    "nation_id":group.nation_id,"nation":group.nation,
                    "football_category":group.football_category,
                    "club_count":group.clubs.len(),
                    "clubs":group.clubs.iter().map(entity).collect::<Vec<_>>()
                })).collect::<Vec<_>>();
                Ok(json!({"leagues":leagues,"total_clubs":catalog.total_clubs,
                    "known_clubs":catalog.known_clubs,"unclassified_clubs":catalog.total_clubs-catalog.known_clubs,
                    "live":self.live,"warnings":catalog.warnings}))
            }
            "compare" => {
                let refs = references(a)?;
                ensure!(refs.len() <= 4, "비교는 최대 4명입니다");
                let mut results = vec![];
                for r in refs {
                    let d = self.inspect_uncached(r.kind, r.id)?;
                    results.push(self.detail_json(&d));
                }
                Ok(json!({"details":results}))
            }
            "library_get" => Ok(serde_json::to_value(Library::read(
                &transaction::data_dir().join("scouting-library.json"),
            )?)?),
            "library_put" => {
                let library: Library =
                    serde_json::from_value(a.get("library").cloned().context("목록 자료 없음")?)?;
                if let Some(expected) = a.get("expected") {
                    Library::read(&transaction::data_dir().join("scouting-library.json"))?
                        .ensure_expected(&serde_json::from_value(expected.clone())?)?;
                }
                library.write(&transaction::data_dir().join("scouting-library.json"))?;
                Ok(serde_json::to_value(library)?)
            }
            "shortlist_update" => {
                let path = transaction::data_dir().join("scouting-library.json");
                let library = Library::read(&path)?;
                let expected: Library = serde_json::from_value(a.get("expected").cloned().context("기존 명단 정보 없음")?)?;
                library.ensure_expected(&expected)?;
                let action: crate::workspace::ShortlistAction = serde_json::from_value(a.get("change").cloned().context("명단 변경 내용 없음")?)?;
                let next = library.update_shortlist(action)?;
                next.write(&path)?;
                Ok(serde_json::to_value(next)?)
            }
            "library_import" => {
                let incoming = Library::read(Path::new(required_str(a, "path")?))?;
                Ok(json!({"library":incoming,"saved":false}))
            }
            "fm_shortlist_import" => {
                let path = Path::new(required_str(a, "path")?);
                let incoming = crate::workspace::fm_shortlists::read(path)?;
                Ok(json!({"library":incoming,"saved":false,"game_applied":false,
                    "format":"si_shortlist","compatibility":"fm27_runtime_unverified"}))
            }
            "fm_shortlist_export" => {
                let library = Library::read(&transaction::data_dir().join("scouting-library.json"))?;
                let expected: Library = serde_json::from_value(a.get("expected").cloned().context("기존 명단 정보 없음")?)?;
                library.ensure_expected(&expected)?;
                let selected = library.export_shortlist(required_str(a, "name")?)?;
                let kind: EntityKind = serde_json::from_value(a.get("kind").cloned().context("선수 또는 스태프 명단 종류를 선택하세요")?)?;
                let path = PathBuf::from(required_str(a, "path")?);
                crate::workspace::fm_shortlists::export(&path, &selected.lists[0], kind)?;
                Ok(json!({"path":path,"name":selected.lists[0].name,"kind":kind,
                    "count":selected.lists[0].members.len(),"file_saved":true,"game_applied":false,
                    "compatibility":"fm27_runtime_unverified",
                    "next_step":"FM의 해당 선수/스태프 관심명단에서 파일 불러오기가 필요합니다. FM27 실제 불러오기는 아직 검증되지 않았습니다."}))
            }
            "library_export" => {
                let mut l = Library::read(&transaction::data_dir().join("scouting-library.json"))?;
                if let Some(name) = a.get("name").and_then(|v| v.as_str()) { l = l.export_shortlist(name)?; }
                let path = PathBuf::from(required_str(a, "path")?);
                ensure!(!path.exists(), "새 파일 경로를 선택하세요");
                l.write(&path)?;
                Ok(json!({"path":path}))
            }
            "resolve_targets" => {
                let refs: Vec<Ref> =
                    serde_json::from_value(a.get("targets").cloned().context("대상 없음")?)?;
                ensure!(refs.len() <= 10000, "대상이 너무 많습니다");
                // Resolve the index once, rather than scanning every player for
                // each member of a large shortlist. Ambiguous identities remain
                // unresolved, just as with find().
                let wanted = refs.iter().map(|r| (r.kind, r.id)).collect::<std::collections::HashSet<_>>();
                let mut indexed = HashMap::<(EntityKind, u32), Option<&Entity>>::new();
                for e in &self.index()?.entities {
                    let key = (e.kind, e.id);
                    if wanted.contains(&key) {
                        indexed.entry(key).and_modify(|value| *value = None).or_insert(Some(e));
                    }
                }
                let mut found = vec![];
                let mut missing = vec![];
                for r in refs {
                    match indexed.get(&(r.kind, r.id)).copied().flatten() {
                        Some(e) => found.push(entity(e)),
                        None => missing.push(json!(r)),
                    }
                }
                Ok(json!({"items":found,"missing":missing}))
            }
            "batch_preview" => {
                let refs = references(a)?;
                let rules: Vec<Rule> =
                    serde_json::from_value(a.get("rules").cloned().context("규칙 없음")?)?;
                let mut items = vec![];
                let mut summaries = vec![];
                let mut unchanged = vec![];
                for r in refs {
                    let d = self.inspect_uncached(r.kind, r.id)?;
                    // Rules all set to current values are valid but do not generate writes.
                    let edits = crate::workspace::edits(&d, &rules)?;
                    let changes = transaction::prepare_allow_unchanged(&d, &edits)?;
                    if changes.is_empty() {
                        unchanged.push(entity(&d.entity));
                        continue;
                    }
                    summaries.push(json!({"entity":entity(&d.entity),"changes":changes.iter().map(change).collect::<Vec<_>>()}));
                    items.push((d, edits));
                }
                ensure!(!items.is_empty(), "선택한 대상에 실제 변경이 없습니다");
                let token = self.token(Preview::Batch(items));
                Ok(
                    json!({"token":token,"items":summaries,"unchanged":unchanged,"can_apply":self.live,"live":self.live}),
                )
            }
            "batch_apply" => {
                self.require_live()?;
                let Preview::Batch(items) = self.consume(required_str(a, "token")?)? else {
                    bail!("대량 변경 검토가 아닙니다")
                };
                let r = batch::apply_many(&self.index()?.session, &items)?;
                self.invalidate();
                Ok(batch_receipt(&r))
            }
            "batch_history" => Ok(json!(
                batch::history()
                    .iter()
                    .map(batch_receipt)
                    .collect::<Vec<_>>()
            )),
            "batch_undo" => {
                let session = self.require_live()?.clone();
                let id = required_str(a, "receipt_id")?;
                let r = batch::history()
                    .into_iter()
                    .find(|r| r.id == id)
                    .context("대량 변경 기록 없음")?;
                ensure!(
                    same_session(&session, &r.session),
                    "다른 게임 세션의 기록입니다"
                );
                let result = batch::undo_many(&r)?;
                self.invalidate();
                Ok(batch_receipt(&result))
            }
            "freeze_preview" => {
                let refs = references(a)?;
                ensure!(refs.len() <= 50, "고정은 최대 50명입니다");
                let rules: Vec<Rule> =
                    serde_json::from_value(a.get("rules").cloned().context("규칙 없음")?)?;
                let mut items = vec![];
                let mut summaries = vec![];
                let mut unchanged = vec![];
                for r in refs {
                    let d = self.inspect_uncached(r.kind, r.id)?;
                    let edits = crate::workspace::edits(&d, &rules)?;
                    let changes = transaction::prepare_allow_unchanged(&d, &edits)?;
                    if changes.is_empty() {
                        unchanged.push(entity(&d.entity));
                    }
                    let values = edits
                        .iter()
                        .map(|e| {
                            let f = d
                                .fields
                                .iter()
                                .find(|f| f.key == e.key)
                                .expect("validated rule field");
                            json!({"key":e.key,"label":f.label,"before":f.value,"after":e.value})
                        })
                        .collect::<Vec<_>>();
                    summaries.push(json!({"entity":entity(&d.entity),"values":values,"changes":changes.iter().map(change).collect::<Vec<_>>()}));
                    items.push((d, edits));
                }
                let token = self.token(Preview::Freeze(items));
                Ok(
                    json!({"token":token,"items":summaries,"unchanged":unchanged,"can_apply":self.live,"live":self.live}),
                )
            }
            "freeze_enable" => {
                self.require_live()?;
                let Preview::Freeze(items) = self.consume(required_str(a, "token")?)? else {
                    bail!("고정 전용 검토 결과가 아닙니다. 고정 검토를 다시 실행하세요")
                };
                ensure!(
                    !items.is_empty() && items.len() <= 50,
                    "고정은 1–50명입니다"
                );
                self.work.freeze = Some(Freeze {
                    targets: items
                        .into_iter()
                        .map(|(d, edits)| (d.entity, edits, d.guards))
                        .collect(),
                    last: None,
                    writes: 0,
                });
                Ok(self.freeze_status())
            }
            "freeze_status" => Ok(self.freeze_status()),
            "freeze_stop" => {
                self.work.freeze = None;
                Ok(self.freeze_status())
            }
            "freeze_tick" => {
                self.require_live()?;
                let Some(mut state) = self.work.freeze.take() else {
                    return Ok(self.freeze_status());
                };
                if state
                    .last
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
                {
                    self.work.freeze = Some(state);
                    return Ok(self.freeze_status());
                }
                state.last = Some(Instant::now());
                let result = (|| -> Result<()> {
                    let mut items = vec![];
                    for (e, values, guards) in &state.targets {
                        let d = self.inspect_uncached(e.kind, e.id)?;
                        ensure!(
                            &d.guards == guards,
                            "{}의 구조가 바뀌어 고정을 중단합니다",
                            e.name
                        );
                        let changed: Vec<_> = values
                            .iter()
                            .filter(|v| {
                                d.fields
                                    .iter()
                                    .find(|f| f.key == v.key)
                                    .is_none_or(|f| f.value != v.value)
                            })
                            .cloned()
                            .collect();
                        if !changed.is_empty() {
                            transaction::prepare(&d, &changed)?;
                            items.push((d, changed));
                        }
                    }
                    if !items.is_empty() {
                        let r = batch::apply_many(&self.index()?.session, &items)?;
                        ensure!(
                            r.status == "applied",
                            "고정 적용에 경고가 있습니다: {}",
                            r.status
                        );
                        state.writes += 1;
                        self.details.clear();
                        self.pending.clear();
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    return Err(anyhow::anyhow!("수치 고정을 중지했습니다: {e:#}"));
                }
                self.work.freeze = Some(state);
                Ok(self.freeze_status())
            }
            "export_rows" => {
                let refs = references(a)?;
                let path = PathBuf::from(required_str(a, "path")?);
                let mut csv =
                    String::from("\u{feff}종류,ID,이름,영문 이름,구단,나이,CA,PA,포지션,축구 구분\r\n");
                fn cell(s: &str) -> String {
                    let escaped = s.replace('"', "\"\"");
                    let safe = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
                        format!("'{escaped}")
                    } else {
                        escaped
                    };
                    format!("\"{safe}\"")
                }
                for r in refs {
                    let e = self.find(r.kind, r.id)?;
                    csv.push_str(
                        &[
                            cell(&format!("{:?}", e.kind)),
                            e.id.to_string(),
                            cell(&e.name),
                            cell(&e.english_name),
                            cell(&e.club),
                            e.age.map_or(String::new(), |v| v.to_string()),
                            e.ca.map_or(String::new(), |v| v.to_string()),
                            e.pa.map_or(String::new(), |v| v.to_string()),
                            cell(&e.role),
                            cell(e.football_category.label()),
                        ]
                        .join(","),
                    );
                    csv.push_str("\r\n");
                }
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                f.write_all(csv.as_bytes())?;
                f.sync_all()?;
                Ok(json!({"path":path}))
            }
            _ => bail!("알 수 없는 명령: {command}"),
        }
    }
    fn freeze_status(&self) -> Value {
        match &self.work.freeze {
            Some(s) => {
                json!({"active":true,"targets":s.targets.iter().map(|(e,v,_)|json!({"entity":entity(e),"values":v})).collect::<Vec<_>>(),"write_cycles":s.writes})
            }
            None => json!({"active":false,"targets":[],"write_cycles":0}),
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
            for file in [
                "detail-Player-7.json",
                "detail-Player-8.json",
                "detail-Staff-17.json",
                "detail-Staff-18.json",
            ] {
                let _ = std::fs::remove_file(self.folder.join(file));
            }
            let _ = std::fs::remove_dir(&self.folder);
        }
    }
    fn fixture() -> Fixture {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "touchline-freeze-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let mut entities = vec![];
        for (id, ca) in [(7, 100u16), (8, 110u16)] {
            let entity = Entity {
                nationality: String::new(),
                nationality_id: None,
                football_category: crate::model::FootballCategory::Unknown,
                id,
                kind: EntityKind::Player,
                name: format!("Fixture{id}"),
                english_name: String::new(),
                club: String::new(),
                club_id: None,
                age: None,
                ca: Some(ca),
                pa: Some(150),
                role: String::new(),
                group: String::new(),
                address: 0x10000 + id as u64 * 8,
                base: 0x10000,
                index: Some(id),
            };
            let detail = Detail {
                entity: entity.clone(),
                fields: [("ca", ca), ("pa", 150u16)]
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
                            address: 0x20000 + id as u64 * 8 + n as u64 * 2,
                            storage: Storage::U16,
                            scale: 1.,
                            original: v.to_le_bytes().to_vec(),
                        }),
                    })
                    .collect(),
                guards: vec![],
                notes: vec![],
            };
            std::fs::write(
                folder.join(format!("detail-Player-{id}.json")),
                serde_json::to_vec(&detail).unwrap(),
            )
            .unwrap();
            entities.push(entity);
        }
        let session = Session {
            pid: 0,
            created: 1,
            module_base: 0,
            module_size: 0,
            module_stamp: 0,
            db: 0,
            registry_begin: 0,
            game_date: "2027-11-26".into(),
            profile: "fixture".into(),
            own_club_id: None,
        };
        Fixture {
            bridge: Bridge {
                index: Some(Index {
                    club_leagues: Vec::new(),
                    session,
                    entities,
                    warnings: vec![],
                }),
                snapshot: Some(folder.join("index.json")),
                ..Bridge::default()
            },
            folder,
        }
    }
    fn args(ids: &[u32]) -> Value {
        json!({"targets":ids.iter().map(|id|json!({"kind":"Player","id":id})).collect::<Vec<_>>(),"rules":[{"key":"ca","operation":"set","value":100}]})
    }
    #[test]
    fn shortlist_resolution_preserves_order_kind_and_ambiguous_identity() {
        let mut f = fixture();
        let index = f.bridge.index.as_mut().unwrap();
        let duplicate = index.entities[0].clone();
        let mut staff = duplicate.clone();
        staff.kind = EntityKind::Staff;
        index.entities.extend([duplicate, staff]);
        let result = f.bridge.dispatch("resolve_targets", &json!({"targets":[
            {"kind":"Staff","id":7}, {"kind":"Player","id":8},
            {"kind":"Player","id":7}, {"kind":"Player","id":99}
        ]})).unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["items"][0]["kind"], "Staff");
        assert_eq!(result["items"][0]["id"], 7);
        assert_eq!(result["items"][1]["id"], 8);
        assert_eq!(result["missing"], json!([{"kind":"Player","id":7},{"kind":"Player","id":99}]));
        assert!(result["items"][0].get("address").is_none());
    }
    #[test]
    fn leagues_catalog_bridge_is_address_free_and_snapshot_refresh_never_reads_process() {
        let mut f = fixture();
        let index = f.bridge.index.as_mut().unwrap();
        for id in [101, 102, 103] {
            let mut club = index.entities[0].clone();
            club.id = id; club.kind = EntityKind::Club; club.name = format!("Synthetic club {id}");
            club.club_id = Some(id); club.football_category = FootballCategory::Men;
            index.entities.push(club);
        }
        index.club_leagues = vec![ClubLeague {club_id:101,league_id:Some(5),league:"Test League".into(),nation_id:Some(10),nation:"Test Nation".into()},
            ClubLeague {club_id:102,league_id:Some(5),league:"Test League".into(),nation_id:Some(10),nation:"Test Nation".into()}];
        let result = f.bridge.dispatch("league_catalog", &json!({"refresh":true})).unwrap();
        assert_eq!(result["live"], false); assert_eq!(result["total_clubs"], 3);
        assert_eq!(result["known_clubs"], 2); assert_eq!(result["unclassified_clubs"], 1);
        assert_eq!(result["leagues"].as_array().unwrap().len(), 2);
        let clubs = result["leagues"].as_array().unwrap().iter().flat_map(|group| group["clubs"].as_array().unwrap()).collect::<Vec<_>>();
        assert!(clubs.iter().all(|club| club["kind"] == "Club" && club.get("address").is_none() && club.get("base").is_none()));
        assert_eq!(clubs.len(), 3);
        assert!(f.bridge.dispatch("league_catalog", &json!({"refresh":"yes"})).is_err());
    }
    #[test]
    fn freeze_preview_retains_current_value_targets_and_hides_bindings() {
        let mut f = fixture();
        let p = f.bridge.dispatch("freeze_preview", &args(&[7])).unwrap();
        assert_eq!(p["items"].as_array().unwrap().len(), 1);
        assert_eq!(p["unchanged"].as_array().unwrap().len(), 1);
        assert_eq!(p["items"][0]["values"][0]["before"], 100.);
        assert_eq!(p["items"][0]["values"][0]["after"], 100.);
        assert!(p["items"][0]["changes"].as_array().unwrap().is_empty());
        assert_eq!(p["can_apply"], false);
        let public = p.to_string();
        for private in ["address", "binding", "guards", "original"] {
            assert!(!public.contains(private));
        }
        let Preview::Freeze(items) = f.bridge.consume(p["token"].as_str().unwrap()).unwrap() else {
            panic!("wrong preview type")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1[0].value, 100.);
    }
    #[test]
    fn freeze_keeps_matching_members_while_batch_omits_them() {
        let mut f = fixture();
        let frozen = f.bridge.dispatch("freeze_preview", &args(&[7, 8])).unwrap();
        assert_eq!(frozen["items"].as_array().unwrap().len(), 2);
        assert_eq!(frozen["unchanged"].as_array().unwrap().len(), 1);
        let batch = f.bridge.dispatch("batch_preview", &args(&[7, 8])).unwrap();
        assert_eq!(batch["items"].as_array().unwrap().len(), 1);
        assert_eq!(batch["items"][0]["entity"]["id"], 8);
        assert_eq!(batch["unchanged"][0]["id"], 7);
        assert!(f.bridge.dispatch("batch_preview", &args(&[7])).is_err());
    }
    #[test]
    fn freeze_enable_requires_its_own_single_use_token_without_memory_io() {
        let mut f = fixture();
        let frozen = f.bridge.dispatch("freeze_preview", &args(&[7])).unwrap();
        let batch = f.bridge.dispatch("batch_preview", &args(&[8])).unwrap();
        // Only exercise in-process token/state handling. Never call freeze_tick,
        // apply or a process-memory API with this synthetic session.
        f.bridge.live = true;
        assert!(
            f.bridge
                .dispatch("freeze_enable", &json!({"token":batch["token"]}))
                .is_err()
        );
        assert!(f.bridge.work.freeze.is_none());
        assert!(
            !f.bridge
                .pending
                .contains_key(batch["token"].as_str().unwrap())
        );
        let enabled = f
            .bridge
            .dispatch("freeze_enable", &json!({"token":frozen["token"]}))
            .unwrap();
        assert_eq!(enabled["active"], true);
        assert_eq!(enabled["write_cycles"], 0);
        assert_eq!(enabled["targets"][0]["values"][0]["value"], 100.);
        assert!(
            f.bridge
                .dispatch("freeze_enable", &json!({"token":frozen["token"]}))
                .is_err()
        );
        f.bridge.dispatch("freeze_stop", &json!({})).unwrap();
        assert!(f.bridge.work.freeze.is_none());
    }
    #[test]
    fn freeze_rejects_more_than_fifty_targets_before_loading_details() {
        let mut f = fixture();
        let ids = (1..=51).collect::<Vec<_>>();
        let error = f
            .bridge
            .dispatch("freeze_preview", &args(&ids))
            .unwrap_err();
        assert!(error.to_string().contains("50"));
        assert!(f.bridge.pending.is_empty());
    }
    fn start(f: &mut Fixture, args: Value) -> u64 {
        f.bridge.dispatch("query_start", &args).unwrap()["query_id"]
            .as_u64()
            .unwrap()
    }
    fn step(f: &mut Fixture, id: u64) -> Value {
        f.bridge
            .dispatch("query_step", &json!({"query_id":id}))
            .unwrap()
    }
    #[test]
    fn staff_query_and_facets_refresh_before_filtering_and_keep_query_candidates_stable() {
        let mut b=crate::bridge::tests::staff_refresh_fixture();b.live=true;
        let result=b.dispatch("query_start",&json!({"kind":"Staff","club_id":22,"filter":{"role":"스카우트"}})).unwrap();
        let id=result["query_id"].as_u64().unwrap();
        let candidates=&b.work.queries[&id].candidates;
        assert_eq!(candidates.len(),1);assert_eq!(candidates[0].club_id,Some(22));assert_eq!(candidates[0].role,"스카우트");
        b.staff_refresh_hook=Some(|index|{index.entities[0].club_id=Some(33);index.entities[0].role="감독".into();Ok(())});
        let facets=b.dispatch("search_facets",&json!({"kind":"Staff"})).unwrap();
        assert_eq!(facets["roles"][0]["value"],"감독");assert_eq!(b.work.queries[&id].candidates[0].club_id,Some(22));
    }
    #[test]
    fn staff_facets_separate_unknown_affiliation_from_known_unemployment_and_missing_job() {
        let mut b=crate::bridge::tests::staff_refresh_fixture();
        let rows=&mut b.index.as_mut().unwrap().entities;
        let mut unemployed=rows[0].clone();unemployed.id=8;unemployed.club_id=None;unemployed.club.clear();unemployed.role="무직".into();unemployed.group="무소속".into();
        let mut unknown=unemployed.clone();unknown.id=9;unknown.group=crate::model::STAFF_AFFILIATION_UNKNOWN.into();unknown.role="직무 미확인".into();
        let mut no_job=rows[0].clone();no_job.id=10;no_job.role.clear();rows.extend([unemployed,unknown,no_job]);
        let facets=b.dispatch("search_facets",&json!({"kind":"Staff"})).unwrap();
        assert_eq!(facets["total"],4);assert_eq!(facets["employed"],2);assert_eq!(facets["unemployed"],1);assert_eq!(facets["employment_unknown"],1);
        assert!(!facets["roles"].as_array().unwrap().iter().any(|r|r["value"]=="직무 미확인"));
        assert!(!facets["groups"].as_array().unwrap().iter().any(|r|r["value"]==crate::model::STAFF_AFFILIATION_UNKNOWN));
    }
    #[test]
    fn staff_overlay_preserves_scalar_baseline_and_detail_invalidates_only_its_own_token() {
        let mut f=fixture();
        let mut staff=load_snapshot_detail(f.bridge.snapshot.as_ref().unwrap(),EntityKind::Player,7).unwrap();
        staff.entity.kind=EntityKind::Staff;staff.entity.id=17;
        f.bridge.index.as_mut().unwrap().entities.push(staff.entity.clone());
        std::fs::write(f.folder.join("detail-Staff-17.json"),serde_json::to_vec(&staff).unwrap()).unwrap();
        f.bridge.dispatch("detail",&json!({"kind":"Staff","id":17})).unwrap();
        f.bridge.dispatch("detail",&json!({"kind":"Player","id":8})).unwrap();
        let staff_token=f.bridge.dispatch("preview",&json!({"kind":"Staff","id":17,"edits":[{"key":"ca","value":101}]})).unwrap()["token"].as_str().unwrap().to_string();
        let player_token=f.bridge.dispatch("preview",&json!({"kind":"Player","id":8,"edits":[{"key":"ca","value":111}]})).unwrap()["token"].as_str().unwrap().to_string();
        staff.fields[0].value=125.;staff.entity.club_id=Some(22);staff.entity.role="스카우트".into();
        std::fs::write(f.folder.join("detail-Staff-17.json"),serde_json::to_vec(&staff).unwrap()).unwrap();
        let overlay=f.bridge.dispatch("overlay_detail",&json!({"id":17})).unwrap();assert_eq!(overlay["fields"][0]["value"],125.);
        assert_eq!(f.bridge.details[&(EntityKind::Staff,17)].fields[0].value,100.);assert!(f.bridge.pending.contains_key(&staff_token));assert!(f.bridge.pending.contains_key(&player_token));
        f.bridge.dispatch("detail",&json!({"kind":"Staff","id":17})).unwrap();
        assert_eq!(f.bridge.details[&(EntityKind::Staff,17)].fields[0].value,125.);assert!(!f.bridge.pending.contains_key(&staff_token));assert!(f.bridge.pending.contains_key(&player_token));
    }
    #[test]
    fn detailed_conditions_keep_or_candidates_and_apply_and_precedence() {
        let mut f = fixture();
        let rules = json!([
            {"source":"field","key":"ca","op":"gte","value":110},
            {"join":"or","source":"entity","key":"name","op":"contains","value":"Fixture7"},
            {"join":"and","source":"entity","key":"employed","op":"eq","value":false}
        ]);
        let id = start(&mut f, json!({"kind":"Player","filter":{"conditions":rules}}));
        let result = step(&mut f,id);
        assert_eq!(result["matched"], 2);
        assert_eq!(result["failed"], 0);
        // Basic filters restrict the entire OR expression, not just one group.
        let id = start(&mut f, json!({"kind":"Player","filter":{"ca_min":105,"conditions":rules}}));
        let result = step(&mut f,id);
        assert_eq!(result["items"][0]["id"],8);
        assert_eq!(result["matched"],1);
        // false OR true AND false must remain false (AND before OR).
        let id = start(&mut f, json!({"kind":"Player","filter":{"conditions":[
            {"source":"entity","key":"ca","op":"gt","value":150},
            {"join":"or","source":"field","key":"pa","op":"eq","value":150},
            {"join":"and","source":"entity","key":"employed","op":"eq","value":true}
        ]}}));
        assert_eq!(step(&mut f,id)["matched"],0);
        // Conditions-only indexed queries do not require profile snapshots.
        let id = start(&mut f, json!({"kind":"Player","filter":{"conditions":[
            {"source":"entity","key":"id","op":"between","value":8,"value2":9}
        ]}}));
        assert_eq!(step(&mut f,id)["items"][0]["id"],8);
    }
    #[test]
    fn position_group_query_roundtrip_preserves_any_semantics_and_other_conditions() {
        let mut f=fixture();
        for (id,left,right) in [(7,20.,1.),(8,1.,20.)] {
            let path=f.folder.join(format!("detail-Player-{id}.json"));
            let mut detail:Detail=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            for (key,value) in [("position_dl",left),("position_dr",right)] {
                let mut field=detail.fields[0].clone();field.key=key.into();field.label=key.into();field.min=0.;field.max=20.;field.value=value;detail.fields.push(field);
            }
            std::fs::write(path,serde_json::to_vec(&detail).unwrap()).unwrap();
        }
        let filter=json!({"conditions":[{"source":"position","key":"any","op":"gte","value":20,"positions":["position_dl","position_dr"]}]});
        let saved:workspace::Filter=serde_json::from_value(filter).unwrap();
        let restored=serde_json::to_value(saved).unwrap();
        let id=start(&mut f,json!({"kind":"Player","filter":restored}));let result=step(&mut f,id);
        assert_eq!(result["matched"],2);assert_eq!(result["failed"],0);
        let mut constrained=restored.clone();constrained["conditions"].as_array_mut().unwrap().push(json!({"source":"entity","key":"ca","op":"gte","value":105}));
        let id=start(&mut f,json!({"kind":"Player","filter":constrained}));let result=step(&mut f,id);
        assert_eq!(result["matched"],1);assert_eq!(result["items"][0]["id"],8);
        let mut one=restored.clone();one["conditions"][0]["positions"]=json!(["position_dl"]);
        let id=start(&mut f,json!({"kind":"Player","filter":one}));let result=step(&mut f,id);
        assert_eq!(result["matched"],1);assert_eq!(result["items"][0]["id"],7);
        assert!(f.bridge.dispatch("search",&json!({"kind":"Player","filter":restored})).is_err());
    }
    #[test]
    fn detailed_missing_values_never_match_inequality_and_disabled_rows_are_ignored() {
        let mut f = fixture();
        let path = f.folder.join("detail-Player-8.json");
        let mut d: Detail = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        d.fields.retain(|field| field.key != "ca");
        std::fs::write(&path,serde_json::to_vec(&d).unwrap()).unwrap();
        let id = start(&mut f,json!({"kind":"Player","filter":{"conditions":[
            {"source":"field","key":"ca","op":"ne","value":100}
        ]}}));
        assert_eq!(step(&mut f,id)["matched"],0);
        let id = start(&mut f,json!({"kind":"Player","filter":{"conditions":[
            {"source":"field","key":"ca","op":"ne","value":100},
            {"join":"or","source":"entity","key":"id","op":"eq","value":8}
        ]}}));
        assert_eq!(step(&mut f,id)["items"][0]["id"],8);
        std::fs::remove_file(&path).unwrap();
        let id = start(&mut f,json!({"kind":"Player","filter":{"conditions":[
            {"source":"field","key":"ca","op":"ne","value":100},
            {"join":"or","source":"entity","key":"id","op":"eq","value":8}
        ]}}));
        let result=step(&mut f,id);
        assert_eq!(result["items"][0]["id"],8);
        assert_eq!(result["failed"],1);
        assert_eq!(result["items"][0]["missing_values"][0],"ca");
        let id = start(&mut f,json!({"kind":"Player","filter":{"conditions":[
            {"enabled":false,"source":"field","key":"unavailable_old_key","op":"eq","value":1},
            {"join":"or","source":"entity","key":"name","op":"eq","value":"  FIXTURE7 "}
        ]}}));
        assert_eq!(step(&mut f,id)["items"][0]["id"],7);
        // Missing primary nation is not treated as "not Italian".
        let id = start(&mut f,json!({"kind":"Player","filter":{"conditions":[
            {"source":"entity","key":"nationality_id","op":"ne","value":1}
        ]}}));
        assert_eq!(step(&mut f,id)["matched"],0);
    }
    #[test]
    fn detailed_condition_errors_reject_before_creating_a_cursor() {
        let mut f = fixture();
        for rule in [
            json!({"source":"entity","key":"address","op":"eq","value":1}),
            json!({"source":"entity","key":"employed","op":"gte","value":true}),
            json!({"source":"entity","key":"age","op":"contains","value":"10"}),
            json!({"source":"entity","key":"name","op":"eq","value":1}),
            json!({"source":"field","key":"ca","op":"between","value":180,"value2":100}),
            json!({"source":"field","key":"ca","op":"eq","value":201}),
            json!({"source":"field","key":"unsupported","op":"eq","value":1}),
            json!({"source":"entity","key":"id","op":"eq","value":1.5}),
            json!({"source":"entity","key":"football_category","op":"eq","value":"other"}),
            json!({"join":"xor","source":"entity","key":"ca","op":"eq","value":100})
        ] {
            assert!(f.bridge.dispatch("query_start",&json!({"kind":"Player","filter":{"conditions":[rule]}})).is_err());
            assert!(f.bridge.work.queries.is_empty());
        }
        let rule=json!({"source":"entity","key":"age","op":"gte","value":16});
        assert!(f.bridge.dispatch("query_start",&json!({"kind":"Player","filter":{"conditions":vec![rule;65]}})).is_err());
    }
    #[test]
    fn primary_nationality_is_indexed_for_players_and_staff_with_stable_pagination() {
        let mut f = fixture();
        let template = f.bridge.index.as_ref().unwrap().entities[0].clone();
        for kind in [EntityKind::Player, EntityKind::Staff] {
            let mut entities = vec![];
            for (nation_id, name) in [(3, "Italy"), (1, "France"), (2, "Germany")] {
                for duplicate in (0..75).rev() {
                    let mut e = template.clone();
                    e.id = nation_id * 100 + duplicate;
                    e.kind = kind;
                    e.nationality = name.into();
                    e.nationality_id = Some(nation_id);
                    entities.push(e);
                }
            }
            for (id, name) in [(901, ""), (900, "  ")] {
                let mut e = template.clone();
                e.id = id;
                e.kind = kind;
                e.nationality = name.into();
                e.nationality_id = None;
                entities.push(e);
            }
            f.bridge.index.as_mut().unwrap().entities = entities;
            for descending in [false, true] {
                let nations = if descending { vec![3, 2, 1] } else { vec![1, 2, 3] };
                let expected = nations.into_iter()
                    .flat_map(|nation| (0..75).map(move |duplicate| nation * 100 + duplicate))
                    .chain([900, 901]).collect::<Vec<u32>>();
                let mut simple = vec![];
                while simple.len() < expected.len() {
                    let page = f.bridge.dispatch("search", &json!({
                        "kind":kind,"sort":"nationality","descending":descending,
                        "offset":simple.len(),"limit":17,
                    })).unwrap();
                    assert_eq!(page["total"], expected.len());
                    simple.extend(page["items"].as_array().unwrap().iter().map(|row| row["id"].as_u64().unwrap() as u32));
                }
                assert_eq!(simple, expected);
                // IDs intentionally have no detail files: country sorting must
                // work entirely on the index, not hundreds of profile reads.
                let started = f.bridge.dispatch("query_start", &json!({
                    "kind":kind,"sort":"nationality","descending":descending,
                })).unwrap();
                assert_eq!(started["sorting"], "index");
                assert_eq!(started["field_keys"], json!([]));
                let mut advanced = vec![];
                let mut pages = 0;
                loop {
                    let page = step(&mut f, started["query_id"].as_u64().unwrap());
                    assert_eq!(page["failed"], 0);
                    for row in page["items"].as_array().unwrap() {
                        let id = row["id"].as_u64().unwrap() as u32;
                        if id < 900 {
                            assert_eq!(row["nationality_id"], id / 100);
                            assert!(row["nationality"].as_str().unwrap().len() > 0);
                        } else {
                            assert_eq!(row["nationality_id"], Value::Null);
                        }
                        assert!(row.get("address").is_none());
                        advanced.push(id);
                    }
                    pages += 1;
                    if page["complete"] == true { break; }
                }
                assert!(pages >= 2);
                assert_eq!(advanced, expected);
            }
        }
    }
    #[test]
    fn nationality_filter_agrees_across_simple_and_cursor_searches_without_detail_reads() {
        let mut f = fixture();
        let template = f.bridge.index.as_ref().unwrap().entities[0].clone();
        for kind in [EntityKind::Player, EntityKind::Staff, EntityKind::Person] {
            let mut entities = vec![];
            for n in (0..240).rev() {
                let mut e = template.clone();
                e.kind = kind;
                e.id = 1000 + n;
                e.nationality_id = Some(776);
                e.nationality = "대한민국".into();
                e.club_id = Some(1141);
                entities.push(e);
            }
            // Country names and club identity are intentionally misleading.
            for (id, nationality_id) in [(900, Some(777)), (901, None)] {
                let mut e = template.clone();
                e.kind = kind;
                e.id = id;
                e.nationality_id = nationality_id;
                e.nationality = "대한민국".into();
                e.club_id = Some(776);
                entities.push(e);
            }
            f.bridge.index.as_mut().unwrap().entities = entities;
            let expected = (1000..1240).collect::<Vec<_>>();
            let filter = json!({"nationality_id":776,"ca_min":100,"ca_max":100,"employed":true});
            let mut simple = vec![];
            while simple.len() < expected.len() {
                let page = f.bridge.dispatch("search", &json!({
                    "kind":kind,"sort":"nationality","filter":filter,
                    "offset":simple.len(),"limit":17,
                })).unwrap();
                assert_eq!(page["total"], expected.len());
                simple.extend(page["items"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap() as u32));
            }
            assert_eq!(simple, expected);
            let started = f.bridge.dispatch("query_start", &json!({"kind":kind,"sort":"nationality","filter":filter})).unwrap();
            assert_eq!(started["field_keys"], json!([]));
            let mut advanced = vec![];
            let mut pages = 0;
            loop {
                let page = step(&mut f, started["query_id"].as_u64().unwrap());
                assert_eq!(page["failed"], 0);
                advanced.extend(page["items"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap() as u32));
                pages += 1;
                if page["complete"] == true { break; }
            }
            assert!(pages >= 2);
            assert_eq!(advanced, expected);
            for command in ["search", "query_start"] {
                let unknown = f.bridge.dispatch(command, &json!({"kind":kind,"filter":{"nationality_id":999}})).unwrap();
                assert_eq!(unknown["total"], 0);
                assert!(f.bridge.dispatch(command, &json!({"kind":kind,"filter":{"nationality_id":0}})).is_err());
            }
        }
    }
    #[test]
    fn nationality_facets_use_unique_primary_ids_and_exclude_nonperson_kinds() {
        let mut f = fixture();
        let template = f.bridge.index.as_ref().unwrap().entities[0].clone();
        for kind in EntityKind::ALL {
            let mut entities = vec![];
            for (n, (id, label)) in [
                (Some(776), "  대한민국  "), (Some(776), "대한민국"),
                (Some(776), ""), (Some(777), "대한민국"),
                (Some(100), "Italy"), (None, "Unknown"), (Some(0), "Invalid"),
                (Some(888), "\t"),
            ].into_iter().enumerate() {
                let mut e = template.clone();
                e.id = 1000 + n as u32;
                e.kind = kind;
                e.nationality_id = id;
                e.nationality = label.into();
                entities.push(e);
            }
            // Another entity type must never contribute to this kind's counts.
            let mut other = template.clone();
            other.kind = if kind == EntityKind::Player { EntityKind::Staff } else { EntityKind::Player };
            other.nationality_id = Some(776);
            other.nationality = "대한민국".into();
            entities.push(other);
            f.bridge.index.as_mut().unwrap().entities = entities;
            let facets = f.bridge.dispatch("search_facets", &json!({"kind":kind})).unwrap();
            assert_eq!(facets["total"], 8);
            if matches!(kind, EntityKind::Player | EntityKind::Staff | EntityKind::Person) {
                assert_eq!(facets["nations"], json!([
                    {"id":100,"name":"Italy","count":1},
                    {"id":776,"name":"대한민국","count":3},
                    {"id":777,"name":"대한민국","count":1},
                ]));
            } else {
                assert_eq!(facets["nations"], json!([]));
            }
        }
    }
    #[test]
    fn searches_keep_independent_cursors_and_complete_without_replacing_each_other() {
        let mut f = fixture();
        let first = start(&mut f, json!({"kind":"Player","query":"7"}));
        let second = start(&mut f, json!({"kind":"Player","query":"8"}));
        let first_rows = step(&mut f, first);
        assert_eq!(first_rows["items"][0]["id"], 7);
        assert_eq!(first_rows["complete"], true);
        assert!(f.bridge.work.queries.contains_key(&second));
        let second_rows = step(&mut f, second);
        assert_eq!(second_rows["items"][0]["id"], 8);
        assert_eq!(second_rows["complete"], true);
        assert!(f.bridge.work.queries.is_empty());
    }
    #[test]
    fn numeric_steps_read_captured_candidates_without_relooking_up_the_index() {
        let mut f = fixture();
        let id = start(
            &mut f,
            json!({"kind":"Player","fields":["ca"],"sort":"name"}),
        );
        // Synthetic regression trap: a repeated UID lookup would now fail.
        // Real connection replacement always invalidates cursors, while live
        // detail reads still verify registry/object/session guards themselves.
        f.bridge.index.as_mut().unwrap().entities.clear();
        let result = step(&mut f, id);
        assert_eq!(result["failed"], 0);
        assert_eq!(result["matched"], 2);
        assert_eq!(result["items"][0]["name"], "Fixture7");
        assert_eq!(result["items"][0]["values"]["ca"], 100.);
        assert_eq!(result["items"][1]["values"]["ca"], 110.);
    }
    #[test]
    fn query_start_rejects_duplicate_kind_uids_once_before_creating_cursor() {
        let mut f = fixture();
        let mut duplicate = f.bridge.index.as_ref().unwrap().entities[0].clone();
        duplicate.name = "Different name outside the filter".into();
        f.bridge.index.as_mut().unwrap().entities.push(duplicate);
        let result = f
            .bridge
            .dispatch("query_start", &json!({"kind":"Player","query":"8"}));
        assert!(result.unwrap_err().to_string().contains("중복"));
        assert!(f.bridge.work.queries.is_empty());
        f.bridge
            .index
            .as_mut()
            .unwrap()
            .entities
            .last_mut()
            .unwrap()
            .kind = EntityKind::Staff;
        let id = start(&mut f, json!({"kind":"Player","query":"8"}));
        assert_eq!(step(&mut f, id)["matched"], 1);
    }
    #[test]
    fn failed_step_session_validation_ends_only_that_cursor_once() {
        let mut f = fixture();
        let first = start(&mut f, json!({"kind":"Player"}));
        let second = start(&mut f, json!({"kind":"Player"}));
        let validations = std::cell::Cell::new(0);
        let result = f.bridge.work.take_validated_query(first, || {
            validations.set(validations.get() + 1);
            bail!("synthetic session ended")
        });
        assert!(result.err().unwrap().to_string().contains("session ended"));
        assert_eq!(validations.get(), 1);
        assert!(!f.bridge.work.queries.contains_key(&first));
        assert_eq!(f.bridge.work.queries.get(&second).unwrap().cursor, 0);
        assert!(
            f.bridge
                .work
                .take_validated_query(first, || {
                    validations.set(validations.get() + 1);
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(validations.get(), 1);
        assert_eq!(step(&mut f, second)["matched"], 2);
    }
    #[test]
    fn cancel_only_removes_named_cursor_and_legacy_cancel_only_targets_latest() {
        let mut f = fixture();
        let a = start(&mut f, json!({"kind":"Player"}));
        let b = start(&mut f, json!({"kind":"Player"}));
        assert_eq!(
            f.bridge
                .dispatch("query_cancel", &json!({"query_id":a}))
                .unwrap()["stopped"],
            true
        );
        assert!(f.bridge.work.queries.contains_key(&b));
        let c = start(&mut f, json!({"kind":"Player"}));
        let canceled = f.bridge.dispatch("query_cancel", &json!({})).unwrap();
        assert_eq!(canceled["query_id"], c);
        assert!(f.bridge.work.queries.contains_key(&b));
        assert_eq!(
            f.bridge.dispatch("query_cancel", &json!({})).unwrap()["stopped"],
            false
        );
        assert_eq!(step(&mut f, b)["matched"], 2);
    }
    #[test]
    fn active_search_limit_never_evicts_an_existing_search() {
        let mut f = fixture();
        let ids = (0..4)
            .map(|_| start(&mut f, json!({"kind":"Player"})))
            .collect::<Vec<_>>();
        assert!(
            f.bridge
                .dispatch("query_start", &json!({"kind":"Player"}))
                .is_err()
        );
        for id in &ids {
            assert!(f.bridge.work.queries.contains_key(id));
        }
        assert_eq!(step(&mut f, ids[0])["complete"], true);
        let fifth = start(&mut f, json!({"kind":"Player"}));
        assert!(fifth > ids[3]);
    }
    #[test]
    fn indexed_search_and_facets_apply_exact_roles_group_club_and_employment() {
        let mut f = fixture();
        let entities = &mut f.bridge.index.as_mut().unwrap().entities;
        entities[0].name = "이 강인".into();
        entities[0].english_name = "Kang In Lee".into();
        entities[0].club = "Paris Saint-Germain".into();
        entities[0].club_id = Some(44);
        entities[0].role = "AMC · MC · MC".into();
        entities[0].group = "1군".into();
        entities[0].age = Some(24);
        entities[1].role = "AMC".into();
        entities[1].group = "U21".into();
        entities[1].age = Some(19);
        let facets = f
            .bridge
            .dispatch("search_facets", &json!({"kind":"Player"}))
            .unwrap();
        assert_eq!(facets["total"], 2);
        assert_eq!(facets["employed"], 1);
        assert_eq!(facets["unemployed"], 1);
        assert!(
            facets["roles"]
                .as_array()
                .unwrap()
                .contains(&json!({"value":"AMC","count":2}))
        );
        assert!(
            facets["roles"]
                .as_array()
                .unwrap()
                .contains(&json!({"value":"MC","count":1}))
        );
        let id = start(
            &mut f,
            json!({"kind":"Player","query":"LEE 강인","sort":"club","filter":{"club_query":"PARIS germain","role":"MC","group":"1군","employed":true,"age_min":24,"age_max":24,"ca_min":100,"ca_max":100,"pa_min":150,"pa_max":150}}),
        );
        let rows = step(&mut f, id);
        assert_eq!(rows["total"], 1);
        assert_eq!(rows["items"][0]["id"], 7);
        let id = start(
            &mut f,
            json!({"kind":"Player","sort":"role","filter":{"unemployed":true}}),
        );
        assert_eq!(step(&mut f, id)["items"][0]["id"], 8);
    }
    fn paged_role_ids(f: &mut Fixture, kind: EntityKind, descending: bool, filter: Value) -> Vec<u32> {
        let mut simple = vec![];
        let mut offset = 0;
        loop {
            let page = f.bridge.dispatch("search", &json!({
                "kind":kind,"sort":"role","descending":descending,
                "offset":offset,"limit":17,"filter":filter,
            })).unwrap();
            simple.extend(page["items"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap() as u32));
            offset = simple.len();
            if offset == page["total"].as_u64().unwrap() as usize { break }
        }
        let id = start(f, json!({"kind":kind,"sort":"role","descending":descending,"filter":filter}));
        let mut advanced = vec![];
        let mut pages = 0;
        loop {
            let page = step(f, id);
            pages += 1;
            advanced.extend(page["items"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap() as u32));
            if page["complete"] == true { break }
        }
        if simple.len() > 200 { assert!(pages >= 2); }
        assert_eq!(simple, advanced, "simple and cursor searches must agree across pages");
        simple
    }
    #[test]
    fn tactical_role_sort_all_positions_compounds_unknowns_and_both_pagination_directions() {
        let mut f = fixture();
        let template = f.bridge.index.as_ref().unwrap().entities[0].clone();
        // Explicit rank/label pairs are intentionally neither lexical nor tactical input order.
        let scrambled = [(14,"ST"),(0,"GK"),(12,"AMC"),(2,"DR"),(6,"WBL"),(11,"AMR"),
            (3,"DC"),(10,"ML"),(1,"SW"),(13,"AML"),(7,"DM"),(5,"WBR"),(9,"MC"),(4,"DL"),(8,"MR")];
        let mut entities = vec![];
        for (rank, role) in scrambled {
            for duplicate in (0..16).rev() {
                let mut e = template.clone();
                e.id = rank * 100 + duplicate + 1;
                e.role = role.into();
                e.name = format!("Reverse name {}", 9999 - e.id);
                entities.push(e);
            }
        }
        for (id, role) in [(3002,"ST / DC"),(3001,"DC · ST"),(4002,""),(4001,"unknown")] {
            let mut e = template.clone();
            e.id = id;
            e.role = role.into();
            entities.push(e);
        }
        f.bridge.index.as_mut().unwrap().entities = entities;
        for descending in [false, true] {
            let mut ranks = (0..15).collect::<Vec<_>>();
            if descending { ranks.reverse(); }
            let mut expected = vec![];
            for rank in ranks {
                expected.extend((1..=16).map(|duplicate| rank * 100 + duplicate));
                if rank == 3 { expected.extend([3001,3002]); }
            }
            expected.extend([4001,4002]);
            assert_eq!(paged_role_ids(&mut f, EntityKind::Player, descending, json!({})), expected);
        }
    }
    #[test]
    fn staff_role_sort_remains_job_text_and_keeps_football_category_filtering() {
        let mut f = fixture();
        let template = f.bridge.index.as_ref().unwrap().entities[0].clone();
        f.bridge.index.as_mut().unwrap().entities = [
            (10,"ST",FootballCategory::Women),
            (12,"GK",FootballCategory::Women),
            (11,"GK",FootballCategory::Women),
            (13,"AMC / MC",FootballCategory::Women),
            (14,"코치",FootballCategory::Women),
            (15,"수석 코치",FootballCategory::Women),
            (16,"AMC",FootballCategory::Men),
        ].into_iter().map(|(id,role,category)| {
            let mut e = template.clone();
            e.kind = EntityKind::Staff;
            e.id = id;
            e.role = role.into();
            e.football_category = category;
            e
        }).collect();
        let filter = json!({"football_category":"women"});
        assert_eq!(paged_role_ids(&mut f, EntityKind::Staff, false, filter.clone()), vec![13,11,12,10,15,14]);
        assert_eq!(paged_role_ids(&mut f, EntityKind::Staff, true, filter), vec![14,15,10,11,12,13]);
        assert_eq!(paged_role_ids(&mut f, EntityKind::Staff, false, json!({"football_category":"men"})), vec![16]);
    }
    #[test]
    fn numeric_search_returns_selected_filter_and_sort_values_and_rejects_typos() {
        let mut f = fixture();
        let begun=f.bridge.dispatch("query_start",&json!({"kind":"Player","fields":["ca"],"sort_field":"pa","filter":{"fields":[{"key":"ca","min":105,"max":120}]}})).unwrap();
        assert_eq!(begun["field_keys"], json!(["ca", "pa"]));
        assert_eq!(begun["sorting"], "client");
        let result = step(&mut f, begun["query_id"].as_u64().unwrap());
        assert_eq!(result["matched"], 1);
        assert_eq!(result["scanned"], 2);
        assert_eq!(result["items"][0]["id"], 8);
        assert_eq!(result["items"][0]["values"], json!({"ca":110.,"pa":150.}));
        assert_eq!(result["items"][0]["missing_values"], json!([]));
        for args in [
            json!({"kind":"Player","filter":{"fields":[{"key":"typo","min":0}]}}),
            json!({"kind":"Player","fields":["not_a_field"]}),
            json!({"kind":"Player","sort_field":"missing"}),
            json!({"kind":"Player","filter":{"fields":[{"key":"ca","max":201}]}}),
        ] {
            assert!(f.bridge.dispatch("query_start", &args).is_err());
            assert!(f.bridge.work.queries.is_empty());
        }
        for private in ["address", "binding", "guards", "original"] {
            assert!(!result.to_string().contains(private));
        }
    }
    #[test]
    fn football_category_filters_indexed_numeric_and_paged_search_before_results() {
        let mut f = fixture();
        let rows = &mut f.bridge.index.as_mut().unwrap().entities;
        rows[0].football_category = FootballCategory::Women;
        rows[1].football_category = FootballCategory::Men;
        let mut unknown = rows[1].clone(); unknown.id = 9; unknown.football_category = FootballCategory::Unknown;
        rows.push(unknown);
        for k in [EntityKind::Player, EntityKind::Staff, EntityKind::Club] {
            for e in &mut f.bridge.index.as_mut().unwrap().entities { e.kind = k; }
            for (category, expected) in [("men", 8), ("women", 7), ("unknown", 9)] {
                let args = json!({"kind":k,"filter":{"football_category":category}});
                let first = f.bridge.dispatch("search", &args).unwrap();
                assert_eq!(first["total"], 1);
                assert_eq!(first["items"][0]["id"], expected);
                assert_eq!(first["items"][0]["football_category"], category);
                let id = start(&mut f, args);
                let result = step(&mut f, id);
                assert_eq!(result["total"], 1);
                assert_eq!(result["items"][0]["id"], expected);
            }
            let all = f.bridge.dispatch("search", &json!({"kind":k,"filter":{"football_category":null}})).unwrap();
            assert_eq!(all["total"], 3);
            let paged = f.bridge.dispatch("search", &json!({"kind":k,"filter":{"football_category":"men"},"limit":1,"offset":1})).unwrap();
            assert_eq!(paged["total"], 1);
            assert_eq!(paged["items"], json!([]));
        }
        for e in &mut f.bridge.index.as_mut().unwrap().entities { e.kind = EntityKind::Player; }
        let id = start(&mut f, json!({"kind":"Player","filter":{"football_category":"men","fields":[{"key":"ca","min":105}]}}));
        let result = step(&mut f, id);
        assert_eq!(result["total"], 1);
        assert_eq!(result["items"][0]["id"], 8);
        assert_eq!(result["failed"], 0);
        let facets = f.bridge.dispatch("search_facets", &json!({"kind":"Player"})).unwrap();
        assert!(facets["football_categories"].as_array().unwrap().contains(&json!({"value":"unknown","count":1})));
    }
    #[test]
    fn missing_numeric_cells_are_explicit_and_never_zero() {
        let mut f = fixture();
        let path = f.folder.join("detail-Player-8.json");
        let mut d: Detail = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        d.fields.retain(|f| f.key != "pa");
        std::fs::write(path, serde_json::to_vec(&d).unwrap()).unwrap();
        let id = start(
            &mut f,
            json!({"kind":"Player","sort":"name","fields":["pa"]}),
        );
        let result = step(&mut f, id);
        assert_eq!(result["items"][1]["id"], 8);
        assert_eq!(result["items"][1]["values"], json!({}));
        assert_eq!(result["items"][1]["missing_values"], json!(["pa"]));
    }
    #[test]
    fn optional_contract_columns_keep_unemployed_players_and_staff_without_inventing_values() {
        for kind in [EntityKind::Player, EntityKind::Staff] {
            let mut f = fixture();
            for n in 0..2 {
                let path = f.folder.join(format!("detail-Player-{}.json", 7 + n));
                let mut d: Detail = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                d.entity.kind = kind;
                if kind == EntityKind::Staff { d.entity.id += 10; }
                if n == 1 {
                    d.entity.club = "Club".into();
                    d.entity.club_id = Some(1141);
                    for (key, value, min, max) in [
                        ("contract.expires", 20290630., 19000101., 21001231.),
                        ("contract.weekly_wage", 4000., 0., 100000000.),
                    ] {
                        d.fields.push(Field { key:key.into(), label:key.into(), section:"계약".into(),
                            value, min, max, step:1., hint:String::new(), binding:None });
                    }
                }
                f.bridge.index.as_mut().unwrap().entities[n as usize] = d.entity.clone();
                std::fs::write(f.folder.join(format!("detail-{kind:?}-{}.json", d.entity.id)), serde_json::to_vec(&d).unwrap()).unwrap();
            }
            // The initial sample has no contract. Validate selected fields from
            // the other actual profile, then preserve the unemployed row.
            let catalog = f.bridge.dispatch("field_catalog", &json!({"kind":kind})).unwrap();
            assert!(!catalog["fields"].as_array().unwrap().iter().any(|f| f["key"] == "contract.weekly_wage"));
            for key in ["contract.weekly_wage", "contract.expires"] {
                let column = catalog["column_fields"].as_array().unwrap().iter().find(|f| f["key"] == key).unwrap();
                assert!(column.get("value").is_none(), "union metadata must not pretend another person's value belongs to the sample");
                assert!(column.get("editable").is_none());
                assert!(column["label"].is_string());
                assert!(column["min"].is_number());
            }
            let id = start(&mut f, json!({"kind":kind,"sort":"name","fields":["contract.expires","contract.weekly_wage"]}));
            let rows = step(&mut f, id);
            assert_eq!(rows["matched"], 2);
            assert_eq!(rows["failed"], 0);
            assert_eq!(rows["items"][0]["values"], json!({}));
            assert_eq!(rows["items"][0]["missing_values"], json!(["contract.expires","contract.weekly_wage"]));
            assert_eq!(rows["items"][1]["values"], json!({"contract.expires":20290630.,"contract.weekly_wage":4000.}));
            // A missing numeric condition is a mismatch, never a zero wage.
            let id = start(&mut f, json!({"kind":kind,"fields":["contract.expires"],
                "filter":{"fields":[{"key":"contract.weekly_wage","min":0,"max":5000}]}}));
            let rows = step(&mut f, id);
            assert_eq!(rows["matched"], 1);
            assert_eq!(rows["failed"], 0);
            assert_eq!(rows["items"][0]["values"]["contract.weekly_wage"], 4000.);
            // Numeric sort retains missing cells for the client's null-last sort.
            let id = start(&mut f, json!({"kind":kind,"sort_field":"contract.expires"}));
            let rows = step(&mut f, id);
            assert_eq!(rows["matched"], 2);
            assert_eq!(rows["sorting"], "client");
            assert_eq!(rows["items"][0]["missing_values"], json!(["contract.expires"]));
            assert!(f.bridge.dispatch("query_start", &json!({"kind":kind,"fields":["contract.wage_typo"]})).is_err());
        }
    }
    #[test]
    fn offline_catalog_uses_an_available_profile_when_first_indexed_profile_is_missing() {
        let mut f = fixture();
        std::fs::remove_file(f.folder.join("detail-Player-7.json")).unwrap();
        let catalog = f
            .bridge
            .dispatch("field_catalog", &json!({"kind":"Player"}))
            .unwrap();
        assert_eq!(catalog["sample"]["id"], 8);
        assert_eq!(catalog["live"], false);
        assert_eq!(catalog["fields"][0]["key"], "ca");
        for key in [
            "label", "section", "value", "min", "max", "step", "hint", "editable",
        ] {
            assert!(catalog["fields"][0].get(key).is_some());
        }
    }
    fn staff_fixture() -> Fixture {
        let mut f = fixture();
        let mut staff = f.bridge.index.as_ref().unwrap().entities[0].clone();
        staff.id = 17;
        staff.kind = EntityKind::Staff;
        f.bridge
            .index
            .as_mut()
            .unwrap()
            .entities
            .push(staff.clone());
        let keys = [
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
        let d = Detail {
            entity: staff,
            fields: keys
                .into_iter()
                .map(|key| Field {
                    key: key.into(),
                    label: key.into(),
                    section: "Staff".into(),
                    value: 20.,
                    min: 1.,
                    max: 20.,
                    step: 1.,
                    hint: String::new(),
                    binding: None,
                })
                .collect(),
            guards: vec![],
            notes: vec![],
        };
        std::fs::write(
            f.folder.join("detail-Staff-17.json"),
            serde_json::to_vec(&d).unwrap(),
        )
        .unwrap();
        f
    }
    #[test]
    fn staff_score_catalog_and_search_are_derived_from_complete_offline_inputs() {
        let mut f = staff_fixture();
        let catalog = f
            .bridge
            .dispatch("field_catalog", &json!({"kind":"Staff"}))
            .unwrap();
        let scores = catalog["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|field| field["key"].as_str().unwrap().starts_with("score_"))
            .collect::<Vec<_>>();
        assert_eq!(scores.len(), 27);
        assert!(
            scores
                .iter()
                .all(|field| field["value"] == 300. && field["editable"] == false)
        );
        let id = start(
            &mut f,
            json!({"kind":"Staff","sort":"score_fitness","fields":["score_scout"],"filter":{"fields":[{"key":"score_fitness","min":300,"max":300}]}}),
        );
        let result = step(&mut f, id);
        assert_eq!(result["matched"], 1);
        assert_eq!(result["sort_field"], "score_fitness");
        assert_eq!(
            result["items"][0]["values"],
            json!({"score_fitness":300.,"score_scout":300.})
        );
        let reloaded: Detail =
            serde_json::from_slice(&std::fs::read(f.folder.join("detail-Staff-17.json")).unwrap())
                .unwrap();
        assert_eq!(reloaded.fields.len(), 24);
        assert!(f.bridge.details.is_empty());
    }
    #[test]
    fn all_twenty_three_staff_attribute_conditions_are_applied_without_truncation() {
        let mut f = staff_fixture();
        let path = f.folder.join("detail-Staff-17.json");
        let mut second: Detail = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let keys = second
            .fields
            .iter()
            .filter(|field| field.key.starts_with("staff_"))
            .map(|field| field.key.clone())
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 23);
        second.entity.id = 18;
        second.entity.name = "Fails final condition".into();
        second
            .fields
            .iter_mut()
            .find(|field| field.key == keys[22])
            .unwrap()
            .value = 19.;
        f.bridge
            .index
            .as_mut()
            .unwrap()
            .entities
            .push(second.entity.clone());
        std::fs::write(
            f.folder.join("detail-Staff-18.json"),
            serde_json::to_vec(&second).unwrap(),
        )
        .unwrap();
        let ranges = keys
            .iter()
            .map(|key| json!({"key":key,"min":20,"max":20}))
            .collect::<Vec<_>>();
        // Repeated selected/sort keys are coalesced in the output; conditions
        // remain unique and include the final (23rd) staff attribute.
        let mut selected = keys.clone();
        selected.push(keys[0].clone());
        let begun=f.bridge.dispatch("query_start",&json!({"kind":"Staff","fields":selected,"sort_field":keys[0],"filter":{"fields":ranges}})).unwrap();
        assert_eq!(begun["field_keys"].as_array().unwrap().len(), 23);
        let rows = step(&mut f, begun["query_id"].as_u64().unwrap());
        assert_eq!(rows["scanned"], 2);
        assert_eq!(rows["matched"], 1);
        assert_eq!(rows["failed"], 0);
        assert_eq!(rows["items"][0]["id"], 17);
        assert_eq!(rows["items"][0]["values"].as_object().unwrap().len(), 23);
        assert_eq!(rows["items"][0]["values"][&keys[22]], 20.);
        assert_eq!(rows["items"][0]["missing_values"], json!([]));
    }
    #[test]
    fn numeric_display_and_condition_limits_accept_sixty_four_and_reject_sixty_five() {
        let mut f = staff_fixture();
        let path = f.folder.join("detail-Staff-17.json");
        let mut detail: Detail = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        crate::workspace::ensure_scores(&mut detail);
        // Synthetic catalog entries exercise the transport boundary without
        // claiming additional FM fields or touching process memory.
        for n in detail.fields.len()..65 {
            detail.fields.push(Field {
                key: format!("fixture_numeric_{n}"),
                label: format!("Fixture {n}"),
                section: "Fixture".into(),
                value: n as f64,
                min: 0.,
                max: 100.,
                step: 1.,
                hint: String::new(),
                binding: None,
            });
        }
        std::fs::write(&path, serde_json::to_vec(&detail).unwrap()).unwrap();
        let all_keys = detail
            .fields
            .iter()
            .map(|field| field.key.clone())
            .collect::<Vec<_>>();
        let ranges = detail
            .fields
            .iter()
            .map(|field| json!({"key":field.key,"min":field.value,"max":field.value}))
            .collect::<Vec<_>>();
        assert_eq!(all_keys.len(), 65);
        let begun=f.bridge.dispatch("query_start",&json!({"kind":"Staff","fields":&all_keys[..64],"sort_field":all_keys[0],"filter":{"fields":&ranges[..64]}})).unwrap();
        assert_eq!(begun["field_keys"].as_array().unwrap().len(), 64);
        let rows = step(&mut f, begun["query_id"].as_u64().unwrap());
        assert_eq!(rows["matched"], 1);
        assert_eq!(rows["items"][0]["values"].as_object().unwrap().len(), 64);
        assert_eq!(
            rows["items"][0]["values"][&all_keys[63]],
            detail.fields[63].value
        );
        for request in [
            json!({"kind":"Staff","fields":all_keys}),
            json!({"kind":"Staff","filter":{"fields":ranges}}),
        ] {
            let error = f.bridge.dispatch("query_start", &request).unwrap_err();
            assert!(error.to_string().contains("64"));
            assert!(f.bridge.work.queries.is_empty());
        }
        let mut duplicate = ranges[..64].to_vec();
        duplicate[63] = duplicate[0].clone();
        let error = f
            .bridge
            .dispatch(
                "query_start",
                &json!({"kind":"Staff","filter":{"fields":duplicate}}),
            )
            .unwrap_err();
        assert!(error.to_string().contains("중복"));
        assert!(f.bridge.work.queries.is_empty());
    }
}
