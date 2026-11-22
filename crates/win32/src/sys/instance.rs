use std::os::windows::io::{FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
};
use windows::core::{PCWSTR, w};

use super::window::CLASS;

/// Tags the text a second start hands to the running window.
pub(super) const FORWARD: usize = 0x7067;

/// Marks the running window; another start finds it while this lives.
#[derive(Debug)]
pub struct Instance {
    _mutex: Option<OwnedHandle>,
}

/// Claims the one window per user. When another already runs, it gets `text`, comes to the
/// front, and the result is `None`.
#[must_use]
pub fn claim(text: &str) -> Option<Instance> {
    // SAFETY: a named mutex in this session; `OwnedHandle` closes it once.
    let (mutex, existed) = unsafe {
        match CreateMutexW(None, false, w!("Local\\pastegrab-window")) {
            Ok(handle) => (
                Some(OwnedHandle::from_raw_handle(handle.0)),
                GetLastError() == ERROR_ALREADY_EXISTS,
            ),
            Err(_) => (None, false),
        }
    };
    if !existed {
        return Some(Instance { _mutex: mutex });
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    let data = COPYDATASTRUCT {
        dwData: FORWARD,
        cbData: u32::try_from(units.len() * 2).unwrap_or(0),
        lpData: units.as_ptr().cast_mut().cast(),
    };
    // SAFETY: `data` and `units` outlive the synchronous WM_COPYDATA, which copies them across.
    unsafe {
        if let Ok(window) = FindWindowW(CLASS, PCWSTR::null()) {
            if IsIconic(window).as_bool() {
                let _ = ShowWindow(window, SW_RESTORE);
            }
            let _ = SetForegroundWindow(window);
            SendMessageW(
                window,
                WM_COPYDATA,
                Some(WPARAM(0)),
                Some(LPARAM(std::ptr::from_ref(&data) as isize)),
            );
        }
    }
    None
}
