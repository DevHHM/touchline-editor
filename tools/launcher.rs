//! A small native entry point for the portable package. No .NET or web runtime
//! is needed until the actual WPF app in app/ starts.
#![windows_subsystem = "windows"]

use std::{env, ffi::OsString, path::Path, process::{self, Command}};

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(owner: *mut std::ffi::c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
}

const REQUIRED: &[&str] = &[
    "app/Touchline Editor.exe", "app/Touchline Editor.dll",
    "app/Touchline Editor.runtimeconfig.json", "app/Touchline Editor.deps.json",
    "app/coreclr.dll", "app/hostfxr.dll", "app/hostpolicy.dll",
    "app/PresentationFramework.dll", "app/Wpf.Ui.dll", "engine/Touchline Engine.exe",
];

fn show_error(message: &str) {
    let title: Vec<u16> = "Touchline Editor\0".encode_utf16().collect();
    let text: Vec<u16> = format!("{message}\0").encode_utf16().collect();
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10); }
}

fn missing(root: &Path) -> Vec<&'static str> {
    REQUIRED.iter().copied().filter(|p| !root.join(p).is_file()).collect()
}

fn run() -> Result<i32, String> {
    let executable = env::current_exe().map_err(|e| e.to_string())?;
    let root = executable.parent().ok_or("실행 파일의 폴더를 찾을 수 없습니다.")?;
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let absent = missing(root);
    if args.len() == 1 && args[0] == "--launcher-check" {
        let files = absent.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(",");
        println!("{{\"ok\":{},\"layout\":\"app-engine-v1\",\"missing\":[{}]}}", absent.is_empty(), files);
        return Ok(if absent.is_empty() { 0 } else { 2 });
    }
    if !absent.is_empty() {
        return Err(format!("배포 파일이 누락되었습니다. ZIP 전체를 새 폴더에 압축 해제하세요.\n\n{}", absent.join("\n")));
    }
    // Preserve the caller's working directory so relative --snapshot/--ui-check
    // paths remain correct. Executable and engine resolution never depend on it.
    let status = Command::new(root.join("app/Touchline Editor.exe"))
        .args(args).status()
        .map_err(|e| format!("Touchline 화면을 시작하지 못했습니다.\n\n{e}"))?;
    Ok(status.code().unwrap_or(1))
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(error) => { show_error(&error); process::exit(1); }
    }
}
