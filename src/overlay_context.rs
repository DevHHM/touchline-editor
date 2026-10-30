//! Window targeting for the coach overlay. Never changes the FM window.
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, RECT},
    UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Anchor {
    pub const ALL: [Self; 4] = [
        Self::TopRight,
        Self::TopLeft,
        Self::BottomRight,
        Self::BottomLeft,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "왼쪽 위",
            Self::TopRight => "오른쪽 위",
            Self::BottomLeft => "왼쪽 아래",
            Self::BottomRight => "오른쪽 아래",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetWindow {
    pub rect: [i32; 4],
    pub scale: f32,
}

pub fn permits_target(expected_pid: u32, actual_pid: u32, minimized: bool, rect: [i32; 4]) -> bool {
    expected_pid != 0
        && expected_pid == actual_pid
        && !minimized
        && rect[2] - rect[0] >= 600
        && rect[3] - rect[1] >= 420
}

/// Requires foreground ownership by the exact, already-validated FM process.
pub fn foreground_target(pid: u32) -> Option<TargetWindow> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return None;
        }
        let mut owner_pid = 0;
        GetWindowThreadProcessId(foreground, &mut owner_pid);
        if owner_pid != pid {
            return None;
        }
        let root = GetAncestor(foreground, GA_ROOTOWNER);
        let hwnd = if root.is_null() { foreground } else { root };
        GetWindowThreadProcessId(hwnd, &mut owner_pid);
        let mut r: RECT = std::mem::zeroed();
        if IsWindowVisible(hwnd) == 0 || GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        let rect = [r.left, r.top, r.right, r.bottom];
        if !permits_target(pid, owner_pid, IsIconic(hwnd) != 0, rect) {
            return None;
        }
        let scale = (GetDpiForWindow(hwnd).max(96) as f32 / 96.).clamp(1., 4.);
        Some(TargetWindow { rect, scale })
    }
}

/// Physical coordinates: negative desktop coordinates and mixed-DPI monitors work.
pub fn placement(
    target: TargetWindow,
    logical: [f32; 2],
    anchor: Anchor,
    offsets: [f32; 2],
) -> [i32; 4] {
    let r = target.rect;
    let margin = (16. * target.scale).round() as i32;
    let w = ((logical[0] * target.scale).round() as i32)
        .min(r[2] - r[0] - 2 * margin)
        .max(1);
    let h = ((logical[1] * target.scale).round() as i32)
        .min(r[3] - r[1] - 2 * margin)
        .max(1);
    let ox = (offsets[0].max(0.) * target.scale).round() as i32;
    let oy = (offsets[1].max(0.) * target.scale).round() as i32;
    let x = match anchor {
        Anchor::TopLeft | Anchor::BottomLeft => r[0] + margin + ox,
        _ => r[2] - margin - w - ox,
    }
    .clamp(r[0] + margin, r[2] - margin - w);
    let y = match anchor {
        Anchor::TopLeft | Anchor::TopRight => r[1] + margin + oy,
        _ => r[3] - margin - h - oy,
    }
    .clamp(r[1] + margin, r[3] - margin - h);
    [x, y, w, h]
}

struct FindWindow {
    title: String,
    hwnd: HWND,
}
static OVERLAY_HWND: AtomicUsize = AtomicUsize::new(0);
fn marker() -> Vec<u16> {
    "Touchline.CoachOverlay.Owner\0".encode_utf16().collect()
}
fn cookie() -> usize {
    std::process::id() as usize + 0x5400_0000
}
unsafe extern "system" fn find_window(hwnd: HWND, data: LPARAM) -> i32 {
    let f = unsafe { &mut *(data as *mut FindWindow) };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if pid != std::process::id() {
        return 1;
    }
    let mut title = [0u16; 160];
    let mut len = 0;
    let ok = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETTEXT,
            title.len(),
            title.as_mut_ptr() as LPARAM,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            15,
            &mut len,
        )
    };
    if ok == 0 {
        return 1;
    }
    if len > 0 && String::from_utf16_lossy(&title[..len as usize]) == f.title {
        f.hwnd = hwnd;
        return 0;
    }
    1
}

/// Called from the overlay's UI callback only, after eframe creates its HWND.
pub fn cache_overlay(title: &str) {
    if !own_overlay(title).is_null() {
        return;
    }
    let mut find = FindWindow {
        title: title.into(),
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(find_window), &mut find as *mut FindWindow as LPARAM);
    }
    if find.hwnd.is_null() {
        return;
    }
    unsafe {
        let style = GetWindowLongPtrW(find.hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            find.hwnd,
            GWL_EXSTYLE,
            style
                | WS_EX_NOACTIVATE as isize
                | WS_EX_TOOLWINDOW as isize
                | WS_EX_TRANSPARENT as isize,
        );
        if SetPropW(find.hwnd, marker().as_ptr(), cookie() as _) != 0 {
            OVERLAY_HWND.store(find.hwnd as usize, Ordering::Release);
        }
    }
}
fn own_overlay(_title: &str) -> HWND {
    let hwnd = OVERLAY_HWND.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return hwnd;
    }
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id()
            && IsWindow(hwnd) != 0
            && GetPropW(hwnd, marker().as_ptr()) as usize == cookie()
        {
            return hwnd;
        }
    }
    OVERLAY_HWND.store(0, Ordering::Release);
    std::ptr::null_mut()
}

