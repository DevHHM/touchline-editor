//! Connection recovery uses a small identity probe; rebuilding the full index
//! is a separate, serial command after the game has remained stable.
use super::*;
use crate::backend::watch::{self, Observation, Ready};

const STABLE_FOR: Duration = Duration::from_millis(1500);
const RECONNECT_BACKOFF: Duration = Duration::from_secs(8);

#[derive(Default)]
pub(super) struct Monitor {
    pub enabled: bool,
    attached: Option<Ready>,
    candidate: Option<(Ready, Instant)>,
    preferred_pid: Option<u32>,
    state: &'static str,
    reason: String,
    last_attempt: Option<Instant>,
}

pub(super) fn is_mutation(command: &str) -> bool {
    matches!(command, "apply" | "undo" | "relation_apply" | "relation_undo"
        | "event_apply" | "event_undo" | "batch_apply" | "batch_undo"
        | "freeze_enable" | "freeze_tick" | "name_localization_apply" | "name_localization_undo")
}

impl Bridge {
    fn connection_result(&self, changed: bool, date_changed: bool, now: Instant) -> Value {
        let can_reconnect = self.connection.enabled && !self.live
            && self.connection.candidate.as_ref().is_some_and(|(_, since)| now.duration_since(*since) >= STABLE_FOR)
            && self.connection.last_attempt.is_none_or(|last| now.duration_since(last) >= RECONNECT_BACKOFF);
        crate::runtime_log::connection(if can_reconnect {"ready"}else{self.connection.state},&self.connection.reason);
        json!({"state":if can_reconnect {"ready"} else {self.connection.state},
            "monitoring":self.connection.enabled,"changed":changed,"date_changed":date_changed,
            "can_reconnect":can_reconnect,"generation":self.connection_generation,
            "session_generation":self.connection_generation,"data_revision":self.epoch,"reason":self.connection.reason,
            "game_date":self.index.as_ref().map(|i|i.session.game_date.as_str()),"summary":self.summary()})
    }
    fn connection_observe(&mut self, observation: Observation, now: Instant) -> Value {
        if !self.connection.enabled {
            self.connection.state = if self.snapshot.is_some() {"snapshot"} else {"disabled"};
            return self.connection_result(false, false, now);
        }
        let mut changed = false;
        let mut date_changed = false;
        if let Observation::Ready(ref ready) = observation {
            if self.live && self.connection.attached.as_ref().is_some_and(|old|
                old.identity == ready.identity && ready.game_date >= old.game_date) {
                date_changed = self.connection.attached.as_ref().is_some_and(|old|old.game_date != ready.game_date);
                if let Some(index) = &mut self.index { index.session.game_date = ready.game_date.clone(); }
                self.connection.attached = Some(ready.clone());
                self.connection.state = "connected";
                self.connection.reason.clear();
                self.connection.candidate = None;
                return self.connection_result(false, date_changed, now);
            }
        }
        if self.live || self.index.is_some() {
            // Discard old addresses, drafts, query cursors and freeze writers
            // before telling the UI that the save is changing.
            self.replace_connection();
            self.connection.attached = None;
            changed = true;
        }
        match observation {
            Observation::Ready(ready) => {
                let preserve_error = self.connection.state == "error"
                    && self.connection.candidate.as_ref().is_some_and(|(old, _)| old.identity == ready.identity);
                self.connection.preferred_pid = Some(ready.identity.pid);
                if !self.connection.candidate.as_ref().is_some_and(|(old, _)| old == &ready) {
                    self.connection.candidate = Some((ready, now));
                }
                // A successful small probe does not erase a failed full index
                // read. Keep its diagnostic visible throughout retry backoff.
                if !preserve_error {
                    self.connection.state = "loading";
                    self.connection.reason = "새 세이브 데이터가 안정되면 자동으로 다시 불러옵니다".into();
                }
            }
            Observation::Waiting(reason) => {
                self.connection.candidate = None;
                self.connection.state = "loading";
                self.connection.reason = reason;
            }
            Observation::Error(reason) => {
                self.connection.candidate = None;
                self.connection.state = "error";
                self.connection.reason = reason;
            }
            Observation::UnsupportedBuild(reason) => {
                self.connection.candidate = None;
                self.connection.state = "unsupported_build";
                self.connection.reason = reason;
            }
            Observation::Absent => {
                self.connection.candidate = None;
                self.connection.preferred_pid = None;
                self.connection.state = "waiting";
                self.connection.reason = crate::backend::discovery::LOCAL_GAME_GUIDANCE.into();
            }
        }
        self.connection_result(changed, date_changed, now)
    }
    pub(super) fn connection_poll(&mut self) -> Value {
        if !self.connection.enabled { return self.connection_observe(Observation::Absent, Instant::now()); }
        self.connection_observe(watch::probe(self.connection.preferred_pid), Instant::now())
    }
    pub(super) fn connection_preflight(&mut self) -> Result<()> {
        let status = self.connection_poll();
        ensure!(status["state"] == "connected" && self.live,
            "세이브가 변경되었거나 로드 중입니다. 이전 변경 검토는 폐기했습니다. 자동 재연결 후 다시 검토하세요");
        Ok(())
    }
    fn attach_observed(&mut self, index: Index, ready: Ready) -> Result<()> {
        ensure!(ready.identity.matches_session(&index.session), "연결 중 FM 세션이 변경됐습니다");
        self.invalidate();
        let mut index = index;
        index.session.game_date = ready.game_date.clone();
        self.index = Some(index);
        self.snapshot = None;
        self.live = true;
        self.connection.enabled = true;
        self.connection.preferred_pid = Some(ready.identity.pid);
        self.connection.attached = Some(ready);
        self.connection.candidate = None;
        self.connection.state = "connected";
        self.connection.reason.clear();
        self.connection.last_attempt = None;
        Ok(())
    }
    fn initial_observation(&mut self, observation: Observation, now: Instant) -> Result<Ready> {
        if let Observation::Ready(ready) = observation { return Ok(ready); }
        self.connection_observe(observation, now);
        // Permission errors, unknown builds and an unloaded save need different
        // actions. Return the actual probe reason, including its source chain.
        bail!("{}", self.connection.reason);
    }
    fn rebuild_observed_with(
        &mut self, before: &Ready, now: Instant,
        load: impl FnOnce(u32) -> Result<Index>,
        observe: impl FnOnce(u32) -> Observation,
    ) -> Result<Value> {
        crate::runtime_log::event("info","connection","attempt","Reading the observed FM process");
        self.connection.last_attempt = Some(now);
        let pid = before.identity.pid;
        let outcome = load(pid);
        self.complete_connection(before, outcome, observe(pid))
    }
    pub(super) fn connection_connect(&mut self) -> Result<Value> {
        crate::runtime_log::event("info","connection","requested","FM connection requested");
        self.replace_connection();
        self.connection = Monitor { enabled: true, ..Monitor::default() };
        let before = self.initial_observation(watch::probe(None), Instant::now())?;
        self.connection.preferred_pid = Some(before.identity.pid);
        self.rebuild_observed_with(&before, Instant::now(),
            |pid| backend::connect_pid(pid, &|s|eprintln!("{s}")),
            |pid| watch::probe(Some(pid)))?;
        ensure!(self.live, "{}", self.connection.reason);
        Ok(self.summary())
    }
    pub(super) fn connection_reconnect(&mut self) -> Result<Value> {
        ensure!(self.connection.enabled, "자동 연결 감시가 켜져 있지 않습니다");
        let observed = self.connection_poll();
        if !observed["can_reconnect"].as_bool().unwrap_or(false) { return Ok(observed); }
        let before = self.connection.candidate.as_ref().unwrap().0.clone();
        self.rebuild_observed_with(&before, Instant::now(),
            |pid| backend::connect_pid(pid, &|s|eprintln!("{s}")),
            |pid| watch::probe(Some(pid)))
    }
    fn complete_connection(&mut self, before: &Ready, outcome: Result<Index>, after: Observation) -> Result<Value> {
        match (outcome, after) {
            (Ok(index), Observation::Ready(ready)) if ready.identity == before.identity && ready.game_date >= before.game_date && ready.identity.matches_session(&index.session) => {
                self.attach_observed(index, ready)?;
                let mut result = self.connection_result(true, false, Instant::now());
                result["state"] = json!("reconnected");
                Ok(result)
            }
            (Err(error), Observation::Ready(ready)) if ready.identity == before.identity => {
                crate::runtime_log::error("connection","index_failed","FM index read failed",&error);
                self.connection.state = "error";
                self.connection.reason = format!("FM 데이터 조회 실패: {error:#}");
                self.connection.candidate = Some((ready, Instant::now()));
                Err(error)
            }
            (_, observation) => {
                // The post-scan probe can expose an access error or unsupported
                // build; do not replace that reason with a generic load message.
                Ok(self.connection_observe(observation, Instant::now()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::watch::Identity;

    fn fixture() -> (Bridge, Ready) {
        let (index, details, _) = crate::relations::fixtures::data();
        let session = &index.session;
        let ready = Ready { identity: Identity { pid: session.pid, created: session.created,
            module_base: session.module_base, module_size: session.module_size,
            module_stamp: session.module_stamp, db: session.db, registry_begin: session.registry_begin,
            registry_end: session.registry_begin + 800, anchors: vec![(4, 0x10000, 0x1000, 7)] },
            game_date: session.game_date.clone() };
        let mut bridge = Bridge { index: Some(index), ..Bridge::default() };
        for detail in details { bridge.details.insert((detail.entity.kind, detail.entity.id), detail); }
        (bridge, ready)
    }
    fn activate(bridge: &mut Bridge, ready: &Ready) {
        bridge.live = true;
        bridge.connection.enabled = true;
        bridge.connection.attached = Some(ready.clone());
        bridge.connection.preferred_pid = Some(ready.identity.pid);
    }
    #[test]
    fn load_process_registry_or_anchor_change_discards_old_tokens_and_queries() {
        for mutation in 0..6 {
            let (mut bridge, ready) = fixture();
            let query = bridge.dispatch("query_start", &json!({"kind":"Player"})).unwrap();
            let detail = bridge.details[&(EntityKind::Player, 7)].clone();
            let token = bridge.token(Preview::Edit { detail, edits: vec![] });
            activate(&mut bridge, &ready);
            let epoch = bridge.epoch;
            let generation = bridge.connection_generation;
            let mut changed = ready;
            match mutation {
                0 => changed.identity.pid += 1,
                1 => changed.identity.created += 1,
                2 => changed.identity.db += 8,
                3 => changed.identity.registry_begin += 8,
                4 => changed.identity.anchors[0].1 += 64,
                _ => changed.game_date = "2000-01-01".into(),
            }
            let result = bridge.connection_observe(Observation::Ready(changed), Instant::now());
            assert_eq!(result["changed"], true);
            assert_eq!(result["can_reconnect"], false);
            assert!(!bridge.live && bridge.index.is_none() && bridge.details.is_empty());
            assert_eq!(bridge.epoch, epoch + 1);
            assert_eq!(result["generation"], generation + 1);
            assert_eq!(result["session_generation"], generation + 1);
            assert_eq!(result["data_revision"], epoch + 1);
            assert!(bridge.consume(&token).is_err());
            assert!(bridge.workspace_command("query_step", &json!({"query_id":query["query_id"]})).is_err());
        }
    }
    #[test]
    fn edits_invalidate_previews_without_changing_the_connected_save_generation() {
        let (mut bridge, ready) = fixture();
        let detail = bridge.details[&(EntityKind::Player, 7)].clone();
        activate(&mut bridge, &ready);
        let start = Instant::now();
        let initial = bridge.connection_observe(Observation::Ready(ready.clone()), start);
        let generation = initial["generation"].as_u64().unwrap();
        let revision = bridge.epoch;
        // All successful edit/undo paths and ordinary refresh use invalidate().
        // Repeat it to cover the next monitor tick after successive edits.
        for step in 1..=3 {
            let token = bridge.token(Preview::Edit { detail: detail.clone(), edits: vec![] });
            bridge.invalidate();
            let observed = bridge.connection_observe(Observation::Ready(ready.clone()), start + Duration::from_secs(step));
            assert_eq!(observed["state"], "connected");
            assert_eq!(observed["changed"], false);
            assert_eq!(observed["can_reconnect"], false);
            assert_eq!(observed["generation"], generation);
            assert_eq!(observed["session_generation"], generation);
            assert_eq!(observed["data_revision"], revision + step);
            assert!(bridge.live && bridge.index.is_some());
            assert!(bridge.consume(&token).is_err());
        }
    }
    #[test]
    fn transient_loading_resets_stability_and_reconnect_is_not_a_write() {
        let (mut bridge, ready) = fixture();
        activate(&mut bridge, &ready);
        let start = Instant::now();
        assert_eq!(bridge.connection_observe(Observation::Absent, start)["state"], "waiting");
        assert!(bridge.require_live().is_err());
        assert_eq!(bridge.connection_observe(Observation::Ready(ready.clone()), start)["can_reconnect"], false);
        assert_eq!(bridge.connection_observe(Observation::Ready(ready.clone()), start + Duration::from_secs(2))["can_reconnect"], true);
        bridge.connection_observe(Observation::Waiting("loading".into()), start + Duration::from_secs(3));
        assert_eq!(bridge.connection_observe(Observation::Ready(ready.clone()), start + Duration::from_secs(4))["can_reconnect"], false);
        assert_eq!(bridge.connection_observe(Observation::Ready(ready), start + Duration::from_secs(6))["state"], "ready");
        assert!(bridge.require_live().is_err());
        assert!(!is_mutation("connection_poll") && !is_mutation("connection_reconnect"));
    }
    #[test]
    fn normal_date_progress_keeps_previews_and_snapshot_mode_never_autoconnects() {
        let (mut bridge, mut ready) = fixture();
        let token = bridge.token(Preview::Edit { detail: bridge.details[&(EntityKind::Player, 7)].clone(), edits: vec![] });
        activate(&mut bridge, &ready);
        ready.game_date = "2099-12-31".into();
        let result = bridge.connection_observe(Observation::Ready(ready.clone()), Instant::now());
        assert_eq!(result["state"], "connected");
        assert_eq!(result["date_changed"], true);
        assert_eq!(result["changed"], false);
        assert!(bridge.consume(&token).is_ok());
        bridge.connection = Monitor::default();
        bridge.live = false;
        bridge.snapshot = Some(PathBuf::from("synthetic.json"));
        let result = bridge.connection_observe(Observation::Ready(ready), Instant::now());
        assert_eq!(result["state"], "snapshot");
        assert_eq!(result["monitoring"], false);
        assert_eq!(result["can_reconnect"], false);
        assert!(bridge.index.is_some());
    }
    #[test]
    fn attach_rejects_wrong_game_and_mutation_preflight_covers_all_writers() {
        let (mut bridge, mut ready) = fixture();
        let index = bridge.index.take().unwrap();
        ready.identity.created += 1;
        assert!(bridge.attach_observed(index, ready).is_err());
        assert!(!bridge.live && bridge.index.is_none());
        for command in ["apply", "undo", "relation_apply", "relation_undo", "event_apply", "event_undo",
            "batch_apply", "batch_undo", "freeze_enable", "freeze_tick", "name_localization_apply", "name_localization_undo"] {
            assert!(is_mutation(command), "{command}");
        }
    }
    #[test]
    fn reload_during_index_scan_never_publishes_the_stale_index() {
        let (mut bridge, ready) = fixture();
        activate(&mut bridge, &ready);
        let old_index = bridge.index.as_ref().unwrap().clone();
        bridge.connection_observe(Observation::Absent, Instant::now());
        let mut replaced = ready.clone();
        replaced.identity.anchors[0].1 += 8;
        let result = bridge.complete_connection(&ready, Ok(old_index.clone()), Observation::Ready(replaced)).unwrap();
        assert_ne!(result["state"], "reconnected");
        assert!(bridge.index.is_none() && !bridge.live);
        let result = bridge.complete_connection(&ready, Ok(old_index), Observation::Ready(ready.clone())).unwrap();
        assert_eq!(result["state"], "reconnected");
        assert!(bridge.index.is_some() && bridge.live && bridge.pending.is_empty());
    }
    #[test]
    fn unsupported_build_is_not_reported_as_save_loading_or_retried() {
        let (mut bridge, ready) = fixture();
        activate(&mut bridge, &ready);
        let reason = "FM27 unknown native build".to_owned();
        let result = bridge.connection_observe(Observation::UnsupportedBuild(reason.clone()), Instant::now());
        assert_eq!(result["state"], "unsupported_build");
        assert_eq!(result["reason"], reason);
        assert_eq!(result["can_reconnect"], false);
        assert!(bridge.index.is_none() && !bridge.live);
    }
    #[test]
    fn stable_save_read_error_preserves_actual_failure_reason() {
        let (mut bridge, ready) = fixture();
        activate(&mut bridge, &ready);
        bridge.replace_connection();
        let result = bridge.complete_connection(&ready, Err(anyhow::anyhow!("registry validation failed")), Observation::Ready(ready.clone()));
        assert!(result.is_err());
        assert_eq!(bridge.connection.state, "error");
        assert!(bridge.connection.reason.contains("registry validation failed"));
        assert!(!bridge.live && bridge.index.is_none());
    }
    #[test]
    fn first_connection_retains_actual_wait_and_failure_messages() {
        for (observation, state, reason) in [
            (Observation::Waiting("세이브 인물 목록이 비어 있습니다".into()), "loading", "세이브 인물 목록이 비어 있습니다"),
            (Observation::Error("FM 프로세스 접근 실패: os error 5".into()), "error", "FM 프로세스 접근 실패: os error 5"),
            (Observation::UnsupportedBuild("지원하지 않는 빌드 ABCD".into()), "unsupported_build", "지원하지 않는 빌드 ABCD"),
            (Observation::Absent, "waiting", crate::backend::discovery::LOCAL_GAME_GUIDANCE),
        ] {
            let mut bridge = Bridge::default();
            bridge.connection.enabled = true;
            let error = bridge.initial_observation(observation, Instant::now()).unwrap_err();
            assert_eq!(error.to_string(), reason);
            assert_eq!(bridge.connection.state, state);
            assert!(!bridge.live && bridge.connection.candidate.is_none());
        }
    }
    #[test]
    fn rebuild_reads_only_the_observed_pid_and_retains_failure_during_backoff() {
        let (mut bridge, ready) = fixture();
        activate(&mut bridge, &ready);
        bridge.replace_connection();
        let start = Instant::now();
        let outcome = bridge.rebuild_observed_with(&ready, start,
            |pid| { assert_eq!(pid, ready.identity.pid); Err(anyhow::anyhow!("city validation failed").context("index read")) },
            |pid| { assert_eq!(pid, ready.identity.pid); Observation::Ready(ready.clone()) });
        assert!(outcome.is_err());
        let status = bridge.connection_observe(Observation::Ready(ready.clone()), start + Duration::from_secs(2));
        assert_eq!(status["state"], "error");
        assert_eq!(status["can_reconnect"], false);
        assert!(status["reason"].as_str().unwrap().contains("index read: city validation failed"));
        let status = bridge.connection_observe(Observation::Ready(ready.clone()), start + Duration::from_secs(9));
        assert_eq!(status["state"], "ready");
        assert_eq!(status["can_reconnect"], true);
        let mut different = ready.clone(); different.identity.db += 8;
        let status = bridge.connection_observe(Observation::Ready(different), start + Duration::from_secs(10));
        assert_eq!(status["state"], "loading");
        assert!(!status["reason"].as_str().unwrap().contains("city validation failed"));
    }
    #[test]
    fn post_scan_error_is_not_reported_as_a_changing_save() {
        let (mut bridge, ready) = fixture();
        let index = bridge.index.clone().unwrap();
        activate(&mut bridge, &ready);
        bridge.replace_connection();
        let result = bridge.complete_connection(&ready, Ok(index), Observation::Error("access denied after scan".into())).unwrap();
        assert_eq!(result["state"], "error");
        assert_eq!(result["reason"], "access denied after scan");
        assert!(!bridge.live && bridge.index.is_none());
    }
}
