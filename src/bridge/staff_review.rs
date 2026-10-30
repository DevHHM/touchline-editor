use super::*;
use crate::staff_review::{self as advice, Row};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ReviewState {
    queries: HashMap<u64, Review>,
}
struct Review {
    id: u64,
    club: Entity,
    club_reputation: Option<f64>,
    incumbents: Vec<Entity>,
    candidates: Vec<Entity>,
    incumbent_cursor: usize,
    candidate_cursor: usize,
    failed_incumbents: usize,
    failed_candidates: usize,
    eligible_candidates: usize,
    rows: Vec<Row>,
    club_reputations: HashMap<u32, Option<f64>>,
    club_entities: HashMap<u32, Entity>,
    warnings: Vec<String>,
}
impl Review {
    fn complete(&self) -> bool {
        self.incumbent_cursor == self.incumbents.len()
            && self.candidate_cursor == self.candidates.len()
    }
    fn response(&self, cancelled: bool) -> Value {
        let complete = self.complete() && !cancelled;
        let mut rows = self.rows.clone();
        for row in &mut rows {
            advice::finish(row, self.club_reputation, complete);
            if complete
                && self.candidate_cursor > 0
                && self.failed_candidates == self.candidate_cursor
                && row.score.is_some_and(|score| score < advice::STRONG_SCORE)
            {
                row.status = "insufficient".into();
                row.status_label = "자료 부족".into();
                row.reasons
                    .retain(|reason| !reason.contains("대체자를 찾지 못함"));
                row.reasons
                    .push("외부 후보 상세를 모두 읽지 못해 대체자 유무를 판단할 수 없음".into());
            }
        }
        rows.sort_by(|a, b| {
            let rank = |row: &Row| match row.status.as_str() {
                "replace_review" => 0,
                "insufficient" => 1,
                "compare" => 2,
                _ => 3,
            };
            rank(a)
                .cmp(&rank(b))
                .then(a.group.cmp(&b.group))
                .then(a.person.name.cmp(&b.person.name))
        });
        json!({"review_id":self.id,"scanned":self.incumbent_cursor+self.candidate_cursor,
            "total":self.incumbents.len()+self.candidates.len(),"failed":self.failed_incumbents+self.failed_candidates,
            "complete":complete,"cancelled":cancelled,"phase":if cancelled{"cancelled"}else if complete{"complete"}else if self.incumbent_cursor<self.incumbents.len(){"incumbents"}else{"candidates"},
            "report":{"club":entity(&self.club),"club_reputation":self.club_reputation,"rows":rows,
                "coverage":{"eligible_candidates":self.eligible_candidates,"selected_candidates":self.candidates.len(),
                    "scanned_candidates":self.candidate_cursor,"failed_candidates":self.failed_candidates,
                    "incumbents":self.incumbents.len(),"scanned_incumbents":self.incumbent_cursor,"failed_incumbents":self.failed_incumbents,
                    "method":"연결된 인덱스의 외부 스태프 중 재직/관련 직무 확인 후보를 CA 순으로 선별한 뒤 실제 직무 점수로 비교. 전체 DB 최고 순위가 아님"},
                "warnings":self.warnings}})
    }
}

