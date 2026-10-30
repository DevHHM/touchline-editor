//! Conservative exclusion of native allocator work while a process is frozen.
//! Stack scanning is intentionally stricter than unwinding: a stale matching
//! word rejects an attempt instead of overlooking an active called function.
use super::*;
use windows_sys::Win32::System::{
    Diagnostics::Debug::{CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext},
    Threading::{OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME},
};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES};

#[repr(C)]
struct ThreadBasicInformation {
    exit_status: i32,
    teb: usize,
    process_id: usize,
    thread_id: usize,
    affinity: usize,
    priority: i32,
    base_priority: i32,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationThread(thread: HANDLE, class: u32, information: *mut c_void, len: u32, returned: *mut u32) -> i32;
}
struct Thread(HANDLE,bool);
impl Drop for Thread { fn drop(&mut self) { unsafe { if self.1 {ResumeThread(self.0);} CloseHandle(self.0); } } }
impl Thread {
    fn release_extra_suspend(&mut self)->Result<()> {
        if self.1 {
            let previous=unsafe{ResumeThread(self.0)};
            ensure!(previous!=u32::MAX,"FM 스레드의 임시 정지 해제 실패; 커리어 추가를 중단했습니다");
            self.1=false;
            ensure!(previous>=2,"FM 스레드 정지 상태가 외부에서 변경되었습니다");
        }
        Ok(())
    }
}

