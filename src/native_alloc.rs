//! Native context storage. Career records are reserved by their dedicated pool
//! transaction, never manufactured with CRT storage. The verified out-of-table
//! branch uses the game's CRT malloc/free pair with no prefix.
//! See reference/fm-native-lifecycle/relation-owner-decompile.
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

const MAX_BYTES: usize = 16_384;
const CONTEXT_POINTER: u64 = 0x4E36180;
const CONTEXT_STATIC: u64 = 0x4E4B190;
const MALLOC_THUNK: u64 = 0x430F450;
const MALLOC_IAT: u64 = 0x493DBC8;
const FREE_IAT: u64 = 0x493DBC0;
// Current-build native allocation/free and owner destructor instruction prefixes.
const CODE_SIGNATURES: &[(u64, &str)] = &[
    (
        0xE6DF10,
        "41565657534883ec284889d680793400743e488b4118488b51204829c248c1fa",
    ),
    (
        0xE6DFD0,
        "415741564154565755534883ec204885d20f84eb0000004983f8ff740b807934",
    ),
    (
        0x333A6C0,
        "41565657534883ec284889ce488b5930488b79384839fb743f4989deeb0b6690",
    ),
    (
        0xF1B870,
        "4157415641554154565755534883ec284889d34889ce488b7908483b79107417",
    ),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeAllocation {
    pub id: String,
    pub address: u64,
    pub len: usize,
    pub session: Session,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Policy {
    context: u64,
    table_begin: u64,
    table_end: u64,
    table_capacity: u64,
    prefix: u32,
    enabled: u8,
    malloc: u64,
    free: u64,
    len: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Ledger {
    allocation: NativeAllocation,
    state: String,
    origin: String,
    owner: Option<u64>,
    policy: Option<Policy>,
    worker_code: u64,
    worker_result: u64,
    note: String,
}
fn stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}
fn folder() -> Result<PathBuf> {
    let path = transaction::data_dir().join("native-allocations");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
fn ledger_path(id: &str) -> Result<PathBuf> {
    ensure!(
        !id.is_empty() && id.len() < 100 && id.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "할당 기록 ID 오류"
    );
    Ok(folder()?.join(format!("{id}.json")))
}
fn persist(ledger: &Ledger) -> Result<()> {
    transaction::persist_json(&ledger_path(&ledger.allocation.id)?, ledger)
}
fn same_session(a: &Session, b: &Session) -> Result<bool> {
    Ok(serde_json::to_vec(a)? == serde_json::to_vec(b)?)
}
fn read_ledger(a: &NativeAllocation) -> Result<Ledger> {
    let ledger: Ledger = serde_json::from_slice(&std::fs::read(ledger_path(&a.id)?)?)?;
    ensure!(
        ledger.allocation.id == a.id
            && ledger.allocation.address == a.address
            && ledger.allocation.len == a.len
            && same_session(&ledger.allocation.session, &a.session)?,
        "할당 기록과 요청이 다릅니다"
    );
    Ok(ledger)
}
fn address(a: u64) -> bool {
    (0x10000..0x0000_8000_0000_0000).contains(&a)
}
fn verify_no_pending(s: &Session) -> Result<()> {
    for item in std::fs::read_dir(folder()?)? {
        let item = item?;
        if item.path().extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(row) = serde_json::from_slice::<Ledger>(&std::fs::read(item.path())?) else {
            continue;
        };
        if row.allocation.session.pid == s.pid
            && row.allocation.session.created == s.created
            && ["worker_prepared", "uncertain_timeout"].contains(&row.state.as_str())
        {
            bail!(
                "이 FM 실행에서 완료 여부가 확인되지 않은 할당 작업이 있습니다. 중복 실행을 차단했습니다 (기록 {})",
                row.allocation.id
            );
        }
    }
    Ok(())
}
fn sized(requested: usize, entries: usize) -> Result<usize> {
    ensure!(
        requested > 0 && requested <= MAX_BYTES,
        "관계 배열 크기가 검증 범위를 벗어납니다"
    );
    let len = requested
        .max(entries)
        .max(2048)
        .checked_add(15)
        .context("할당 크기 초과")?
        & !15;
    ensure!(
        len <= MAX_BYTES,
        "FM 메모리 풀 범위를 벗어나는 관계 배열을 확보할 수 없습니다"
    );
    Ok(len)
}
fn policy(p: &Process, s: &Session, requested: usize) -> Result<Policy> {
    backend::validate_with(p, s)?;
    ensure!(
        s.module_stamp == 0x6A229E8A && s.module_size == 0x1EB41000,
        "관계 확장이 검증되지 않은 FM 빌드입니다"
    );
    for (rva, hex) in CODE_SIGNATURES {
        let expected: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        ensure!(
            p.read(s.module_base + rva, expected.len())? == expected,
            "FM 관계 할당·해제 코드가 변경됐습니다"
        );
    }
    let thunk = p.read(s.module_base + MALLOC_THUNK, 6)?;
    ensure!(
        &thunk[..2] == [0xff, 0x25],
        "FM malloc 연결 코드가 변경됐습니다"
    );
    let displacement = i32::from_le_bytes(thunk[2..6].try_into().unwrap()) as i64;
    ensure!(
        (MALLOC_THUNK as i64 + 6 + displacement) as u64 == MALLOC_IAT,
        "FM malloc 대상이 다릅니다"
    );
    let malloc = p.u64(s.module_base + MALLOC_IAT)?;
    let free = p.u64(s.module_base + FREE_IAT)?;
    ensure!(
        p.modules()?
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("ucrtbase.dll")
                && (m.base..m.base + m.size as u64).contains(&malloc)
                && (m.base..m.base + m.size as u64).contains(&free)),
        "FM의 CRT 할당·해제 연결을 검증할 수 없습니다"
    );
    let context = p.u64(s.module_base + CONTEXT_POINTER)?;
    ensure!(
        context == s.module_base + CONTEXT_STATIC,
        "FM 관계 할당기가 아직 초기화되지 않았거나 변경됐습니다"
    );
    let b = p.u64(context + 0x18)?;
    let e = p.u64(context + 0x20)?;
    let c = p.u64(context + 0x28)?;
    ensure!(
        address(b)
            && b < e
            && e <= c
            && c - b <= (MAX_BYTES as u64) * 8
            && (e - b) % 8 == 0
            && (c - b) % 8 == 0,
        "관계 메모리 풀 테이블 범위 오류"
    );
    let prefix = p.u32(context + 0x30)?;
    let enabled = p.u8(context + 0x34)?;
    ensure!(
        prefix == 0 && enabled == 1,
        "관계 할당기가 초기화되지 않았거나 형식이 다릅니다"
    );
    Ok(Policy {
        context,
        table_begin: b,
        table_end: e,
        table_capacity: c,
        prefix,
        enabled,
        malloc,
        free,
        len: sized(requested, ((e - b) / 8) as usize)?,
    })
}
pub fn available(s: &Session) -> Result<()> {
    verify_no_pending(s)?;
    policy(&Process::open(s.pid, false)?, s, 16).map(|_| ())
}

/// Career vectors and their 24-byte holder use this separate native context.
/// Holder destruction uses the size-24 entry; it must be allocated with that
/// allocator, not an oversized CRT buffer. Pointer vectors may use the verified
/// zero-prefix CRT fallback once their capacity exceeds the context table.
pub fn career_storage_minimum(s: &Session) -> Result<usize> {
    let p = Process::open(s.pid, false)?;
    backend::validate_with(&p, s)?;
    ensure!(s.module_stamp == 0x6A229E8A && s.module_size == 0x1EB41000, "커리어 할당기가 검증되지 않은 FM 빌드입니다");
    let expected = [0x41,0x56,0x56,0x57,0x53,0x48,0x83,0xec,0x28,0x48,0x89,0xd6,0x80,0x79,0x34,0x00,0x74,0x3f];
    ensure!(p.read(s.module_base+0x9234B0, expected.len())? == expected, "커리어 컨텍스트 할당 코드가 다릅니다");
    let context = s.module_base + 0x4E44E68;
    ensure!(p.u64(s.module_base+0x4E35EB8)? == context, "커리어 할당기 초기화가 확인되지 않았습니다");
    let b=p.u64(context+0x18)?; let e=p.u64(context+0x20)?; let c=p.u64(context+0x28)?;
    ensure!(address(b) && b<e && e<=c && c-b<=MAX_BYTES as u64*8 && (e-b)%8==0 && (c-b)%8==0 && p.u32(context+0x30)?==0 && p.u8(context+0x34)?==1, "커리어 메모리 컨텍스트 경계 검증 실패");
    sized(16, ((e-b)/8) as usize)
}

pub fn allocate_career_holder(s: &Session) -> Result<NativeAllocation> {
    verify_no_pending(s)?;
    career_storage_minimum(s)?;
    let mut ledger = Ledger {
        allocation: NativeAllocation { id:format!("{}-{}",stamp(),s.pid), address:0, len:24, session:s.clone() },
        state:"planned".into(), origin:"career_holder_context".into(), owner:None, policy:None,
        worker_code:0, worker_result:0, note:"Unpublished 24-byte native career holder; retain on failure/undo".into(),
    };
    persist(&ledger)?;
    let result = run_worker_args(s.pid,s.created,s.module_base+0x9234B0,s.module_base+0x4E44E68,Some(24),5000,&mut ledger);
    let address = match result { Ok(a)=>a, Err(error)=>{
        if ledger.state!="uncertain_timeout" { ledger.state="failed_before_publish".into(); ledger.note=format!("{error:#}"); let _=persist(&ledger); }
        return Err(error);
    }};
    career_storage_minimum(s)?;
    let writer=Process::open(s.pid,true)?; backend::validate_with(&writer,s)?;
    writer.write(address,&[0;24])?;
    ensure!(writer.read(address,24)?==[0;24], "커리어 목록 초기화 재읽기 실패");
    Ok(ledger.allocation)
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct WorkerPages {
    process: HANDLE,
    code: u64,
    result: u64,
    safe_to_free: bool,
}
impl Drop for WorkerPages {
    fn drop(&mut self) {
        // On timeout or uncertain completion the worker may still be executing.
        // Keep its pages mapped; never terminate a target thread or free live code.
        if self.safe_to_free {
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
fn worker_code(function: u64, arg: u64, result: u64) -> Vec<u8> {
    worker_code_args(function, arg, None, result)
}
fn worker_code_args(function: u64, arg: u64, second: Option<u64>, result: u64) -> Vec<u8> {
    // Windows x64 entry stack: reserve shadow space and restore alignment.
    let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb9];
    code.extend(arg.to_le_bytes());
    if let Some(second) = second { code.extend([0x48, 0xba]); code.extend(second.to_le_bytes()); }
    code.extend([0x48, 0xb8]);
    code.extend(function.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0xa3]);
    code.extend(result.to_le_bytes());
    code.extend([0x48, 0x83, 0xc4, 0x28, 0x31, 0xc0, 0xc3]);
    code
}
fn write_remote(h: HANDLE, at: u64, bytes: &[u8]) -> Result<()> {
    let mut count = 0;
    ensure!(
        unsafe {
            WriteProcessMemory(
                h,
                at as *mut c_void,
                bytes.as_ptr().cast(),
                bytes.len(),
                &mut count,
            )
        } != 0
            && count == bytes.len(),
        "할당 작업자 메모리 준비 실패: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
fn run_worker(
    pid: u32,
    created: u64,
    function: u64,
    arg: u64,
    wait_ms: u32,
    ledger: &mut Ledger,
) -> Result<u64> {
    run_worker_args(pid, created, function, arg, None, wait_ms, ledger)
}
fn run_worker_args(pid: u32, created: u64, function: u64, arg: u64, second: Option<u64>, wait_ms: u32, ledger: &mut Ledger) -> Result<u64> {
    let h = Handle(unsafe {
        OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_READ
                | PROCESS_VM_WRITE,
            0,
            pid,
        )
    });
    ensure!(
        !h.0.is_null(),
        "관계 확장을 위한 FM 접근 실패: {}",
        std::io::Error::last_os_error()
    );
    let p = Process::open(pid, false)?;
    ensure!(
        p.created()? == created,
        "할당 전에 대상 프로세스가 재시작됐습니다"
    );
    let mut pages = WorkerPages {
        process: h.0,
        code: 0,
        result: 0,
        safe_to_free: true,
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
    ensure!(pages.result != 0, "할당 결과 영역 준비 실패");
    pages.code = unsafe {
        VirtualAllocEx(
            h.0,
            std::ptr::null(),
            4096,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    } as u64;
    ensure!(pages.code != 0, "할당 작업자 준비 실패");
    let code = worker_code_args(function, arg, second, pages.result);
    write_remote(h.0, pages.code, &code)?;
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
        "할당 작업자 실행 권한 설정 실패"
    );
    ensure!(
        unsafe { FlushInstructionCache(h.0, pages.code as *const c_void, code.len()) } != 0,
        "할당 작업자 코드 동기화 실패"
    );
    ledger.worker_code = pages.code;
    ledger.worker_result = pages.result;
    ledger.state = "worker_prepared".into();
    persist(ledger)?;
    let start: unsafe extern "system" fn(*mut c_void) -> u32 =
        unsafe { std::mem::transmute(pages.code as usize) };
    let thread = Handle(unsafe {
        CreateRemoteThread(
            h.0,
            std::ptr::null(),
            0,
            Some(start),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
        )
    });
    ensure!(
        !thread.0.is_null(),
        "할당 작업자 시작 실패: {}",
        std::io::Error::last_os_error()
    );
    pages.safe_to_free = false;
    // The journal already has both scratch addresses before a thread can run.
    let waited = unsafe { WaitForSingleObject(thread.0, wait_ms) };
    if waited != WAIT_OBJECT_0 {
        ledger.state = "uncertain_timeout".into();
        ledger.note = format!("worker wait: {waited:#x}; scratch retained; no owner published");
        let _ = persist(ledger);
        bail!(
            "관계 메모리 확보 결과를 확인하지 못했습니다. 게임 관계는 변경하지 않았으며 작업자 메모리는 보존했습니다"
        );
    }
    pages.safe_to_free = true;
    let mut exit_code = u32::MAX;
    ensure!(
        unsafe { GetExitCodeThread(thread.0, &mut exit_code) } != 0 && exit_code == 0,
        "관계 할당 작업자가 정상 종료하지 않았습니다"
    );
    let allocated = p.u64(pages.result)?;
    ensure!(address(allocated), "FM 메모리 할당 실패");
    // Persist the returned allocation before releasing the only remote result
    // cell. A journal failure leaves both scratch pages available for diagnosis.
    ledger.allocation.address = allocated;
    ledger.state = "unpublished".into();
    pages.safe_to_free = false;
    persist(ledger)?;
    pages.safe_to_free = true;
    ledger.worker_code = 0;
    ledger.worker_result = 0;
    Ok(allocated)
}
pub fn allocate(s: &Session, requested: usize) -> Result<NativeAllocation> {
    verify_no_pending(s)?;
    let p = Process::open(s.pid, false)?;
    let before = policy(&p, s, requested)?;
    let a = NativeAllocation {
        id: format!("{}-{}", stamp(), s.pid),
        address: 0,
        len: before.len,
        session: s.clone(),
    };
    let mut ledger = Ledger {
        allocation: a,
        state: "planned".into(),
        origin: "crt_fallback".into(),
        owner: None,
        policy: Some(before.clone()),
        worker_code: 0,
        worker_result: 0,
        note: "Owner not published; allocation retained until process exit if unused".into(),
    };
    persist(&ledger)?;
    let result = run_worker(
        s.pid,
        s.created,
        before.malloc,
        before.len as u64,
        5000,
        &mut ledger,
    );
    let allocated = match result {
        Ok(a) => a,
        Err(e) => {
            if ledger.state != "uncertain_timeout" {
                ledger.state = "failed_before_publish".into();
                ledger.note = format!("{e:#}");
                let _ = persist(&ledger);
            }
            return Err(e);
        }
    };
    ledger.allocation.address = allocated;
    ledger.state = "unpublished".into();
    persist(&ledger)?;
    ensure!(
        policy(&p, s, requested)? == before,
        "할당 도중 FM 관계 할당기가 변경됐습니다. 새 배열은 연결하지 않았습니다"
    );
    // Initialise all bytes while unpublished. This is separate from the short
    // suspended transaction that finally changes the existing owner header.
    let writer = Process::open(s.pid, true)?;
    backend::validate_with(&writer, s)?;
    for offset in (0..before.len).step_by(64) {
        writer.write(
            allocated + offset as u64,
            &vec![0; (before.len - offset).min(64)],
        )?;
    }
    ensure!(
        p.read(allocated, before.len)?.iter().all(|b| *b == 0),
        "새 관계 배열 초기화 검증 실패"
    );
    Ok(ledger.allocation)
}
pub fn adopt_retained(
    s: &Session,
    address: u64,
    len: usize,
    owner: u64,
) -> Result<NativeAllocation> {
    adopt_storage(s,address,len,owner,false)
}
pub fn adopt_career_storage(s: &Session, address: u64, len: usize, owner: u64) -> Result<NativeAllocation> {
    adopt_storage(s,address,len,owner,true)
}
fn adopt_storage(s: &Session,address:u64,len:usize,owner:u64,career:bool) -> Result<NativeAllocation> {
    ensure!(
        self::address(address)
            && self::address(owner)
            && len > 0
            && len <= MAX_BYTES
            && len % (if career {8} else {16}) == 0,
        "보존 관계 배열 범위 오류"
    );
    backend::validate_session(s)?;
    let a = NativeAllocation {
        id: format!("{}-{}", stamp(), s.pid),
        address,
        len,
        session: s.clone(),
    };
    persist(&Ledger {
        allocation: a.clone(),
        state: "retained_original".into(),
        origin: if career {"career_native_retained"} else {"native_original"}.into(),
        owner: Some(owner),
        policy: None,
        worker_code: 0,
        worker_result: 0,
        note: "Original buffer retained by relation transaction; never freed by editor".into(),
    })?;
    Ok(a)
}
pub fn verify_retained(a: &NativeAllocation) -> Result<()> {
    let ledger = read_ledger(a)?;
    ensure!(
        [
            "unpublished",
            "published",
            "detached_retained",
            "retained_original"
        ]
        .contains(&ledger.state.as_str()),
        "할당 소유권을 확인할 수 없습니다: {}",
        ledger.state
    );
    let p = Process::open(a.session.pid, false)?;
    backend::validate_with(&p, &a.session)?;
    ensure!(
        address(a.address) && a.len > 0 && a.len <= MAX_BYTES && (a.len % 16 == 0 || (a.len==24 && ledger.origin=="career_holder_context") || (a.len%8==0 && ledger.origin=="career_native_retained")),
        "할당 범위 오류"
    );
    if ledger.origin=="career_holder_context" { career_storage_minimum(&a.session)?; }
    if let Some(before) = ledger.policy {
        ensure!(
            policy(&p, &a.session, a.len)? == before,
            "관계 배열의 메모리 소유권 조건이 변경됐습니다"
        );
    }
    p.read(a.address, a.len)?;
    Ok(())
}
fn update(a: &NativeAllocation, state: &str, owner: Option<u64>) -> Result<()> {
    let mut ledger = read_ledger(a)?;
    ledger.state = state.into();
    if let Some(owner) = owner {
        ledger.owner = Some(owner);
    }
    persist(&ledger)
}
pub fn mark_published(a: &NativeAllocation, owner: u64) -> Result<()> {
    ensure!(address(owner), "관계 소유자 주소 오류");
    update(a, "published", Some(owner))
}
pub fn mark_detached(a: &NativeAllocation) -> Result<()> {
    update(a, "detached_retained", None)
}
pub fn mark_uncertain(a: &NativeAllocation) -> Result<()> {
    update(a, "uncertain", None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_capacity_stays_outside_native_pool() {
        assert_eq!(sized(32, 1537).unwrap(), 2048);
        assert_eq!(sized(2050, 1537).unwrap(), 2064);
        assert_eq!(sized(32, 4097).unwrap(), 4112);
        assert_eq!(sized(16384, 1537).unwrap(), 16384);
        assert!(sized(16, 16385).is_err());
        assert!(sized(0, 0).is_err());
    }
    #[test]
    fn worker_abi_and_return_store_are_exact() {
        let b = worker_code(0x12345678, 2048, 0x87654321);
        assert_eq!(&b[..4], &[0x48, 0x83, 0xec, 0x28]);
        assert_eq!(u64::from_le_bytes(b[6..14].try_into().unwrap()), 2048);
        assert_eq!(
            u64::from_le_bytes(b[16..24].try_into().unwrap()),
            0x12345678
        );
        assert_eq!(&b[24..28], &[0xff, 0xd0, 0x48, 0xa3]);
        assert_eq!(
            u64::from_le_bytes(b[28..36].try_into().unwrap()),
            0x87654321
        );
        assert_eq!(&b[36..], &[0x48, 0x83, 0xc4, 0x28, 0x31, 0xc0, 0xc3]);
    }

    unsafe extern "system" fn fixture_allocate(size: usize) -> usize {
        let bytes = vec![0xa5u8; size].into_boxed_slice();
        Box::into_raw(bytes) as *mut u8 as usize
    }
    unsafe extern "system" fn fixture_slow(size: usize) -> usize {
        std::thread::sleep(std::time::Duration::from_millis(250));
        unsafe { fixture_allocate(size) }
    }
    unsafe extern "system" fn fixture_context_allocate(context: usize, size: usize) -> usize {
        if context != 0x1234_5678 || size != 24 { return 0; }
        unsafe { fixture_allocate(size) }
    }
    #[test]
    #[ignore = "Private owned-process fixture; invoked only by allocator integration test"]
    fn worker_child() {
        let path =
            std::env::var_os("TOUCHLINE_ALLOC_FIXTURE").expect("owned fixture handshake required");
        let path = PathBuf::from(path);
        std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"allocate":fixture_allocate as *const () as usize,"context_allocate":fixture_context_allocate as *const () as usize,"slow":fixture_slow as *const () as usize})).unwrap()).unwrap();
        for _ in 0..1500 {
            if path.with_extension("stop").exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    struct FixtureChild {
        child: std::process::Child,
        path: PathBuf,
        ledgers: Vec<PathBuf>,
    }
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(self.path.with_extension("stop"));
            for p in &self.ledgers {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    #[test]
    fn worker_runs_only_in_owned_fixture_and_timeout_retains_code() {
        use std::os::windows::process::CommandExt;
        let path = std::env::temp_dir().join(format!("touchline-alloc-fixture-{}.json", stamp()));
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "native_alloc::tests::worker_child",
                "--nocapture",
            ])
            .env("TOUCHLINE_ALLOC_FIXTURE", &path)
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut child = FixtureChild {
            child,
            path,
            ledgers: vec![],
        };
        let ready = std::time::Instant::now();
        while !child.path.exists() && ready.elapsed().as_secs() < 8 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let functions: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&child.path).unwrap()).unwrap();
        let p = Process::open(child.child.id(), false).unwrap();
        let session = Session {
            pid: child.child.id(),
            created: p.created().unwrap(),
            module_base: 0,
            module_size: 0,
            module_stamp: 0,
            db: 0,
            registry_begin: 0,
            game_date: "fixture".into(),
            profile: "owned-test-process".into(),
            own_club_id: None,
        };
        let mut row = Ledger {
            allocation: NativeAllocation {
                id: format!("{}-{}", stamp(), session.pid),
                address: 0,
                len: 2048,
                session: session.clone(),
            },
            state: "planned".into(),
            origin: "test_fixture".into(),
            owner: None,
            policy: None,
            worker_code: 0,
            worker_result: 0,
            note: "Only the spawned test child".into(),
        };
        child.ledgers.push(ledger_path(&row.allocation.id).unwrap());
        let result = run_worker(
            session.pid,
            session.created,
            functions["allocate"].as_u64().unwrap(),
            2048,
            5000,
            &mut row,
        )
        .unwrap();
        assert_eq!(p.read(result, 2048).unwrap(), vec![0xa5; 2048]);
        assert_eq!(row.worker_code, 0);
        assert_eq!(row.worker_result, 0);
        row.state = "test_completed".into();
        persist(&row).unwrap();
        row.allocation.id = format!("{}-{}", stamp(), session.pid);
        child.ledgers.push(ledger_path(&row.allocation.id).unwrap());
        let holder=run_worker_args(session.pid,session.created,functions["context_allocate"].as_u64().unwrap(),0x1234_5678,Some(24),5000,&mut row).unwrap();
        assert_eq!(p.read(holder,24).unwrap(),vec![0xa5;24]);
        row.state="test_completed".into();persist(&row).unwrap();
        row.allocation.id = format!("{}-{}", stamp(), session.pid);
        child.ledgers.push(ledger_path(&row.allocation.id).unwrap());
        assert!(
            run_worker(
                session.pid,
                session.created,
                functions["slow"].as_u64().unwrap(),
                32,
                1,
                &mut row
            )
            .is_err()
        );
        assert_eq!(row.state, "uncertain_timeout");
        assert_eq!(
            &p.read(row.worker_code, 4).unwrap(),
            &[0x48, 0x83, 0xec, 0x28]
        );
        assert!(verify_no_pending(&session).is_err());
        std::thread::sleep(std::time::Duration::from_millis(350));
        let late = p.u64(row.worker_result).unwrap();
        assert_eq!(p.read(late, 32).unwrap(), vec![0xa5; 32]);
        assert!(verify_no_pending(&session).is_err());
    }
}
