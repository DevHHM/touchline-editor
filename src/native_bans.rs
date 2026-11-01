//! Fixed official FM27 all-bans helper plus the outer caller's notifications.
use crate::native_health::COMPLETE;
use crate::{model::Session, native_health};
use anyhow::Result;
#[derive(Clone, Copy)]
struct Targets {
    helper: u64,
    manager_cell: u64,
    notify: u64,
    static_object: u64,
}
pub(crate) fn remove_bans(
    s: &Session,
    id: u32,
    player: u64,
    before: &serde_json::Value,
    check: impl FnOnce() -> Result<()>,
) -> Result<String> {
    native_health::remove_bans(s, id, player, before, check)
}
pub(crate) fn stub(base: u64, player: u64, result: u64) -> Vec<u8> {
    build(
        Targets {
            helper: base + 0x15FB170,
            manager_cell: base + 0x56FFB50,
            notify: base + 0xA17190,
            static_object: base + 0x56FF2E0,
        },
        player,
        result,
    )
}
fn mov_rax(b: &mut Vec<u8>, n: u64) {
    b.extend([0x48, 0xB8]);
    b.extend(n.to_le_bytes());
}
fn build(t: Targets, player: u64, result: u64) -> Vec<u8> {
    // Save the receiver in RBX across every native call. Entry RSP=8 mod16;
    // push RBX then reserve 0x20 shadow bytes keeps all CALL sites aligned.
    let mut b = vec![0x53, 0x48, 0x83, 0xEC, 0x20, 0x48, 0xBB];
    b.extend(player.to_le_bytes());
    b.extend([0x48, 0x89, 0xD9]);
    mov_rax(&mut b, t.helper);
    b.extend([0xFF, 0xD0]);
    // Recompute the signed interface adjustment AFTER removal, as native caller.
    b.extend([
        0x48, 0x8B, 0x43, 0x08, 0x48, 0x63, 0x40, 0x04, 0x48, 0x8D, 0x54, 0x03, 0x08,
    ]);
    mov_rax(&mut b, t.manager_cell);
    b.extend([
        0x48, 0x8B, 0x08, 0x48, 0x8B, 0x01, 0xFF, 0x90, 0x98, 0, 0, 0,
    ]);
    // This is a static object ADDRESS (native LEA), never its first stored pointer.
    b.extend([0x48, 0xB9]);
    b.extend(t.static_object.to_le_bytes());
    b.extend([0xB2, 1]);
    mov_rax(&mut b, t.notify);
    b.extend([0xFF, 0xD0]);
    b.extend([0x48, 0xBA]);
    b.extend(result.to_le_bytes());
    mov_rax(&mut b, COMPLETE);
    b.extend([
        0x48, 0x89, 0x02, 0x48, 0x83, 0xC4, 0x20, 0x5B, 0x31, 0xC0, 0xC3,
    ]);
    b
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Process;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static STAGE: AtomicU64 = AtomicU64::new(0);
    #[repr(C)]
    struct Receiver {
        reserved: u64,
        table: *const u32,
        expected: u64,
        manager: u64,
        object: u64,
    }
    unsafe extern "system" fn helper(p: usize) -> usize {
        let p = unsafe { &mut *(p as *mut Receiver) };
        STAGE.store(1, Ordering::SeqCst); // Deliberately change adjustment to prove the stub reloads it after helper.
        unsafe {
            *(p.table.add(1) as *mut u32) = (-16i32) as u32;
        }
        0
    }
    unsafe extern "system" fn manager(m: usize, p: usize) -> usize {
        let r = unsafe { *((m as *const u64).add(1)) } as *const Receiver;
        let r = unsafe { &*r };
        if STAGE.load(Ordering::SeqCst) == 1 && m as u64 == r.manager && p as u64 == r.expected {
            STAGE.store(2, Ordering::SeqCst);
        } else {
            STAGE.store(99, Ordering::SeqCst);
        }
        0
    }
    unsafe extern "system" fn notify(o: usize, flag: u8) -> usize {
        let r = unsafe { &*(*(o as *const u64) as *const Receiver) };
        if STAGE.load(Ordering::SeqCst) == 2 && o as u64 == r.object && flag == 1 {
            STAGE.store(3, Ordering::SeqCst);
        } else {
            STAGE.store(99, Ordering::SeqCst);
        }
        0
    }
    #[test]
    #[ignore = "Owned ban-sequence child only; launched by enclosing test"]
    fn worker_child() {
        let file = PathBuf::from(
            std::env::var_os("TOUCHLINE_BAN_OWNED_FIXTURE").expect("owned fixture required"),
        );
        let table = Box::into_raw(Box::new([0u32, 32u32])) as *mut u32;
        let mut vt = Box::new([0u64; 20]);
        vt[0x98 / 8] = manager as *const () as u64;
        let vt = Box::into_raw(vt) as u64;
        let mgr = Box::into_raw(Box::new([vt, 0u64]));
        let object = Box::into_raw(Box::new(0u64));
        let recv = Box::into_raw(Box::new(Receiver {
            reserved: 0,
            table,
            expected: 0,
            manager: mgr as u64,
            object: object as u64,
        }));
        unsafe {
            (*recv).expected = (recv as u64) - 8;
            (*mgr)[1] = recv as u64;
            *object = recv as u64;
        }
        let cell = Box::into_raw(Box::new(mgr as u64));
        std::fs::write(&file,serde_json::to_vec(&serde_json::json!({"helper":helper as *const()as u64,"manager_cell":cell as u64,"notify":notify as *const()as u64,"static_object":object as u64,"player":recv as u64,"stage":&STAGE as *const _ as u64})).unwrap()).unwrap();
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
        journal: PathBuf,
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
            let _ = std::fs::remove_file(&self.journal);
        }
    }
    #[test]
    fn owned_fixture_executes_helper_then_signed_adjusted_manager_then_static_notification() {
        use std::os::windows::process::CommandExt;
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let file = std::env::temp_dir().join(format!("touchline-ban-owned-{id}.json"));
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "native_bans::tests::worker_child",
                "--nocapture",
            ])
            .env("TOUCHLINE_BAN_OWNED_FIXTURE", &file)
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let fixture = Fixture {
            child,
            file,
            journal: crate::transaction::data_dir()
                .join("health-experiments")
                .join(format!("{id}.json")),
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
            profile: "owned-ban-fixture".into(),
            own_club_id: None,
        };
        let t = Targets {
            helper: info["helper"].as_u64().unwrap(),
            manager_cell: info["manager_cell"].as_u64().unwrap(),
            notify: info["notify"].as_u64().unwrap(),
            static_object: info["static_object"].as_u64().unwrap(),
        };
        native_health::owned_ban_fixture(&s, &id, |result| {
            build(t, info["player"].as_u64().unwrap(), result)
        })
        .unwrap();
        assert_eq!(
            p.u64(info["stage"].as_u64().unwrap()).unwrap(),
            3,
            "every native ABI stage must receive correct arguments in order"
        );
    }
    #[test]
    fn ban_stub_preserves_rbx_uses_signed_adjustment_and_lea_semantics() {
        let b = stub(0x180000000, 0x300000, 0x400000);
        assert_eq!(&b[..7], &[0x53, 0x48, 0x83, 0xEC, 0x20, 0x48, 0xBB]);
        assert!(b.windows(4).any(|w| w == [0x48, 0x63, 0x40, 0x04]));
        let address = (0x180000000u64 + 0x56FF2E0).to_le_bytes();
        assert!(b.windows(8).any(|w| w == address));
        assert_eq!(
            &b[b.len() - 8..],
            &[0x48, 0x83, 0xC4, 0x20, 0x5B, 0x31, 0xC0, 0xC3]
        );
    }
}
