//! The two Windows calls the launcher makes.

#![allow(unsafe_code)]

use pastegrab_i18n::RU;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HSTRING, PCWSTR, w};

/// Whether this user's pastegrab window is open; the class name is the window's own.
pub(crate) fn window_open() -> bool {
    // SAFETY: a lookup by class name; the handle is not kept.
    unsafe { FindWindowW(w!("pastegrab"), PCWSTR::null()) }.is_ok()
}

pub(crate) fn alert(problem: &str) {
    let text = HSTRING::from(format!("{}\n\n{problem}", RU.cannot_start));
    let title = HSTRING::from(RU.window_title);
    // SAFETY: both strings outlive the call.
    unsafe { MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR) };
}
