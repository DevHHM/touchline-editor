//! Fixed FM27 player transfer experiment. All game objects are native-owned.
//! Exact two-call ABI: reference/player-transfer-20261101-latest.md.
use crate::{model::Session, native_health};
use anyhow::Result;

pub(crate) const LOOKUP: u64 = 0x1A4BFC0;
pub(crate) const TRANSFER: u64 = 0xBAC8D0;
pub(crate) const DATE_SENTINEL: u32 = 0x076C0001;
#[derive(Clone, Copy)]
struct Targets {
    lookup: u64,
    transfer: u64,
}

fn mov64(code: &mut Vec<u8>, prefix: &[u8], value: u64) {
    code.extend(prefix);
    code.extend(value.to_le_bytes());
}
fn clear_stack(code: &mut Vec<u8>, offset: u8) {
    code.extend([0x48, 0xC7, 0x44, 0x24, offset, 0, 0, 0, 0]);
}
fn call(code: &mut Vec<u8>, function: u64) {
    mov64(code, &[0x48, 0xB8], function);
    code.extend([0xFF, 0xD0]);
}
fn stub(module: u64, player: u64, person: u64, manager: u64, team: u64, result: u64) -> Vec<u8> {
    build(
        Targets {
            lookup: module + LOOKUP,
            transfer: module + TRANSFER,
        },
        player,
        person,
        manager,
        team,
        result,
    )
}
fn build(
    targets: Targets,
    player: u64,
    person: u64,
    manager: u64,
    team: u64,
    result: u64,
) -> Vec<u8> {
    // Entry RSP=8 mod16; 0x58 aligns it. First call has arguments through
    // [rsp+0x48]. The one-qword owning wrapper lives separately at +0x50.
    let mut b = vec![0x48, 0x83, 0xEC, 0x58];
    for offset in [0x20, 0x28, 0x30, 0x38, 0x40, 0x48, 0x50] {
        clear_stack(&mut b, offset);
    }
    mov64(&mut b, &[0x48, 0xB9], manager + 0x18);
    b.extend([0x48, 0x8D, 0x54, 0x24, 0x50]);
    mov64(&mut b, &[0x49, 0xB8], person);
    mov64(&mut b, &[0x49, 0xB9], team);
    call(&mut b, targets.lookup);
    // Do not assume the first callee preserved its incoming stack arguments.
    for offset in [0x20, 0x28, 0x30, 0x38, 0x40] {
        clear_stack(&mut b, offset);
    }
    b.extend([0xC7, 0x44, 0x24, 0x28]);
    b.extend(DATE_SENTINEL.to_le_bytes());
    mov64(&mut b, &[0x48, 0xB9], manager);
    mov64(&mut b, &[0x48, 0xBA], player);
    b.extend([0x4C, 0x8D, 0x44, 0x24, 0x50]);
    mov64(&mut b, &[0x49, 0xB9], team);
    call(&mut b, targets.transfer);
    // Exactly the native caller's optional owning-wrapper release. No CRT free.
    b.extend([0x48, 0x8B, 0x4C, 0x24, 0x50, 0x48, 0x85, 0xC9, 0x74, 0]);
    let empty = b.len() - 1;
    b.extend([0xF0, 0xFF, 0x49, 0x08, 0x75, 0]);
    let retained = b.len() - 1;
    b.extend([0x48, 0x8B, 0x01, 0xBA, 1, 0, 0, 0, 0xFF, 0x10]);
    let completed = b.len();
    b[empty] = (completed - empty - 1) as u8;
    b[retained] = (completed - retained - 1) as u8;
    mov64(&mut b, &[0x48, 0xBA], result);
    mov64(&mut b, &[0x48, 0xB8], native_health::COMPLETE);
    b.extend([0x48, 0x89, 0x02, 0x48, 0x83, 0xC4, 0x58, 0x31, 0xC0, 0xC3]);
    b
}
pub(crate) fn apply(
    session: &Session,
    player_id: u32,
    player: u64,
    person: u64,
    manager: u64,
    team: u64,
    before: &serde_json::Value,
    preflight: impl FnOnce() -> Result<()>,
) -> Result<String> {
    native_health::run_experiment(
        session,
        player_id,
        player,
        before,
        "transfer",
        |result| stub(session.module_base, player, person, manager, team, result),
        preflight,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Process;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU32, AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    static STAGE: AtomicU64 = AtomicU64::new(0);
    static ROUND: AtomicU64 = AtomicU64::new(0);
    static OFFER: AtomicU64 = AtomicU64::new(0);
    static WRAPPER: AtomicU64 = AtomicU64::new(0);
    #[repr(C)]
    struct Offer {
        vtable: u64,
        refs: AtomicU32,
        padding: u32,
    }
    const FIX_MANAGER: usize = 0x33333330;
    const FIX_PERSON: usize = 0x22222220;
    const FIX_PLAYER: usize = 0x11111110;
    const FIX_TEAM: usize = 0x44444440;
    unsafe extern "system" fn lookup(
        manager: usize,
        out: *mut usize,
        person: usize,
        team: usize,
        fifth: usize,
        sixth: usize,
        seventh: usize,
        eighth: usize,
        ninth: usize,
        tenth: usize,
    ) -> usize {
        let valid = manager == FIX_MANAGER + 0x18
            && person == FIX_PERSON
            && team == FIX_TEAM
            && [fifth, sixth, seventh, eighth, ninth, tenth] == [0; 6]
            && unsafe { *out } == 0;
        STAGE.store(if valid { 1 } else { 99 }, Ordering::SeqCst);
        WRAPPER.store(out as u64, Ordering::SeqCst);
        let round = ROUND.fetch_add(1, Ordering::SeqCst);
        let offer = OFFER.load(Ordering::SeqCst) as *mut Offer;
        unsafe {
            (*offer)
                .refs
                .store(if round == 1 { 2 } else { 1 }, Ordering::SeqCst);
            *out = if round == 2 { 0 } else { offer as usize };
        }
        // Deliberately not the output wrapper address: caller must ignore RAX.
        0
    }
    unsafe extern "system" fn transfer(
        manager: usize,
        player: usize,
        out: *mut usize,
        team: usize,
        fee: i32,
        date: u32,
        seventh: u8,
        eighth: usize,
        ninth: u8,
    ) -> usize {
        let round = ROUND.load(Ordering::SeqCst) - 1;
        let expected = if round == 2 {
            0
        } else {
            OFFER.load(Ordering::SeqCst) as usize
        };
        let valid = STAGE.load(Ordering::SeqCst) == 1
            && manager == FIX_MANAGER
            && player == FIX_PLAYER
            && team == FIX_TEAM
            && out as u64 == WRAPPER.load(Ordering::SeqCst)
            && unsafe { *out } == expected
            && fee == 0
            && date == DATE_SENTINEL
            && seventh == 0
            && eighth == 0
            && ninth == 0;
        STAGE.store(if valid { 2 } else { 99 }, Ordering::SeqCst);
        // Deliberately not zero/pointer/success: completion uses post-call marker.
        0xDEAD_BEEF
    }
    unsafe extern "system" fn destroy(object: *mut Offer, flag: u32) -> usize {
        let valid = STAGE.load(Ordering::SeqCst) == 2
            && object as u64 == OFFER.load(Ordering::SeqCst)
            && flag == 1
            && unsafe { (*object).refs.load(Ordering::SeqCst) } == 0;
        STAGE.store(if valid { 3 } else { 99 }, Ordering::SeqCst);
        0xBAD0
    }
    #[test]
    #[ignore = "Owned transfer ABI child only; launched by enclosing test"]
    fn worker_child() {
        let file = PathBuf::from(
            std::env::var_os("TOUCHLINE_TRANSFER_OWNED_FIXTURE").expect("owned fixture required"),
        );
        let vt = Box::into_raw(Box::new([destroy as *const () as u64])) as u64;
        let offer = Box::into_raw(Box::new(Offer {
            vtable: vt,
            refs: AtomicU32::new(0),
            padding: 0,
        }));
        OFFER.store(offer as u64, Ordering::SeqCst);
        std::fs::write(&file,serde_json::to_vec(&serde_json::json!({"lookup":lookup as *const()as u64,"transfer":transfer as *const()as u64,
            "stage":&STAGE as *const _ as u64,"round":&ROUND as *const _ as u64,"refs":unsafe{&(*offer).refs}as *const _ as u64})).unwrap()).unwrap();
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
            for j in &self.journals {
                let _ = std::fs::remove_file(j);
            }
        }
    }
    #[test]
    fn owned_fixture_verifies_ten_then_nine_arguments_same_wrapper_cleanup_and_null_case() {
        use std::os::windows::process::CommandExt;
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let file = std::env::temp_dir().join(format!("touchline-transfer-owned-{id}.json"));
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "native_transfer::tests::worker_child",
                "--nocapture",
            ])
            .env("TOUCHLINE_TRANSFER_OWNED_FIXTURE", &file)
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
            profile: "owned-transfer-fixture".into(),
            own_club_id: None,
        };
        let targets = Targets {
            lookup: info["lookup"].as_u64().unwrap(),
            transfer: info["transfer"].as_u64().unwrap(),
        };
        for (round, expected_stage, expected_refs) in [(0, 3, 0), (1, 2, 1), (2, 2, 1)] {
            let journal_id = format!("{id}-{round}");
            fixture.journals.push(
                crate::transaction::data_dir()
                    .join("health-experiments")
                    .join(format!("{journal_id}.json")),
            );
            // Shared test-only runner executes only in this child; no game process is opened.
            native_health::owned_ban_fixture(&s, &journal_id, |result| {
                build(
                    targets,
                    FIX_PLAYER as u64,
                    FIX_PERSON as u64,
                    FIX_MANAGER as u64,
                    FIX_TEAM as u64,
                    result,
                )
            })
            .unwrap();
            assert_eq!(
                p.u64(info["stage"].as_u64().unwrap()).unwrap(),
                expected_stage,
                "lookup10/transfer9/cleanup must execute in order"
            );
            assert_eq!(
                p.u32(info["refs"].as_u64().unwrap()).unwrap(),
                expected_refs,
                "owning wrapper must release exactly one reference; null must not destroy"
            );
            assert_eq!(
                p.u64(info["round"].as_u64().unwrap()).unwrap(),
                round + 1,
                "null lookup must still reach native transfer"
            );
        }
    }
    #[test]
    fn fixed_two_call_worker_has_all_stack_arguments_native_cleanup_and_marker() {
        let module = 0x180000000;
        let b = stub(
            module, 0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x55555555,
        );
        assert_eq!(&b[..4], &[0x48, 0x83, 0xEC, 0x58]);
        for address in [module + LOOKUP, module + TRANSFER] {
            let sites = b
                .windows(8)
                .enumerate()
                .filter(|(_, w)| *w == address.to_le_bytes())
                .map(|(n, _)| n)
                .collect::<Vec<_>>();
            assert_eq!(sites.len(), 1);
            assert_eq!(&b[sites[0] + 8..sites[0] + 10], &[0xFF, 0xD0]);
        }
        assert!(
            b.windows(8)
                .any(|w| w == [0xC7, 0x44, 0x24, 0x28, 1, 0, 0x6C, 7])
        );
        assert!(b.windows(4).any(|w| w == [0xF0, 0xFF, 0x49, 8]));
        let test = b
            .windows(5)
            .position(|w| w == [0x48, 0x85, 0xC9, 0x74, 0x10])
            .unwrap();
        let jump = test + 4;
        let destination = jump + 1 + b[jump] as usize;
        assert_eq!(&b[destination..destination + 2], &[0x48, 0xBA]);
        assert_eq!(
            &b[b.len() - 7..],
            &[0x48, 0x83, 0xC4, 0x58, 0x31, 0xC0, 0xC3]
        );
        assert!(
            b.windows(8)
                .any(|w| w == native_health::COMPLETE.to_le_bytes())
        );
        assert!(!b.windows(2).any(|w| w == [0x48, 0xA3]));
    }
}
