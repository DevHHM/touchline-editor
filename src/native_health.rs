//! Native FM27 injury experiment. No arbitrary-function API and no RAX contract.
//! Completion confirms only return from the native call; backend health code
//! performs independent identity/list/post-state validation afterward.
use crate::{backend, memory::Process, model::Session, transaction};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::c_void,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    System::{
        Diagnostics::Debug::{FlushInstructionCache, WriteProcessMemory},
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_READWRITE,
            VirtualAllocEx, VirtualFreeEx, VirtualProtectEx,
        },
        Threading::{
            CreateRemoteThread, GetExitCodeThread, OpenProcess, PROCESS_CREATE_THREAD,
            PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
            WaitForSingleObject,
        },
    },
};

const HELPER_RVA: u64 = 0x29C87C0;
pub(crate) const COMPLETE: u64 = 0x544C4845414C0002;
#[derive(Serialize, Deserialize)]
struct Journal {
    id: String,
    session: Session,
    player_id: u32,
    player_base: u64,
    state: String,
    before: serde_json::Value,
    worker_code: u64,
    worker_result: u64,
    note: String,
    #[serde(default)]
    operation: String,
}
fn folder() -> Result<PathBuf> {
    let p = transaction::data_dir().join("health-experiments");
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
fn path(id: &str) -> Result<PathBuf> {
    ensure!(
        !id.is_empty() && id.len() < 100 && id.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "건강 시험 기록 ID 오류"
    );
    Ok(folder()?.join(format!("{id}.json")))
}
fn persist(j: &Journal) -> Result<()> {
    transaction::persist_json(&path(&j.id)?, j)
}
fn block_pending(s: &Session) -> Result<()> {
    for item in std::fs::read_dir(folder()?)? {
        let item = item?;
        if item.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let bytes = std::fs::read(item.path())?;
        let row: Journal = serde_json::from_slice(&bytes)
            .context("건강 시험 기록이 손상되어 중복 실행을 차단했습니다")?;
        if row.session.pid == s.pid
            && row.session.created == s.created
            && !matches!(row.state.as_str(), "verified" | "not_started")
        {
            bail!(
                "완료 여부가 검증되지 않은 건강 시험 기록이 있습니다 ({}). 자동·반복 실행을 차단했습니다",
                row.id
            );
        }
    }
    Ok(())
}
pub(crate) fn verified(id: &str) -> Result<()> {
    update(id, "verified", "Independent health post-state verified")
}
pub(crate) fn verification_failed(id: &str, note: &str) -> Result<()> {
    update(id, "verification_failed", note)
}
fn update(id: &str, state: &str, note: &str) -> Result<()> {
    let mut j: Journal = serde_json::from_slice(&std::fs::read(path(id)?)?)?;
    j.state = state.into();
    j.note = note.into();
    persist(&j)
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Pages {
    process: HANDLE,
    code: u64,
    result: u64,
    safe: bool,
}
impl Drop for Pages {
    fn drop(&mut self) {
        if self.safe {
            unsafe {
                if self.code != 0 {
                    VirtualFreeEx(self.process, self.code as *mut c_void, 0, MEM_RELEASE);
                }
                if self.result != 0 {
                    VirtualFreeEx(self.process, self.result as *mut c_void, 0, MEM_RELEASE);
                }
            }
        }
    }
}
fn stub(function: u64, player: u64, result: u64) -> Vec<u8> {
    // Entry RSP is 8 mod 16. 0x28 supplies shadow space and call alignment.
    let mut b = vec![0x48, 0x83, 0xEC, 0x28, 0x48, 0xB9];
    b.extend(player.to_le_bytes());
    b.extend([0x48, 0xB8]);
    b.extend(function.to_le_bytes());
    b.extend([0xFF, 0xD0]);
    // Only a reached post-call marker counts; the helper's RAX is ignored.
    b.extend([0x48, 0xBA]);
    b.extend(result.to_le_bytes());
    b.extend([0x48, 0xB8]);
    b.extend(COMPLETE.to_le_bytes());
    b.extend([0x48, 0x89, 0x02, 0x48, 0x83, 0xC4, 0x28, 0x31, 0xC0, 0xC3]);
    b
}
fn write(h: HANDLE, at: u64, b: &[u8]) -> Result<()> {
    let mut n = 0;
    ensure!(
        unsafe { WriteProcessMemory(h, at as *mut c_void, b.as_ptr().cast(), b.len(), &mut n) }
            != 0
            && n == b.len(),
        "건강 시험 작업자 준비 실패: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
pub(crate) fn remove_injuries(
    s: &Session,
    player_id: u32,
    player_base: u64,
    before: &serde_json::Value,
    preflight: impl FnOnce() -> Result<()>,
) -> Result<String> {
    run_experiment(
        s,
        player_id,
        player_base,
        before,
        "injuries",
        |result| stub(s.module_base + HELPER_RVA, player_base, result),
        preflight,
    )
}
pub(crate) fn remove_bans(
    s: &Session,
    player_id: u32,
    player_base: u64,
    before: &serde_json::Value,
    preflight: impl FnOnce() -> Result<()>,
) -> Result<String> {
    run_experiment(
        s,
        player_id,
        player_base,
        before,
        "bans",
        |result| crate::native_bans::stub(s.module_base, player_base, result),
        preflight,
    )
}
pub(crate) fn run_experiment(
    s: &Session,
    player_id: u32,
    player_base: u64,
    before: &serde_json::Value,
    operation: &'static str,
    build_stub: impl FnOnce(u64) -> Vec<u8>,
    preflight: impl FnOnce() -> Result<()>,
) -> Result<String> {
    ensure!(
        matches!(operation, "injuries" | "bans" | "transfer"),
        "지원되지 않는 네이티브 시험 작업입니다"
    );
    ensure!(
        s.module_stamp == backend::profiles::FM27.timestamp
            && s.module_size == backend::profiles::FM27.image_size
            && s.profile == backend::profiles::FM27.label,
        "검증된 FM27 건강 시험 빌드가 아닙니다"
    );
    block_pending(s)?;
    let mut j = Journal {
        id: format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            s.pid
        ),
        session: s.clone(),
        player_id,
        player_base,
        state: "planned".into(),
        before: before.clone(),
        worker_code: 0,
        worker_result: 0,
        note: "Experimental non-undoable native health operation".into(),
        operation: operation.into(),
    };
    persist(&j)?;
    execute_with(s, &mut j, 5000, preflight, build_stub)?;
    Ok(j.id)
}
fn execute(
    s: &Session,
    function: u64,
    player: u64,
    j: &mut Journal,
    wait_ms: u32,
    preflight: impl FnOnce() -> Result<()>,
) -> Result<()> {
    execute_with(s, j, wait_ms, preflight, |result| {
        stub(function, player, result)
    })
}
fn execute_with(
    s: &Session,
    j: &mut Journal,
    wait_ms: u32,
    preflight: impl FnOnce() -> Result<()>,
    build: impl FnOnce(u64) -> Vec<u8>,
) -> Result<()> {
    let h = Handle(unsafe {
        OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_READ
                | PROCESS_VM_WRITE,
            0,
            s.pid,
        )
    });
    ensure!(
        !h.0.is_null(),
        "건강 시험을 위한 FM 접근 실패: {}",
        std::io::Error::last_os_error()
    );
    let p = Process::open(s.pid, false)?;
    ensure!(p.created()? == s.created, "대상 프로세스가 재시작됐습니다");
    let mut pages = Pages {
        process: h.0,
        code: 0,
        result: 0,
        safe: true,
    };
    pages.result = unsafe {
        VirtualAllocEx(
            h.0,
            std::ptr::null(),
            8,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    } as u64;
    ensure!(pages.result != 0, "건강 시험 완료 셀 준비 실패");
    pages.code = unsafe {
        VirtualAllocEx(
            h.0,
            std::ptr::null(),
            4096,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    } as u64;
    ensure!(pages.code != 0, "건강 시험 작업자 준비 실패");
    let bytes = build(pages.result);
    ensure!(bytes.len() <= 4096, "건강 시험 작업자 코드 크기 초과");
    write(h.0, pages.result, &[0; 8])?;
    write(h.0, pages.code, &bytes)?;
    let mut old = 0;
    ensure!(
        unsafe {
            VirtualProtectEx(
                h.0,
                pages.code as *const c_void,
                4096,
                PAGE_EXECUTE_READ,
                &mut old,
            )
        } != 0,
        "건강 시험 실행 권한 설정 실패"
    );
    ensure!(
        unsafe { FlushInstructionCache(h.0, pages.code as *const c_void, bytes.len()) } != 0,
        "건강 시험 코드 동기화 실패"
    );
    j.worker_code = pages.code;
    j.worker_result = pages.result;
    j.state = "prepared".into();
    persist(j)?;
    // Last full target/health/code validation after scratch preparation. The
    // experiment does not suspend every FM thread while native cleanup runs.
    if let Err(error) = preflight() {
        j.state = "not_started".into();
        j.note = format!("Preflight rejected: {error:#}");
        persist(j)?;
        return Err(error);
    }
    ensure!(
        p.created()? == s.created,
        "호출 직전에 대상 프로세스가 재시작됐습니다"
    );
    let start: unsafe extern "system" fn(*mut c_void) -> u32 =
        unsafe { std::mem::transmute(pages.code as usize) };
    let thread = Handle(unsafe {
        CreateRemoteThread(
            h.0,
            std::ptr::null(),
            0,
            Some(start),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
        )
    });
    if thread.0.is_null() {
        j.state = "not_started".into();
        j.note = "CreateRemoteThread failed before execution".into();
        persist(j)?;
        bail!(
            "건강 시험 작업자를 시작하지 못했습니다: {}",
            std::io::Error::last_os_error()
        );
    }
    // From here even a transport/journal error must retain scratch and block
    // retries. Never kill FM, its thread, or live code on timeout.
    pages.safe = false;
    let waited = unsafe { WaitForSingleObject(thread.0, wait_ms) };
    if waited != WAIT_OBJECT_0 {
        j.state = "uncertain_timeout".into();
        j.note = format!("wait={waited:#x}; scratch retained; do not retry");
        let _ = persist(j);
        bail!(
            "건강 시험 결과가 불확실합니다 (기록 {}). 작업자 메모리를 보존했습니다. 자동 재시도하지 마세요",
            j.id
        );
    }
    let mut exit = u32::MAX;
    if unsafe { GetExitCodeThread(thread.0, &mut exit) } == 0
        || exit != 0
        || p.u64(pages.result)? != COMPLETE
    {
        j.state = "abnormal_exit".into();
        j.note = format!("exit={exit:#x}; completion not certified; scratch retained");
        let _ = persist(j);
        bail!(
            "건강 시험 작업자가 정상 완료되지 않았습니다 (기록 {}). 자동 재시도하지 마세요",
            j.id
        );
    }
    j.state = "native_completed".into();
    j.note = "Native helper returned; independent verification pending".into();
    persist(j)?;
    pages.safe = true;
    Ok(())
}
#[cfg(test)]
pub(crate) fn owned_ban_fixture(
    s: &Session,
    id: &str,
    build: impl FnOnce(u64) -> Vec<u8>,
) -> Result<()> {
    let mut j = Journal {
        id: id.into(),
        session: s.clone(),
        player_id: 0,
        player_base: 0,
        state: "planned".into(),
        before: serde_json::json!({"owned_fixture":true}),
        worker_code: 0,
        worker_result: 0,
        note: "Owned ban ABI fixture only".into(),
        operation: "owned_ban_fixture".into(),
    };
    persist(&j)?;
    execute_with(s, &mut j, 5000, || Ok(()), build)?;
    verified(id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_verified_journal_defaults_operation_without_blocking_next_experiment() {
        let v = serde_json::json!({"id":"1-2","session":{"pid":0,"created":0,"module_base":0,"module_size":0,"module_stamp":0,"db":0,"registry_begin":0,"game_date":"fixture","profile":"fixture","own_club_id":null},"player_id":1,"player_base":0,"state":"verified","before":{},"worker_code":0,"worker_result":0,"note":"old injury journal"});
        let row: Journal = serde_json::from_value(v).unwrap();
        assert_eq!(row.state, "verified");
        assert_eq!(row.operation, "");
        assert!(
            run_experiment(
                &row.session,
                1,
                0,
                &serde_json::json!({}),
                "unknown",
                |_| panic!("unsupported operation must not build code"),
                || panic!("unsupported operation must not read a process")
            )
            .is_err()
        );
    }
    #[test]
    fn void_worker_uses_rcx_aligned_shadow_space_and_post_call_marker() {
        let b = stub(0x1234567812345678, 0x2345678923456788, 0x3456789A34567898);
        assert_eq!(&b[..6], &[0x48, 0x83, 0xEC, 0x28, 0x48, 0xB9]);
        assert_eq!(&b[6..14], &0x2345678923456788u64.to_le_bytes());
        assert_eq!(&b[24..26], &[0xFF, 0xD0]);
        assert_eq!(&b[38..46], &COMPLETE.to_le_bytes());
        assert_eq!(
            &b[b.len() - 7..],
            &[0x48, 0x83, 0xC4, 0x28, 0x31, 0xC0, 0xC3][..]
        );
        assert!(
            !b.windows(2).any(|w| w == [0x48, 0xA3]),
            "must not save helper RAX as allocator result"
        );
    }
    static FIXTURE_VALUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    unsafe extern "system" fn fixture_void(receiver: usize) -> usize {
        let value = unsafe { &*(receiver as *const std::sync::atomic::AtomicU64) };
        value.store(42, std::sync::atomic::Ordering::SeqCst);
        0 // Explicitly not an allocation pointer; success must use the marker.
    }
    unsafe extern "system" fn fixture_slow(receiver: usize) -> usize {
        std::thread::sleep(std::time::Duration::from_millis(250));
        unsafe { fixture_void(receiver) }
    }
    #[test]
    #[ignore = "Private owned-process fixture, started only by the health worker test"]
    fn worker_child() {
        let file = PathBuf::from(
            std::env::var_os("TOUCHLINE_HEALTH_OWNED_FIXTURE")
                .expect("owned fixture handshake required"),
        );
        std::fs::write(&file,serde_json::to_vec(&serde_json::json!({"call":fixture_void as *const () as usize,"slow":fixture_slow as *const () as usize,"receiver":&FIXTURE_VALUE as *const _ as usize})).unwrap()).unwrap();
        for _ in 0..1500 {
            if file.with_extension("stop").exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    struct Fixture {
        child: std::process::Child,
        file: PathBuf,
        journals: Vec<PathBuf>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::write(self.file.with_extension("stop"), b"stop");
            for _ in 0..50 {
                if self.child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.file);
            let _ = std::fs::remove_file(self.file.with_extension("stop"));
            for p in &self.journals {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    #[test]
    fn owned_fixture_void_return_preflight_rejection_and_timeout_preservation() {
        use std::os::windows::process::CommandExt;
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let file = std::env::temp_dir().join(format!("touchline-health-owned-fixture-{id}.json"));
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "native_health::tests::worker_child",
                "--nocapture",
            ])
            .env("TOUCHLINE_HEALTH_OWNED_FIXTURE", &file)
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut fixture = Fixture {
            child,
            file,
            journals: vec![],
        };
        for _ in 0..400 {
            if fixture.file.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let info: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&fixture.file).unwrap()).unwrap();
        let p = Process::open(fixture.child.id(), false).unwrap();
        let s = Session {
            pid: fixture.child.id(),
            created: p.created().unwrap(),
            module_base: 0,
            module_size: 0,
            module_stamp: 0,
            db: 0,
            registry_begin: 0,
            game_date: "fixture".into(),
            profile: "owned-health-fixture".into(),
            own_club_id: None,
        };
        let receiver = info["receiver"].as_u64().unwrap();
        let mut j = Journal {
            id: format!("{id}-1"),
            session: s.clone(),
            player_id: 1,
            player_base: receiver,
            state: "planned".into(),
            before: serde_json::json!({"fixture":true}),
            worker_code: 0,
            worker_result: 0,
            note: "Owned test process only".into(),
            operation: "owned_fixture".into(),
        };
        fixture.journals.push(path(&j.id).unwrap());
        assert!(
            execute(
                &s,
                info["call"].as_u64().unwrap(),
                receiver,
                &mut j,
                5000,
                || bail!("fixture preflight rejection")
            )
            .is_err()
        );
        assert_eq!(j.state, "not_started");
        assert_eq!(p.u64(receiver).unwrap(), 0);
        assert!(block_pending(&s).is_ok());
        j.id = format!("{id}-2");
        fixture.journals.push(path(&j.id).unwrap());
        execute(
            &s,
            info["call"].as_u64().unwrap(),
            receiver,
            &mut j,
            5000,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(p.u64(receiver).unwrap(), 42);
        assert_eq!(j.state, "native_completed");
        assert!(block_pending(&s).is_err());
        verified(&j.id).unwrap();
        assert!(block_pending(&s).is_ok());
        j.id = format!("{id}-3");
        fixture.journals.push(path(&j.id).unwrap());
        assert!(
            execute(
                &s,
                info["slow"].as_u64().unwrap(),
                receiver,
                &mut j,
                1,
                || Ok(())
            )
            .is_err()
        );
        assert_eq!(j.state, "uncertain_timeout");
        assert!(block_pending(&s).is_err());
        assert!(
            p.read(j.worker_code, 8).is_ok(),
            "uncertain worker code must remain mapped"
        );
        for _ in 0..100 {
            if p.u64(j.worker_result).unwrap() == COMPLETE {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            p.u64(j.worker_result).unwrap(),
            COMPLETE,
            "timed-out owned worker must be allowed to finish"
        );
        assert!(
            block_pending(&s).is_err(),
            "late completion must never silently permit a retry"
        );
    }
}
