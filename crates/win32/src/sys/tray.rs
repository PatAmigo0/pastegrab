//! A tray icon that exists only while it tells about a job that ended in the background. A click
//! on it or its balloon brings the window back.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_APP,
};

use super::shell;
use crate::text::Notice;

/// What the tray icon sends the window.
pub(super) const WM_TRAY: u32 = WM_APP + 3;

pub(super) struct Tray {
    shown: Cell<bool>,
}

impl Tray {
    pub(super) const fn new() -> Self {
        Self {
            shown: Cell::new(false),
        }
    }

    /// Shows `notice` in a balloon unless the window is in front.
    pub(super) fn announce(&self, hwnd: HWND, notice: &Notice) {
        if in_front(hwnd) {
            return;
        }
        let mut data = icon(hwnd);
        data.uFlags |= NIF_INFO;
        copy(&mut data.szInfoTitle, notice.title);
        copy(&mut data.szInfo, &notice.body);
        data.dwInfoFlags = if notice.failed {
            NIIF_WARNING
        } else {
            NIIF_INFO
        };
        if self.shown.replace(true) {
            // SAFETY: `data` describes this window's icon and outlives the call.
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) };
            return;
        }
        let mut version = icon(hwnd);
        version.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: as above.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &raw const data);
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &raw const version);
        }
    }

    /// The icon's own messages; with version 4 the event sits in the low word.
    pub(super) fn message(&self, hwnd: HWND, lparam: LPARAM) {
        match u32::try_from(lparam.0 & 0xFFFF).unwrap_or(0) {
            NIN_BALLOONUSERCLICK | NIN_SELECT => {
                self.remove(hwnd);
                restore(hwnd);
            }
            NIN_BALLOONTIMEOUT => self.remove(hwnd),
            _ => {}
        }
    }

    /// Takes the icon away, once the person is back or the window closes.
    pub(super) fn remove(&self, hwnd: HWND) {
        if self.shown.replace(false) {
            let data = icon(hwnd);
            // SAFETY: as in `announce`.
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
        }
    }
}

fn icon(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: shell::small_icon(),
        ..Default::default()
    };
    copy(&mut data.szTip, "pastegrab");
    data
}

/// Copies `text` with its terminator, cut to fit.
fn copy(field: &mut [u16], text: &str) {
    let room = field.len().saturating_sub(1);
    for (slot, unit) in field.iter_mut().zip(text.encode_utf16().take(room)) {
        *slot = unit;
    }
    let end = text.encode_utf16().count().min(room);
    if let Some(slot) = field.get_mut(end) {
        *slot = 0;
    }
}

fn in_front(hwnd: HWND) -> bool {
    // SAFETY: plain queries about a window of this thread.
    unsafe { GetForegroundWindow() == hwnd && !IsIconic(hwnd).as_bool() }
}

fn restore(hwnd: HWND) {
    // SAFETY: as above; a click on the tray lets this process take the foreground.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
