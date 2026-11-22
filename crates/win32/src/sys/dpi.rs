//! Per-monitor DPI on Windows 8.1 through `shcore.dll` loaded at runtime, and system DPI on
//! Windows 8.0 where that DLL does not exist.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetDC, GetDeviceCaps, HMONITOR, LOGPIXELSY, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
    MonitorFromPoint, MonitorFromWindow, ReleaseDC,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;
use windows::core::{s, w};

const PROCESS_PER_MONITOR_DPI_AWARE: i32 = 2;
const MDT_EFFECTIVE_DPI: i32 = 0;

type SetProcessDpiAwarenessFn = unsafe extern "system" fn(i32) -> i32;
type GetDpiForMonitorFn = unsafe extern "system" fn(HMONITOR, i32, *mut u32, *mut u32) -> i32;

struct Shcore {
    set_awareness: Option<SetProcessDpiAwarenessFn>,
    get_monitor_dpi: Option<GetDpiForMonitorFn>,
}

static SHCORE: OnceLock<Shcore> = OnceLock::new();

fn shcore() -> &'static Shcore {
    SHCORE.get_or_init(|| {
        // SAFETY: `shcore.dll` is loaded from System32 and kept for the life of the process so
        // its function pointers stay valid.
        unsafe {
            let Ok(module) = LoadLibraryExW(w!("shcore.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            else {
                return Shcore {
                    set_awareness: None,
                    get_monitor_dpi: None,
                };
            };
            let set_awareness = GetProcAddress(module, s!("SetProcessDpiAwareness"))
                .map(|p| std::mem::transmute::<_, SetProcessDpiAwarenessFn>(p));
            let get_monitor_dpi = GetProcAddress(module, s!("GetDpiForMonitor"))
                .map(|p| std::mem::transmute::<_, GetDpiForMonitorFn>(p));
            Shcore {
                set_awareness,
                get_monitor_dpi,
            }
        }
    })
}

/// Turns on per-monitor DPI awareness on Windows 8.1 and system DPI awareness on Windows 8.0.
pub fn enable() {
    if let Some(set) = shcore().set_awareness {
        // SAFETY: plain integer argument to Windows' `SetProcessDpiAwareness`.
        let hr = unsafe { set(PROCESS_PER_MONITOR_DPI_AWARE) };
        if hr >= 0 {
            return;
        }
    }
    // SAFETY: plain call; falls back to system DPI when `shcore.dll` is absent.
    let _ = unsafe { SetProcessDPIAware() };
}

/// Effective DPI of the primary monitor.
pub(super) fn for_primary() -> i32 {
    // SAFETY: plain query for the primary monitor.
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    for_monitor(monitor).unwrap_or_else(system_dpi)
}

/// Effective DPI of the monitor holding `hwnd`.
pub(super) fn for_window(hwnd: HWND) -> i32 {
    // SAFETY: `hwnd` belongs to this thread.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    for_monitor(monitor).unwrap_or_else(system_dpi)
}

fn for_monitor(monitor: HMONITOR) -> Option<i32> {
    let get = shcore().get_monitor_dpi?;
    let (mut x, mut y) = (0_u32, 0_u32);
    // SAFETY: `x` and `y` are writable and outlive the call.
    let hr = unsafe { get(monitor, MDT_EFFECTIVE_DPI, &raw mut x, &raw mut y) };
    if hr >= 0 && y > 0 {
        i32::try_from(y).ok()
    } else {
        None
    }
}

fn system_dpi() -> i32 {
    // SAFETY: the screen's device context is released right after the query.
    unsafe {
        let dc = GetDC(None);
        let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY);
        ReleaseDC(None, dc);
        if dpi > 0 { dpi } else { 96 }
    }
}

/// Scales a 96-DPI pixel measurement to `dpi`.
pub(super) const fn scale(value: i32, dpi: i32) -> i32 {
    value * dpi / 96
}
