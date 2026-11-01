//! Explicitly opted-in local prototype. This module is absent from normal builds.
use super::*;
use std::io::Read;
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub(super) struct SaveBackup {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    fingerprint: u64,
}

fn fingerprint(path: &Path) -> Result<u64> {
    let mut file = std::fs::File::open(path).context("시험 세이브 백업을 열지 못했습니다")?;
    let mut hash = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        for byte in &buffer[..count] { hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3); }
    }
    Ok(hash)
}

impl SaveBackup {
    fn capture(path: &Path) -> Result<Self> {
        let path = std::fs::canonicalize(path).context("시험 세이브 백업을 찾지 못했습니다")?;
        ensure!(path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("fm")), "FM 세이브 백업 파일이 필요합니다");
        let metadata = std::fs::metadata(&path)?;
        ensure!(metadata.is_file() && (1_000_000..=2_147_483_648).contains(&metadata.len()), "시험 세이브 백업 크기를 확인할 수 없습니다");
        let modified = metadata.modified()?;
        let proof = Self { bytes: metadata.len(), modified, fingerprint: fingerprint(&path)?, path };
        proof.verify()?;
        Ok(proof)
    }

    fn verify(&self) -> Result<()> {
        let metadata = std::fs::metadata(&self.path).context("시험 세이브 백업을 다시 확인하지 못했습니다")?;
        ensure!(metadata.is_file() && metadata.len() == self.bytes && metadata.modified()? == self.modified,
            "검토 후 시험 세이브 백업이 변경됐습니다");
        ensure!(fingerprint(&self.path)? == self.fingerprint, "검토 후 시험 세이브 백업 내용이 변경됐습니다");
        Ok(())
    }
}

fn require_opt_in() -> Result<()> {
    ensure!(std::env::var_os("TOUCHLINE_FM27_HEALTH_EXPERIMENT").as_deref() == Some(std::ffi::OsStr::new("1")),
        "부상 제거 시험 엔진은 명시적인 로컬 실험 설정이 필요합니다");
    Ok(())
}

