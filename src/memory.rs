//! Bounded native Windows memory access. Read-only unless the caller explicitly opens a writer.
mod quiescence;
use anyhow::{Result, anyhow, bail, ensure};
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::{
            Debug::{ReadProcessMemory, WriteProcessMemory},
            ToolHelp::*,
        },
        Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SUSPEND_RESUME,
            PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
        },
    },
};

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub base: u64,
    pub size: u32,
    pub path: String,
}
pub struct Process {
    pub pid: u32,
    handle: HANDLE,
    writable: bool,
}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
struct Snapshot(HANDLE);
impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}
fn os_error(context: &str) -> anyhow::Error {
    anyhow!("{}: {}", context, std::io::Error::last_os_error())
}

pub fn processes() -> Result<Vec<(u32, String)>> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    ensure!(
        raw != INVALID_HANDLE_VALUE,
        "프로세스 목록을 읽을 수 없습니다"
    );
    let snap = Snapshot(raw);
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut out = Vec::new();
    let mut ok = unsafe { Process32FirstW(snap.0, &mut entry) };
    while ok != 0 {
        out.push((entry.th32ProcessID, wide(&entry.szExeFile)));
        ok = unsafe { Process32NextW(snap.0, &mut entry) };
    }
    Ok(out)
}
impl Process {
    pub fn open(pid: u32, writable: bool) -> Result<Self> {
        let mut rights = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;
        if writable {
            rights |= PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_SUSPEND_RESUME;
        }
        let handle = unsafe { OpenProcess(rights, 0, pid) };
        if handle.is_null() {
            return Err(os_error("FM 프로세스 접근 실패"));
        }
        Ok(Self {
            pid,
            handle,
            writable,
        })
    }
    pub fn read(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        ensure!(
            address >= 0x10000
                && address <= 0x7fff_ffff_ffff
                && size <= 16_000_000
                && address.checked_add(size as u64).is_some(),
            "유효하지 않은 읽기 범위"
        );
        if size == 0 {
            return Ok(Vec::new());
        }
        let mut bytes = vec![0; size];
        let mut done = 0;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                bytes.as_mut_ptr().cast(),
                size,
                &mut done,
            )
        };
        if ok == 0 || done != size {
            bail!(
                "메모리 읽기 실패 0x{address:X} ({done}/{size}): {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(bytes)
    }
    pub fn write(&self, address: u64, bytes: &[u8]) -> Result<()> {
        ensure!(self.writable, "읽기 전용 연결입니다");
        ensure!(
            address >= 0x10000 && !bytes.is_empty() && bytes.len() <= 64,
            "편집 쓰기 범위가 허용되지 않습니다"
        );
        let mut done = 0;
        let ok = unsafe {
            WriteProcessMemory(
                self.handle,
                address as *mut c_void,
                bytes.as_ptr().cast(),
                bytes.len(),
                &mut done,
            )
        };
        if ok == 0 || done != bytes.len() {
            return Err(os_error("메모리 쓰기 실패"));
        }
        Ok(())
    }
    pub fn u8(&self, a: u64) -> Result<u8> {
        Ok(self.read(a, 1)?[0])
    }
    pub fn u16(&self, a: u64) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read(a, 2)?.try_into().unwrap()))
    }
    pub fn u32(&self, a: u64) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read(a, 4)?.try_into().unwrap()))
    }
    pub fn i32(&self, a: u64) -> Result<i32> {
        Ok(i32::from_le_bytes(self.read(a, 4)?.try_into().unwrap()))
    }
    pub fn u64(&self, a: u64) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read(a, 8)?.try_into().unwrap()))
    }
    pub fn i64(&self, a: u64) -> Result<i64> {
        Ok(i64::from_le_bytes(self.read(a, 8)?.try_into().unwrap()))
    }
    pub fn f32(&self, a: u64) -> Result<f32> {
        Ok(f32::from_le_bytes(self.read(a, 4)?.try_into().unwrap()))
    }
    pub fn f64(&self, a: u64) -> Result<f64> {
        Ok(f64::from_le_bytes(self.read(a, 8)?.try_into().unwrap()))
    }
    pub fn ptr(&self, a: u64) -> u64 {
        self.u64(a)
            .ok()
            .filter(|&p| p >= 0x10000 && p < 0x7fff_ffff_ffff)
            .unwrap_or(0)
    }
    pub fn string(&self, a: u64, max: usize) -> String {
        if a < 0x10000 {
            return String::new();
        }
        let bytes = self.read(a, max.min(1024)).or_else(|_| self.read(a, 32));
        bytes
            .ok()
            .and_then(|v| {
                String::from_utf8(v[..v.iter().position(|&b| b == 0).unwrap_or(v.len())].to_vec())
                    .ok()
            })
            .unwrap_or_default()
            .trim()
            .to_string()
    }
    pub fn created(&self) -> Result<u64> {
        let mut c: FILETIME = unsafe { zeroed() };
        let mut e = c;
        let mut k = c;
        let mut u = c;
        if unsafe { GetProcessTimes(self.handle, &mut c, &mut e, &mut k, &mut u) } == 0 {
            return Err(os_error("프로세스 세션 확인 실패"));
        }
        Ok(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
    }
    pub fn modules(&self) -> Result<Vec<Module>> {
        let raw =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.pid) };
        ensure!(raw != INVALID_HANDLE_VALUE, "FM 모듈을 확인할 수 없습니다");
        let snap = Snapshot(raw);
        let mut e: MODULEENTRY32W = unsafe { zeroed() };
        e.dwSize = size_of::<MODULEENTRY32W>() as u32;
        let mut rows = Vec::new();
        let mut ok = unsafe { Module32FirstW(snap.0, &mut e) };
        while ok != 0 {
            rows.push(Module {
                name: wide(&e.szModule),
                path: wide(&e.szExePath),
                base: e.modBaseAddr as u64,
                size: e.modBaseSize,
            });
            ok = unsafe { Module32NextW(snap.0, &mut e) };
        }
        Ok(rows)
    }
    /// Transaction-only pause. No caller in the read/index path invokes this method.
    pub fn suspend(&self) -> Result<SuspendGuard<'_>> {
        ensure!(self.writable, "읽기 연결에서는 일시 정지할 수 없습니다");
        let status = unsafe { NtSuspendProcess(self.handle) };
        ensure!(
            status >= 0,
            "안전한 편집을 위한 짧은 일시 정지 실패: 0x{:X}",
            status
        );
        Ok(SuspendGuard {
            process: self,
            active: true,
        })
    }
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> i32;
    fn NtResumeProcess(process: HANDLE) -> i32;
}
pub struct SuspendGuard<'a> {
    process: &'a Process,
    active: bool,
}
impl SuspendGuard<'_> {
    /// Only valid during this process-wide suspension. Conservatively rejects
    /// in-flight calls, including callees whose return addresses remain on stack.
    pub fn assert_quiescent(&self, ranges: &[(u64, u64)]) -> Result<()> {
        ensure!(self.active, "스레드 정지 상태를 확인할 수 없습니다");
        quiescence::assert_quiescent(self.process, ranges)
    }
    pub fn resume(mut self) -> Result<()> {
        let status = unsafe { NtResumeProcess(self.process.handle) };
        ensure!(status >= 0, "FM 재개 실패: 0x{:X}", status);
        self.active = false;
        Ok(())
    }
}
impl Drop for SuspendGuard<'_> {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                NtResumeProcess(self.process.handle);
            }
        }
    }
}
