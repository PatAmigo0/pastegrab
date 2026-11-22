//! The system tray icon: appears for background job balloons and while the window is parked in
//! the tray. A left click restores the window; a right click opens its menu.

use std::cell::Cell;

use pastegrab_i18n::RU;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetForegroundWindow, IsIconic,
    IsWindowVisible, MF_SEPARATOR, MF_STRING, PostMessageW, RegisterWindowMessageW, SW_HIDE,
    SW_RESTORE, SW_SHOW, SetForegroundWindow, SetMenuDefaultItem, ShowWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP,
};
use windows::core::{HSTRING, w};

use super::shell;
use crate::text::Notice;

/// What the tray icon sends the window.
pub(super) const WM_TRAY: u32 = WM_APP + 3;

const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const MENU_OPEN: usize = 1;
const MENU_EXIT: usize = 2;

/// Action requested from the tray menu that the main window must carry out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TrayAction {
    None,
    Exit,
}

pub(super) struct Tray {
    created: u32,
    shown: Cell<bool>,
    pinned: Cell<bool>,
}

impl Tray {
    pub(super) fn new() -> Self {
        // SAFETY: registers a constant system broadcast message name.
        let created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        Self {
            created,
            shown: Cell::new(false),
            pinned: Cell::new(false),
        }
    }

    /// Broadcast Explorer sends after restarting its shell taskbar.
    pub(super) const fn created(&self) -> u32 {
        self.created
    }

    /// Hides the window into the system tray.
    pub(super) fn park(&self, hwnd: HWND) {
        self.pinned.set(true);
        self.ensure_icon(hwnd);
        // SAFETY: `hwnd` belongs to this thread.
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    }

    /// Brings a parked window back to the screen and drops the tray icon.
    pub(super) fn unpark(&self, hwnd: HWND) {
        self.pinned.set(false);
        self.remove(hwnd);
        restore(hwnd);
    }

    /// Re-creates the tray icon after `explorer.exe` restarts while the window is parked.
    pub(super) fn reattach(&self, hwnd: HWND) {
        self.shown.set(false);
        if self.pinned.get() {
            self.ensure_icon(hwnd);
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
        add_icon(hwnd, &data);
    }

    /// The icon's own messages; with version 4 the event sits in the low word.
    pub(super) fn message(&self, hwnd: HWND, lparam: LPARAM) -> TrayAction {
        match u32::try_from(lparam.0 & 0xFFFF).unwrap_or(0) {
            NIN_BALLOONUSERCLICK | NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                self.unpark(hwnd);
                TrayAction::None
            }
            NIN_BALLOONTIMEOUT => {
                if !self.pinned.get() {
                    self.remove(hwnd);
                }
                TrayAction::None
            }
            WM_CONTEXTMENU | WM_RBUTTONUP => self.context_menu(hwnd),
            _ => TrayAction::None,
        }
    }

    /// Takes the icon away, once the person is back or the window closes.
    pub(super) fn remove(&self, hwnd: HWND) {
        self.pinned.set(false);
        if self.shown.replace(false) {
            let data = icon(hwnd);
            // SAFETY: as in `announce`.
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
        }
    }

    fn ensure_icon(&self, hwnd: HWND) {
        if self.shown.replace(true) {
            return;
        }
        let data = icon(hwnd);
        add_icon(hwnd, &data);
    }

    fn context_menu(&self, hwnd: HWND) -> TrayAction {
        // SAFETY: menu lives for this call; `SetForegroundWindow` + `WM_NULL` ensure clicking
        // outside dismisses a tray menu cleanly.
        let picked = unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return TrayAction::None;
            };
            let open = HSTRING::from(RU.tray.open);
            let exit = HSTRING::from(RU.tray.exit);
            let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, &open);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT, &exit);
            let _ = SetMenuDefaultItem(menu, u32::try_from(MENU_OPEN).unwrap_or(1), 0);
            let mut point = POINT::default();
            let _ = GetCursorPos(&raw mut point);
            let _ = SetForegroundWindow(hwnd);
            let picked = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                hwnd,
                None,
            )
            .0;
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            picked
        };
        match usize::try_from(picked).ok() {
            Some(MENU_OPEN) => {
                self.unpark(hwnd);
                TrayAction::None
            }
            Some(MENU_EXIT) => TrayAction::Exit,
            _ => TrayAction::None,
        }
    }
}

fn add_icon(hwnd: HWND, data: &NOTIFYICONDATAW) {
    let mut version = icon(hwnd);
    version.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    // SAFETY: both structs describe this window's icon and outlive the calls.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, data);
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &raw const version);
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
    unsafe {
        GetForegroundWindow() == hwnd
            && IsWindowVisible(hwnd).as_bool()
            && !IsIconic(hwnd).as_bool()
    }
}

fn restore(hwnd: HWND) {
    // SAFETY: as above; a click on the tray lets this process take the foreground.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