impl Bridge {
    pub(super) fn staff_review_command(&mut self, command: &str, a: &Value) -> Result<Value> {
        match command {
            "staff_review_start" => {
                ensure!(
                    self.staff_review_state.queries.len() < 2,
                    "스태프 검토는 동시에 최대 2개입니다. 이전 검토를 중지하세요"
                );
                let club = self.find(EntityKind::Club, required_id(a, "club_id")?)?;
                let limit = a
                    .get("max_candidates")
                    .filter(|v| !v.is_null())
                    .map(|v| v.as_u64().context("후보 수 형식 오류"))
                    .transpose()?
                    .unwrap_or(2000);
                ensure!((1..=20000).contains(&limit), "후보 수는 1–20,000명입니다");
                if self.live {
                    backend::validate_session(&self.index()?.session)?;
                }
                let mut warnings=vec![
                    "읽기 전용 권고입니다. 실제 지원 의사, 계약 해지 보상금, 요구 급료는 확인되지 않았습니다.".into(),
                    "점수는 사용자 계산식 / 300이며 FM 훈련 별점이 아닙니다. 255점 이상은 유지하며, 최소 18점 또는 8% 이상 개선되는 접근 후보가 있어야 교체 검토로 표시합니다.".into(),
                    "명성 비교는 0–10,000 척도입니다. 후보 명성과 소속 구단 명성으로 접근 난도만 추정합니다. 낮은 명성 자체는 해고 이유가 아닙니다.".into(),
                    "현재 훈련 배정·개별 업무 책임은 읽지 않습니다. 일반 코치와 골키퍼 코치는 가장 높은 전문 분야, 연령팀은 해당 유소년 계산식으로 비교합니다.".into(),
                    "동일 대체자가 여러 자리의 후보일 수 있습니다. 여러 직책을 한 사람이 동시에 채운다는 뜻은 아닙니다.".into(),
                ];
                let club_rep = match self.inspect_search_candidate(&club) {
                    Ok(d) => advice::club_reputation(&d),
                    Err(_) => {
                        warnings.push(
                            "구단 상세를 읽지 못했습니다. 명성이 필요한 교체 권고는 보류합니다."
                                .into(),
                        );
                        None
                    }
                };
                let mut seen = HashSet::new();
                let mut incumbents = vec![];
                let mut candidates = vec![];
                for e in self
                    .index()?
                    .entities
                    .iter()
                    .filter(|e| e.kind == EntityKind::Staff)
                {
                    ensure!(
                        seen.insert(e.id),
                        "스태프 ID가 중복되어 있습니다. 다시 연결하세요"
                    );
                    if e.club_id == Some(club.id) {
                        incumbents.push(e.clone());
                    } else if advice::family(e) != advice::Family::Unknown
                        || e.club_id.is_none() && e.ca.is_some_and(|ca| ca > 0)
                    {
                        candidates.push(e.clone());
                    }
                }
                incumbents.sort_by(|a, b| {
                    a.group
                        .cmp(&b.group)
                        .then(a.name.cmp(&b.name))
                        .then(a.id.cmp(&b.id))
                });
                // The cheap index contains CA, not all 27 role scores. Preserve
                // this coverage limitation instead of pretending a global rank.
                candidates.sort_by(|a, b| b.ca.cmp(&a.ca).then(a.id.cmp(&b.id)));
                let eligible_candidates = candidates.len();
                candidates.truncate(limit as usize);
                let candidate_clubs = candidates
                    .iter()
                    .filter_map(|e| e.club_id)
                    .collect::<HashSet<_>>();
                let mut club_entities = HashMap::new();
                for e in self
                    .index()?
                    .entities
                    .iter()
                    .filter(|e| e.kind == EntityKind::Club && candidate_clubs.contains(&e.id))
                {
                    ensure!(
                        club_entities.insert(e.id, e.clone()).is_none(),
                        "구단 ID가 중복되어 있습니다. 다시 연결하세요"
                    );
                }
                if eligible_candidates > candidates.len() {
                    warnings.push(format!("외부 후보 {eligible_candidates}명 중 인덱스 CA 상위 {}명만 상세 확인합니다. 제외된 후보에 더 좋은 직무 전문가가 있을 수 있습니다.",candidates.len()));
                }
                if incumbents.is_empty() {
                    warnings.push("현재 인덱스에서 이 구단 소속 스태프가 없습니다. 다시 연결해 명단을 갱신하세요.".into());
                    candidates.clear();
                }
                self.sequence += 1;
                let id = self.sequence;
                let review = Review {
                    id,
                    club,
                    club_reputation: club_rep,
                    incumbents,
                    candidates,
                    incumbent_cursor: 0,
                    candidate_cursor: 0,
                    failed_incumbents: 0,
                    failed_candidates: 0,
                    eligible_candidates,
                    rows: vec![],
                    club_reputations: HashMap::new(),
                    club_entities,
                    warnings,
                };
                let output = review.response(false);
                if !review.complete() {
                    self.staff_review_state.queries.insert(id, review);
                }
                Ok(output)
            }
            "staff_review_step" => {
                let id = a
                    .get("review_id")
                    .and_then(Value::as_u64)
                    .context("검토 ID가 필요합니다")?;
                let mut review = self
                    .staff_review_state
                    .queries
                    .remove(&id)
                    .context("검토가 완료·취소되었거나 연결이 바뀌었습니다")?;
                // A stale session destroys the cursor before any memory reads.
                if self.live {
                    backend::validate_session(&self.index()?.session)?;
                }
                let start = Instant::now();
                let mut count = 0;
                while !review.complete()
                    && count < 64
                    && start.elapsed() < Duration::from_millis(180)
                {
                    count += 1;
                    if review.incumbent_cursor < review.incumbents.len() {
                        let e = &review.incumbents[review.incumbent_cursor];
                        review.incumbent_cursor += 1;
                        match self.inspect_search_candidate(e) {
                            Ok(d) => review.rows.push(advice::incumbent(e, Some(&d))),
                            Err(_) => {
                                review.failed_incumbents += 1;
                                review.rows.push(advice::incumbent(e, None));
                            }
                        }
                    } else {
                        let e = &review.candidates[review.candidate_cursor];
                        review.candidate_cursor += 1;
                        match self.inspect_search_candidate(e) {
                            Ok(d) => {
                                if !review
                                    .rows
                                    .iter()
                                    .any(|row| advice::candidate_relevant(row, &d))
                                {
                                    continue;
                                }
                                let employer_rep = if let Some(club_id) = e.club_id {
                                    if let Some(rep) = review.club_reputations.get(&club_id) {
                                        *rep
                                    } else {
                                        let rep = review
                                            .club_entities
                                            .get(&club_id)
                                            .and_then(|club| {
                                                self.inspect_search_candidate(club).ok()
                                            })
                                            .as_ref()
                                            .and_then(advice::club_reputation);
                                        review.club_reputations.insert(club_id, rep);
                                        rep
                                    }
                                } else {
                                    None
                                };
                                for row in &mut review.rows {
                                    advice::offer(row, &d, review.club_reputation, employer_rep);
                                }
                            }
                            Err(_) => review.failed_candidates += 1,
                        }
                    }
                }
                let output = review.response(false);
                if !review.complete() {
                    self.staff_review_state.queries.insert(id, review);
                }
                Ok(output)
            }
            "staff_review_cancel" => {
                let id = a
                    .get("review_id")
                    .and_then(Value::as_u64)
                    .context("검토 ID가 필요합니다")?;
                match self.staff_review_state.queries.remove(&id) {
                    Some(review) => Ok(review.response(true)),
                    None => Ok(
                        json!({"review_id":id,"cancelled":true,"complete":false,"stopped":false}),
                    ),
                }
            }
            _ => bail!("지원하지 않는 스태프 검토 명령"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staff_review::tests::{field, staff};
    struct Fixture {
        folder: PathBuf,
        bridge: Bridge,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }
    fn fixture() -> Fixture {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "touchline-staff-review-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&folder).unwrap();
        let mut club = staff(10, None, "구단", "", 15., 7000.);
        club.entity.kind = EntityKind::Club;
        club.entity.name = "테스트 구단".into();
        club.fields = vec![field("club.reputation", 7000.)];
        let profiles = vec![
            club,
            staff(1, Some(10), "수석 코치", "1군", 13., 4500.),
            staff(2, Some(10), "코치", "U-20", 18., 5000.),
            staff(3, None, "수석 코치", "무소속", 18., 5000.),
            staff(4, None, "코치", "무소속", 19., 6000.),
        ];
        let mut entities = vec![];
        for d in profiles {
            std::fs::write(
                folder.join(format!("detail-{:?}-{}.json", d.entity.kind, d.entity.id)),
                serde_json::to_vec_pretty(&d).unwrap(),
            )
            .unwrap();
            entities.push(d.entity);
        }
        let index = Index {
            club_leagues: Vec::new(),
            session: Session {
                pid: 0,
                created: 0,
                module_base: 0,
                module_size: 0,
                module_stamp: 0,
                db: 0,
                registry_begin: 0,
                game_date: "2027-10-29".into(),
                profile: "synthetic-staff-review-fixture".into(),
                own_club_id: Some(10),
            },
            entities,
            warnings: vec!["Synthetic test fixture, not FM data".into()],
        };
        std::fs::write(
            folder.join("index.json"),
            serde_json::to_vec_pretty(&index).unwrap(),
        )
        .unwrap();
        Fixture {
            folder: folder.clone(),
            bridge: Bridge {
                index: Some(index),
                snapshot: Some(folder.join("index.json")),
                ..Bridge::default()
            },
        }
    }
    #[test]
    fn snapshot_scan_scores_candidates_and_never_exposes_memory_bindings() {
        let mut f = fixture();
        let first = f
            .bridge
            .staff_review_command("staff_review_start", &json!({"club_id":10}))
            .unwrap();
        let id = first["review_id"].as_u64().unwrap();
        let mut result = first;
        while result["complete"] != true {
            result = f
                .bridge
                .staff_review_command("staff_review_step", &json!({"review_id":id}))
                .unwrap();
        }
        assert_eq!(result["scanned"], 4);
        let rows = result["report"]["rows"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|r| r["person"]["id"] == 1).unwrap()["status"],
            "replace_review"
        );
        assert_eq!(
            rows.iter().find(|r| r["person"]["id"] == 2).unwrap()["status"],
            "retain"
        );
        let public = result.to_string();
        for token in ["address", "binding", "guards", "original"] {
            assert!(!public.contains(token));
        }
        assert!(
            f.bridge
                .staff_review_command("staff_review_step", &json!({"review_id":id}))
                .is_err()
        );
    }
    #[test]
    fn cancellation_and_connection_refresh_destroy_cursors() {
        let mut f = fixture();
        let first = f
            .bridge
            .staff_review_command("staff_review_start", &json!({"club_id":10}))
            .unwrap();
        let id = first["review_id"].as_u64().unwrap();
        let cancel = f
            .bridge
            .staff_review_command("staff_review_cancel", &json!({"review_id":id}))
            .unwrap();
        assert_eq!(cancel["cancelled"], true);
        assert_eq!(cancel["complete"], false);
        assert!(
            f.bridge
                .staff_review_command("staff_review_step", &json!({"review_id":id}))
                .is_err()
        );
        let first = f
            .bridge
            .staff_review_command("staff_review_start", &json!({"club_id":10}))
            .unwrap();
        f.bridge.invalidate();
        assert!(
            f.bridge
                .staff_review_command(
                    "staff_review_step",
                    &json!({"review_id":first["review_id"]})
                )
                .is_err()
        );
    }
    #[test]
    fn scan_cap_and_failed_snapshots_are_reported_not_assumed_zero_quality() {
        let mut f = fixture();
        std::fs::remove_file(f.folder.join("detail-Staff-1.json")).unwrap();
        let first = f
            .bridge
            .staff_review_command(
                "staff_review_start",
                &json!({"club_id":10,"max_candidates":1}),
            )
            .unwrap();
        let id = first["review_id"].as_u64().unwrap();
        let result = f
            .bridge
            .staff_review_command("staff_review_step", &json!({"review_id":id}))
            .unwrap();
        assert_eq!(result["report"]["coverage"]["selected_candidates"], 1);
        assert_eq!(result["report"]["coverage"]["eligible_candidates"], 2);
        assert_eq!(result["failed"], 1);
        assert_eq!(
            result["report"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["person"]["id"] == 1)
                .unwrap()["status"],
            "insufficient"
        );
    }
    #[test]
    fn total_candidate_read_failure_is_not_a_positive_retention_recommendation() {
        let mut f = fixture();
        for id in [3, 4] {
            std::fs::remove_file(f.folder.join(format!("detail-Staff-{id}.json"))).unwrap();
        }
        let start = f
            .bridge
            .staff_review_command("staff_review_start", &json!({"club_id":10}))
            .unwrap();
        let result = f
            .bridge
            .staff_review_command(
                "staff_review_step",
                &json!({"review_id":start["review_id"]}),
            )
            .unwrap();
        let rows = result["report"]["rows"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|row| row["person"]["id"] == 1).unwrap()["status"],
            "insufficient"
        );
        assert_eq!(
            rows.iter().find(|row| row["person"]["id"] == 2).unwrap()["status"],
            "retain"
        );
    }
    #[test]
    #[ignore = "writes only a synthetic QA fixture; run explicitly"]
    fn export_staff_review_fixture() {
        let f = fixture();
        let output = PathBuf::from("output/staff-review-fixture-v09");
        std::fs::create_dir_all(&output).unwrap();
        for entry in std::fs::read_dir(&f.folder).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), output.join(entry.file_name())).unwrap();
        }
        std::fs::write(output.join("README.txt"),"Synthetic staffing review fixture. No live FM values or memory bindings. Club 10; staff 1 should be replacement-review; staff 2 retained. All hiring availability is a heuristic, not confirmed interest.\n").unwrap();
    }
}