fn thread_ids(pid:u32)->Result<Vec<u32>> {
    let raw=unsafe{CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0)};
    ensure!(raw!=INVALID_HANDLE_VALUE,"FM 스레드 목록 확인 실패");
    let snap=Snapshot(raw); let mut entry:THREADENTRY32=unsafe{zeroed()};entry.dwSize=size_of::<THREADENTRY32>() as u32;
    let mut ids=vec![];let mut ok=unsafe{Thread32First(snap.0,&mut entry)};
    ensure!(ok!=0,"FM 스레드 목록이 비어 있습니다");
    while ok!=0 { if entry.th32OwnerProcessID==pid {ids.push(entry.th32ThreadID);} ok=unsafe{Thread32Next(snap.0,&mut entry)}; }
    ensure!(unsafe{GetLastError()}==ERROR_NO_MORE_FILES,"FM 스레드 목록을 끝까지 읽지 못했습니다");
    ensure!(!ids.is_empty()&&ids.len()<=4096,"FM 스레드 개수가 잘못되었습니다");ids.sort_unstable();Ok(ids)
}
fn blocked(address:u64,ranges:&[(u64,u64)])->bool {ranges.iter().any(|(start,end)|address>=*start&&address<*end)}
fn check_stack(rip:u64,stack:&[u8],ranges:&[(u64,u64)])->Result<()> {
    ensure!(!blocked(rip,ranges),"FM이 커리어 데이터를 처리 중입니다. 잠시 후 다시 시도하세요");
    ensure!(!stack.chunks_exact(8).any(|p|blocked(u64::from_le_bytes(p.try_into().unwrap()),ranges)),"FM 커리어 처리 호출이 아직 끝나지 않았습니다. 잠시 후 다시 시도하세요");
    Ok(())
}
pub(super) fn assert_quiescent(process:&Process,ranges:&[(u64,u64)])->Result<()> {
    ensure!(!ranges.is_empty()&&ranges.iter().all(|(a,b)|*a>=0x10000&&a<b),"검증할 코드 범위가 잘못되었습니다");
    let ids=thread_ids(process.pid)?;
    // Retain every handle until the second inventory has been checked.
    let mut handles=Vec::with_capacity(ids.len());
    for id in &ids {
        let raw=unsafe{OpenThread(THREAD_GET_CONTEXT|THREAD_QUERY_INFORMATION|THREAD_SUSPEND_RESUME,0,*id)};
        ensure!(!raw.is_null(),"FM 스레드 {id} 접근 실패; 커리어 추가를 중단했습니다");
        let mut thread=Thread(raw,false);
        let prior=unsafe{SuspendThread(thread.0)};
        ensure!(prior!=u32::MAX,"FM 스레드 {id} 정지 확인 실패");thread.1=true;
        ensure!(prior>0,"FM에 새 스레드가 생겼습니다. 잠시 후 다시 시도하세요");
        let mut context:CONTEXT=unsafe{zeroed()};context.ContextFlags=CONTEXT_CONTROL_AMD64;
        ensure!(unsafe{GetThreadContext(thread.0,&mut context)}!=0,"FM 스레드 {id} 실행 위치 확인 실패");
        ensure!(context.Rip>=0x10000&&context.Rip<0x7fff_ffff_ffff,"FM 스레드 {id} 실행 위치가 잘못되었습니다");
        ensure!(!blocked(context.Rip,ranges),"FM이 커리어 데이터를 처리 중입니다. 잠시 후 다시 시도하세요");
        let mut info:ThreadBasicInformation=unsafe{zeroed()};let mut returned=0;
        let status=unsafe{NtQueryInformationThread(thread.0,0,(&mut info as *mut ThreadBasicInformation).cast(),size_of::<ThreadBasicInformation>() as u32,&mut returned)};
        ensure!(status>=0&&returned as usize==size_of::<ThreadBasicInformation>()&&info.process_id==process.pid as usize&&info.thread_id==*id as usize&&info.teb>=0x10000,"FM 스레드 {id} 소유권 확인 실패");
        let limits=process.read(info.teb as u64+8,16)?;
        let top=u64::from_le_bytes(limits[0..8].try_into().unwrap());let bottom=u64::from_le_bytes(limits[8..16].try_into().unwrap());
        let sp=context.Rsp;
        ensure!(bottom>=0x10000&&bottom<=sp&&sp<=top&&sp%8==0&&top%8==0&&top-sp<=8*1024*1024,"FM 스레드 {id} 스택 경계를 검증할 수 없습니다");
        let mut at=sp;
        while at<top {let len=(top-at).min(65536) as usize;let stack=process.read(at,len)?;check_stack(context.Rip,&stack,ranges)?;at+=len as u64;}
        handles.push(thread);
    }
    ensure!(thread_ids(process.pid)?==ids,"FM 스레드가 변경되었습니다. 잠시 후 다시 시도하세요");
    for thread in &mut handles {thread.release_extra_suspend()?;}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_current_instruction_and_called_frames_with_half_open_boundaries() {
        let ranges=[(0x10000,0x11000),(0x20000,0x20020)];
        assert!(check_stack(0x10000,&[],&ranges).is_err());
        assert!(check_stack(0x11000,&0x20010u64.to_le_bytes(),&ranges).is_err());
        assert!(check_stack(0x11000,&0x20020u64.to_le_bytes(),&ranges).is_ok());
        assert!(check_stack(0x90000,&[0;64],&ranges).is_ok());
    }
    #[inline(never)]
    fn fixture_wait(path:&std::path::Path) {
        for _ in 0..1000 {
            if path.with_extension("stop").exists() {return;}
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    #[test]
    #[ignore = "Owned process fixture invoked by the quiescence integration test"]
    fn frozen_child() {
        let path=std::path::PathBuf::from(std::env::var_os("TOUCHLINE_QUIESCENCE_FIXTURE").expect("owned fixture handshake required"));
        std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"active":fixture_wait as *const () as usize})).unwrap()).unwrap();
        fixture_wait(&path);
    }
    #[test]
    fn suspended_owned_process_rejects_active_callee_and_resumes_without_writes() {
        use std::os::windows::process::CommandExt;
        struct Child {process:std::process::Child,path:std::path::PathBuf}
        impl Drop for Child {fn drop(&mut self){let _=self.process.kill();let _=self.process.wait();let _=std::fs::remove_file(&self.path);}}
        let path=std::env::temp_dir().join(format!("touchline-quiescence-{}-{}.json",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored","--exact","memory::quiescence::tests::frozen_child","--nocapture"])
            .env("TOUCHLINE_QUIESCENCE_FIXTURE",&path).creation_flags(0x08000000)
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let child=Child{process:child,path};let start=std::time::Instant::now();
        while !child.path.exists()&&start.elapsed().as_secs()<8 {std::thread::sleep(std::time::Duration::from_millis(20));}
        let info:serde_json::Value=serde_json::from_slice(&std::fs::read(&child.path).unwrap()).unwrap();
        let p=Process::open(child.process.id(),true).unwrap();
        let suspended=p.suspend().unwrap();
        suspended.assert_quiescent(&[(0x10000,0x10001)]).unwrap();
        let active=info["active"].as_u64().unwrap();
        assert!(suspended.assert_quiescent(&[(active,active+4096)]).is_err());
        suspended.resume().unwrap();
        // A second suspension proves temporary per-thread counts were balanced.
        p.suspend().unwrap().resume().unwrap();
    }
}