impl Bridge {
    pub(super) fn health_experiment_command(&mut self, command: &str, args: &Value) -> Result<Value> {
        require_opt_in()?;
        self.require_live()?;
        match command {
            #[cfg(feature = "fm27-transfer-experiment")]
            "transfer_test_preview" => {
                let player = self.find(EntityKind::Player, required_id(args, "player_id")?)?;
                let destination = self.find(EntityKind::Club, required_id(args, "destination_club_id")?)?;
                let plan = backend::transfer_experiment::preview(self.require_live()?, &player, &destination)?;
                let mut summary = serde_json::to_value(plan.summary())?;
                let eligible = summary["can_apply"].as_bool() == Some(true);
                let backup = args.get("backup_path").filter(|v| !v.is_null())
                    .map(|_| SaveBackup::capture(Path::new(required_str(args, "backup_path")?))).transpose()?;
                summary["can_apply"] = json!(eligible && backup.is_some());
                summary["save_backup_retained"] = json!(backup.is_some());
                summary["experimental"] = json!(true);
                summary["scalar_undo_available"] = json!(false);
                if eligible { summary["token"] = json!(self.token(Preview::TransferExperiment { plan, backup })); }
                Ok(summary)
            }
            #[cfg(feature = "fm27-transfer-experiment")]
            "transfer_test_apply" => {
                let Preview::TransferExperiment { plan, backup } = self.consume(required_str(args, "token")?)? else {
                    bail!("강제이적 시험 검토가 아닙니다");
                };
                let backup = backup.context("세이브 백업을 포함해 다시 검토하세요")?;
                backup.verify()?;
                let outcome = backend::transfer_experiment::apply(&plan);
                self.invalidate();
                Ok(serde_json::to_value(outcome?)?)
            }
            "ban_test_preview" => {
                let id = required_id(args, "id")?;
                let entity = self.find(EntityKind::Player, id)?;
                let plan = backend::bans::preview(self.require_live()?, &entity)?;
                let mut summary = serde_json::to_value(plan.summary())?;
                let backup = args.get("backup_path").filter(|v| !v.is_null())
                    .map(|_| SaveBackup::capture(Path::new(required_str(args, "backup_path")?))).transpose()?;
                let has_bans = summary["ban_count"].as_u64().is_some_and(|count| count > 0);
                summary["can_apply"] = json!(has_bans && backup.is_some());
                summary["save_backup_retained"] = json!(backup.is_some());
                summary["experimental"] = json!(true);
                summary["scalar_undo_available"] = json!(false);
                if has_bans { summary["token"] = json!(self.token(Preview::BanExperiment { plan, backup })); }
                Ok(summary)
            }
            "ban_test_apply" => {
                let Preview::BanExperiment { plan, backup } = self.consume(required_str(args, "token")?)? else {
                    bail!("징계 해제 시험 검토가 아닙니다");
                };
                let backup = backup.context("세이브 백업을 포함해 다시 검토하세요")?;
                backup.verify()?;
                let outcome = backend::bans::apply(&plan);
                self.invalidate();
                Ok(serde_json::to_value(outcome?)?)
            }
            "injury_test_preview" => {
                let id = required_id(args, "id")?;
                let entity = self.find(EntityKind::Player, id)?;
                let plan = backend::health::preview(self.require_live()?, &entity)?;
                let mut summary = serde_json::to_value(plan.summary())?;
                let backup = args.get("backup_path").filter(|v| !v.is_null())
                    .map(|_| SaveBackup::capture(Path::new(required_str(args, "backup_path")?))).transpose()?;
                let has_injuries = summary["injury_count"].as_u64().is_some_and(|count| count > 0);
                summary["can_apply"] = json!(has_injuries && backup.is_some());
                summary["save_backup_retained"] = json!(backup.is_some());
                summary["experimental"] = json!(true);
                summary["scalar_undo_available"] = json!(false);
                if has_injuries {
                    summary["token"] = json!(self.token(Preview::InjuryExperiment { plan, backup }));
                }
                Ok(summary)
            }
            "injury_test_apply" => {
                let Preview::InjuryExperiment { plan, backup } = self.consume(required_str(args, "token")?)? else {
                    bail!("부상 제거 시험 검토가 아닙니다");
                };
                let backup = backup.context("현재 상태의 시험 세이브 백업을 포함해 다시 검토하세요")?;
                backup.verify()?;
                // Native cleanup destroys records. Even a failed or uncertain
                // attempt cannot leave other retained scalar/health tokens live.
                let outcome = backend::health::apply(&plan);
                self.invalidate();
                Ok(serde_json::to_value(outcome?)?)
            }
            _ => bail!("알 수 없는 부상 제거 시험 명령"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_backup_content_change_is_rejected_even_with_same_size_and_timestamp() {
        let dir = std::env::temp_dir().join(format!("touchline-injury-backup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.fm");
        std::fs::write(&path, vec![1u8; 1_000_000]).unwrap();
        let backup = SaveBackup::capture(&path).unwrap();
        assert!(backup.verify().is_ok());
        std::fs::write(&path, vec![2u8; 1_000_000]).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(backup.modified)).unwrap();
        assert!(backup.verify().unwrap_err().to_string().contains("내용"));
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn missing_or_non_save_backup_rejects_before_native_work() {
        assert!(SaveBackup::capture(Path::new("nonexistent-touchline-save.fm")).is_err());
        assert!(connection::is_mutation("injury_test_apply"));
        assert!(!connection::is_mutation("injury_test_preview"));
        assert!(connection::is_mutation("ban_test_apply"));
        assert!(!connection::is_mutation("ban_test_preview"));
        assert_eq!(connection::is_mutation("transfer_test_apply"), cfg!(feature = "fm27-transfer-experiment"));
        assert!(!connection::is_mutation("transfer_test_preview"));
    }
}