pub fn foreground_id() -> usize {
    unsafe { GetForegroundWindow() as usize }
}
pub fn foreground_pid() -> u32 {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
    }
    pid
}

struct GameWindows {
    pid: u32,
    rows: Vec<serde_json::Value>,
}
unsafe extern "system" fn inspect_game_window(hwnd: HWND, data: LPARAM) -> i32 {
    let query = unsafe { &mut *(data as *mut GameWindows) };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if pid == query.pid {
        let mut r: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            if GetWindowRect(hwnd, &mut r) != 0 && r.right - r.left >= 600 {
                query.rows.push(serde_json::json!({ "hwnd": hwnd as usize,
                    "bounds": [r.left, r.top, r.right, r.bottom], "dpi": GetDpiForWindow(hwnd),
                    "visible": IsWindowVisible(hwnd) != 0, "minimized": IsIconic(hwnd) != 0 }));
            }
        }
    }
    1
}
fn game_windows(pid: u32) -> Vec<serde_json::Value> {
    let mut query = GameWindows {
        pid,
        rows: Vec::new(),
    };
    unsafe {
        EnumWindows(
            Some(inspect_game_window),
            &mut query as *mut GameWindows as LPARAM,
        );
    }
    query.rows
}

pub fn diagnostic(title: &str, fm_pid: u32) -> serde_json::Value {
    let hwnd = own_overlay(title);
    let target = foreground_target(fm_pid);
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        let style = if hwnd.is_null() {
            0
        } else {
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE)
        };
        if !hwnd.is_null() {
            GetWindowRect(hwnd, &mut r);
        }
        serde_json::json!({
            "overlay_exists": !hwnd.is_null(), "overlay_hwnd": hwnd as usize,
            "foreground_hwnd": foreground_id(), "overlay_has_focus": !hwnd.is_null() && GetForegroundWindow() == hwnd,
            "foreground_pid": foreground_pid(),
            "no_activate": style & WS_EX_NOACTIVATE as isize != 0,
            "click_through": style & WS_EX_TRANSPARENT as isize != 0,
            "tool_window": style & WS_EX_TOOLWINDOW as isize != 0,
            "topmost": style & WS_EX_TOPMOST as isize != 0,
            "visible": !hwnd.is_null() && IsWindowVisible(hwnd) != 0,
            "overlay_bounds": [r.left, r.top, r.right, r.bottom],
            "fm_is_foreground": target.is_some(),
            "fm_foreground_bounds": target.map(|t| t.rect),
            "fm_dpi_scale": target.map(|t| t.scale),
            "fm_windows_read_only": game_windows(fm_pid),
        })
    }
}

/// All writes below address our own overlay HWND; no FM input, styles or focus calls.
pub fn position_overlay(
    title: &str,
    target: Option<TargetWindow>,
    logical: [f32; 2],
    anchor: Anchor,
    offsets: [f32; 2],
) {
    let hwnd = own_overlay(title);
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if let Some(target) = target {
            let [x, y, w, h] = placement(target, logical, anchor, offsets);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_ASYNCWINDOWPOS,
            );
        } else {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_HIDEWINDOW
                    | SWP_ASYNCWINDOWPOS,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_foreground_game_process_and_usable_window() {
        assert!(permits_target(41, 41, false, [0, 0, 1920, 1080]));
        assert!(!permits_target(41, 42, false, [0, 0, 1920, 1080]));
        assert!(!permits_target(41, 41, true, [0, 0, 1920, 1080]));
        assert!(!permits_target(41, 41, false, [0, 0, 400, 250]));
        assert!(!permits_target(0, 0, false, [0, 0, 1920, 1080]));
    }
    #[test]
    fn target_monitor_dpi_and_negative_coordinates_are_preserved() {
        let t = TargetWindow {
            rect: [-2560, -1440, 0, 0],
            scale: 1.5,
        };
        assert_eq!(
            placement(t, [440., 520.], Anchor::TopRight, [8., 48.]),
            [-696, -1344, 660, 780]
        );
        let [x, y, w, h] = placement(t, [440., 520.], Anchor::BottomLeft, [9000., 9000.]);
        assert!(x >= -2536 && y >= -1416 && x + w <= -24 && y + h <= -24);
    }
}
