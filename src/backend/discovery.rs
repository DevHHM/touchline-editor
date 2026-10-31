//! Process candidates are discovery hints, never permission to use a layout.
//! Alternate executable names require both an FM installation path and its
//! loaded native module. No game objects are read during discovery.
use crate::memory::{self, Process};
use anyhow::Result;

pub(crate) const LOCAL_GAME_GUIDANCE: &str = "이 PC에서 실행 중인 FM과 세이브 로드를 기다립니다. PC Game Pass/Microsoft Store판은 빌드별 검증이 필요하며, FM Console·Xbox 콘솔·클라우드 플레이는 지원하지 않습니다";

fn fm_install_path(path: &str) -> bool {
    // Components avoid matching unrelated filenames or e.g. a browser download
    // named Football Manager 27.txt. These are conservative discovery hints;
    // an arbitrary custom installation directory may still need fm.exe.
    let components = path.split(['\\', '/']).collect::<Vec<_>>();
    components[..components.len().saturating_sub(1)].iter().any(|part| {
        let part = part.to_ascii_lowercase();
        matches!(part.as_str(), "football manager 26" | "football manager 27")
    })
}

/// Path-based distribution hint only; it does not identify a build or prove
/// which store supplied it. Keep user-specific full paths out of diagnostics.
pub(super) fn module_context(path: &str) -> &'static str {
    if path.split(['\\', '/']).any(|part|
        part.eq_ignore_ascii_case("XboxGames") || part.eq_ignore_ascii_case("WindowsApps")) {
        "game_plugin.dll (XboxGames/WindowsApps 설치 경로; PC Game Pass/Microsoft Store 빌드 여부 확인 필요)"
    } else {
        "game_plugin.dll"
    }
}

fn image_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    }};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return None; }
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        (ok != 0).then(|| String::from_utf16_lossy(&path[..size as usize]))
    }
}

fn candidates_with(
    processes: Vec<(u32, String)>,
    mut path_for: impl FnMut(u32) -> Option<String>,
    mut has_game_module: impl FnMut(u32) -> bool,
) -> Vec<(u32, String)> {
    processes.into_iter().filter(|(pid, name)| {
        // Preserve the known executable even if metadata access is denied, so
        // connect/probe can report that error instead of claiming FM is absent.
        name.eq_ignore_ascii_case("fm.exe") || (name.to_ascii_lowercase().ends_with(".exe")
            && path_for(*pid).is_some_and(|path| fm_install_path(&path))
            && has_game_module(*pid))
    }).collect()
}

pub(super) fn candidates() -> Result<Vec<(u32, String)>> {
    Ok(candidates_with(memory::processes()?, image_path, |pid| {
        Process::open(pid, false).and_then(|process| process.modules())
            .is_ok_and(|modules| modules.iter().any(|module| module.name.eq_ignore_ascii_case("game_plugin.dll")))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_name_survives_inaccessible_metadata() {
        let candidates = candidates_with(vec![(1, "FM.EXE".into())],
            |_| panic!("known name must not require path access"),
            |_| panic!("known name must not require module access"));
        assert_eq!(candidates, vec![(1, "FM.EXE".into())]);
    }

    #[test]
    fn alternate_name_requires_fm_path_and_loaded_native_module() {
        let candidates = candidates_with(vec![(1, "desktop.exe".into()), (2, "launcher.exe".into()),
            (3, "other.exe".into()), (4, "alternate.exe".into())],
            |pid| Some(match pid {
                1 => r"D:\XboxGames\Football Manager 27\Content\desktop.exe",
                2 => r"D:\XboxGames\Football Manager 27\Content\launcher.exe",
                3 => r"D:\XboxGames\Other Game\other.exe",
                4 => r"E:\Games\Football Manager 26\alternate.exe",
                _ => unreachable!(),
            }.into()),
            |pid| { assert_ne!(pid, 3, "unrelated games must not have modules inspected"); pid != 2 });
        assert_eq!(candidates.iter().map(|(pid, _)| *pid).collect::<Vec<_>>(), vec![1, 4]);
    }

    #[test]
    fn discovery_hints_do_not_match_lookalike_paths_or_cloud_clients() {
        for path in [r"C:\Games\Football Manager 270\game.exe", r"C:\Tools\Football Manager 27.exe",
            r"C:\Games\Football Manager 27 Backup\game.exe", r"C:\Program Files\Xbox\XboxPcApp.exe",
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"] {
            assert!(!fm_install_path(path), "{path}");
        }
        assert!(fm_install_path(r"D:\FOOTBALL MANAGER 26\Content\game.exe"));
        assert_eq!(module_context(r"C:\Users\PrivateName\game_plugin.dll"), "game_plugin.dll");
        assert!(module_context(r"D:\xboxgames\Football Manager 27\game_plugin.dll").contains("확인 필요"));
        assert_eq!(module_context(r"D:\MyXboxGamesBackup\game_plugin.dll"), "game_plugin.dll");
    }
}
