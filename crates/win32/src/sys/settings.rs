//! Native Win32 modal Settings dialog: user overrides and manual update controls.

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::thread;

use pastegrab_app::queue::Preferences;
use pastegrab_i18n::RU;
use windows::Win32::Foundation::{
    FILETIME, HINSTANCE, HWND, LPARAM, LRESULT, RECT, SYSTEMTIME, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, GetMonitorInfoW, GetSysColorBrush, HFONT, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_NOPREFIX};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
    BS_GROUPBOX, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBS_DROPDOWNLIST,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DC_HASDEFID, DM_GETDEFID,
    DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
    GetWindowRect, HMENU, IDC_ARROW, IDCANCEL, IsDialogMessageW, LoadCursorW, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, SW_SHOW, SendMessageW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT,
    WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_DLGMODALFRAME, WS_POPUP, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::shell;
use crate::cell::scale;
use crate::update::{self, UpdateControl, UpdateInfo};

const CLASS: PCWSTR = w!("pastegrab_settings");
const WM_UPDATE_DONE: u32 = WM_APP + 10;
const BST_CHECKED: usize = 1;

const ID_CLIPBOARD: i32 = 201;
const ID_TRAY: i32 = 202;
const ID_QUALITY: i32 = 203;
const ID_DOWNLOADS: i32 = 204;
const ID_VERSION: i32 = 205;
const ID_CHECKED: i32 = 206;
const ID_DOWNLOADED: i32 = 207;
const ID_STATUS: i32 = 208;
const ID_CHECK: i32 = 209;
const ID_RESTART: i32 = 210;
const ID_SAVE: i32 = 211;

static REGISTER: Once = Once::new();

/// Result of the modal Settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Cancelled,
    Saved(Preferences),
    Restart(Preferences),
}

/// Opens the modal Settings dialog centered over `owner` and returns the person's action.
pub(super) fn open(
    owner: HWND,
    dpi: i32,
    normal: HFONT,
    bold: HFONT,
    current: Preferences,
    busy: bool,
    updater: Arc<dyn UpdateControl>,
) -> Outcome {
    let Some(instance) = module_instance() else {
        return Outcome::Cancelled;
    };
    register_class(instance);

    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let ex_style = WS_EX_DLGMODALFRAME;
    let (x, y, width, height) = window_frame(owner, dpi, style, ex_style);

    let done = Rc::new(Cell::new(false));
    let outcome = Rc::new(Cell::new(Outcome::Cancelled));
    let state = Box::new(DialogState {
        dpi,
        normal,
        bold,
        initial: current,
        busy,
        updater,
        controls: OnceCell::new(),
        checking: Cell::new(false),
        pending: Arc::new(Mutex::new(None)),
        alive: Arc::new(AtomicBool::new(true)),
        done: Rc::clone(&done),
        outcome: Rc::clone(&outcome),
    });
    let state_ptr = Box::into_raw(state);
    let title = HSTRING::from(RU.settings.title);

    // SAFETY: `state_ptr` is owned by the dialog until WM_NCDESTROY (or reclaimed on error below).
    let dialog = unsafe {
        CreateWindowExW(
            ex_style,
            CLASS,
            &title,
            style,
            x,
            y,
            width,
            height,
            Some(owner),
            None,
            Some(instance),
            Some(state_ptr.cast_const().cast::<c_void>()),
        )
    };
    let Ok(dialog) = dialog else {
        // SAFETY: window creation failed before WM_NCCREATE attached the pointer.
        unsafe { drop(Box::from_raw(state_ptr)) };
        return Outcome::Cancelled;
    };

    // SAFETY: `owner` and `dialog` belong to this thread.
    unsafe {
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(dialog, SW_SHOW);
        let _ = UpdateWindow(dialog);
        let _ = SetFocus(Some(dialog));
    }

    let mut msg = MSG::default();
    let mut quit_code = None;
    while !done.get() {
        // SAFETY: thread-owned message struct.
        let ret = unsafe { GetMessageW(&raw mut msg, None, 0, 0) }.0;
        if ret <= 0 {
            if ret == 0 {
                quit_code = Some(msg.wParam.0);
            }
            break;
        }
        // SAFETY: dispatches messages for this modal loop.
        unsafe {
            if !IsDialogMessageW(dialog, &raw const msg).as_bool() {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
    }

    // Re-enable owner before destroying the modal window so Windows keeps focus on it.
    // SAFETY: `owner` and `dialog` belong to this thread.
    unsafe {
        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
        let _ = DestroyWindow(dialog);
    }
    if let Some(code) = quit_code {
        // SAFETY: forwards WM_QUIT received while the modal loop was open.
        unsafe { PostQuitMessage(i32::try_from(code).unwrap_or(0)) };
    }
    outcome.get()
}

fn module_instance() -> Option<HINSTANCE> {
    // SAFETY: queries the current executable handle.
    unsafe { GetModuleHandleW(None) }
        .ok()
        .map(|h| HINSTANCE(h.0))
}

fn register_class(instance: HINSTANCE) {
    REGISTER.call_once(|| {
        let wc = WNDCLASSEXW {
            cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dlgproc),
            hInstance: instance,
            // SAFETY: system cursor and brush handles need no release.
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            // SAFETY: as above.
            hbrBackground: unsafe { GetSysColorBrush(COLOR_BTNFACE) },
            hIcon: shell::icon(),
            hIconSm: shell::small_icon(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // SAFETY: `wc` and `CLASS` outlive the call.
        let _ = unsafe { RegisterClassExW(&raw const wc) };
    });
}

fn window_frame(
    owner: HWND,
    dpi: i32,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
) -> (i32, i32, i32, i32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: scale(450, dpi),
        bottom: scale(390, dpi),
    };
    // SAFETY: `rect` is ours.
    let _ = unsafe { AdjustWindowRectEx(&raw mut rect, style, false, ex_style) };
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    let mut owner_rect = RECT::default();
    // SAFETY: `owner_rect` is ours.
    let _ = unsafe { GetWindowRect(owner, &raw mut owner_rect) };
    let mut x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
    let mut y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;

    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `info` is ours and `MonitorFromWindow` accepts any valid HWND.
    let ok = unsafe {
        let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &raw mut info).as_bool()
    };
    if ok {
        let work = info.rcWork;
        x = x.clamp(work.left, (work.right - width).max(work.left));
        y = y.clamp(work.top, (work.bottom - height).max(work.top));
    }
    (x, y, width, height)
}

#[derive(Clone, Copy)]
struct Controls {
    clipboard: HWND,
    tray: HWND,
    quality: HWND,
    downloads: HWND,
    version: HWND,
    checked: HWND,
    downloaded: HWND,
    status: HWND,
    check: HWND,
    restart: HWND,
}

struct DialogState {
    dpi: i32,
    normal: HFONT,
    bold: HFONT,
    initial: Preferences,
    busy: bool,
    updater: Arc<dyn UpdateControl>,
    controls: OnceCell<Controls>,
    checking: Cell<bool>,
    pending: Arc<Mutex<Option<UpdateInfo>>>,
    alive: Arc<AtomicBool>,
    done: Rc<Cell<bool>>,
    outcome: Rc<Cell<Outcome>>,
}

impl DialogState {
    fn create(&self, hwnd: HWND) {
        let (clipboard, tray, quality, downloads) = self.create_general(hwnd);
        let (version, checked, downloaded, status, check, restart) = self.create_updates(hwnd);
        self.create_actions(hwnd);
        let controls = Controls {
            clipboard,
            tray,
            quality,
            downloads,
            version,
            checked,
            downloaded,
            status,
            check,
            restart,
        };
        let _ = self.controls.set(controls);
        self.apply_update(&self.updater.status());
    }

    fn create_general(&self, hwnd: HWND) -> (HWND, HWND, HWND, HWND) {
        let px = |v: i32| scale(v, self.dpi);
        let tab = WS_TABSTOP.0;
        let row_label = SS_NOPREFIX.0 | SS_CENTERIMAGE.0;
        let s = &RU.settings;

        let clipboard = add_control(
            hwnd,
            w!("BUTTON"),
            s.clipboard,
            tab | BS_AUTOCHECKBOX as u32,
            [px(16), px(16), px(418), px(24)],
            ID_CLIPBOARD,
            self.normal,
        );
        let tray = add_control(
            hwnd,
            w!("BUTTON"),
            s.close_to_tray,
            tab | BS_AUTOCHECKBOX as u32,
            [px(16), px(46), px(418), px(24)],
            ID_TRAY,
            self.normal,
        );
        let _ = add_control(
            hwnd,
            w!("STATIC"),
            s.quality,
            row_label,
            [px(16), px(82), px(210), px(28)],
            0,
            self.normal,
        );
        let quality = add_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            tab | CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0,
            [px(234), px(82), px(200), px(208)],
            ID_QUALITY,
            self.normal,
        );
        let _ = add_control(
            hwnd,
            w!("STATIC"),
            s.downloads,
            row_label,
            [px(16), px(118), px(210), px(28)],
            0,
            self.normal,
        );
        let downloads = add_control(
            hwnd,
            w!("COMBOBOX"),
            "",
            tab | CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0,
            [px(234), px(118), px(90), px(168)],
            ID_DOWNLOADS,
            self.normal,
        );

        set_checked(clipboard, self.initial.clipboard);
        set_checked(tray, self.initial.close_to_tray);
        for item in s.qualities {
            combo_add(quality, item);
        }
        combo_select(quality, update::height_cap_index(self.initial.height_cap));
        for n in ["1", "2", "3", "4"] {
            combo_add(downloads, n);
        }
        combo_select(
            downloads,
            usize::from(self.initial.downloads.clamp(1, 4)) - 1,
        );
        (clipboard, tray, quality, downloads)
    }

    fn create_updates(&self, hwnd: HWND) -> (HWND, HWND, HWND, HWND, HWND, HWND) {
        let px = |v: i32| scale(v, self.dpi);
        let tab = WS_TABSTOP.0;
        let label = SS_NOPREFIX.0 | SS_ENDELLIPSIS.0;
        let s = &RU.settings;

        let _ = add_control(
            hwnd,
            w!("BUTTON"),
            s.updates_heading,
            BS_GROUPBOX as u32,
            [px(16), px(160), px(418), px(166)],
            0,
            self.normal,
        );
        let version = add_control(
            hwnd,
            w!("STATIC"),
            "",
            label,
            [px(28), px(184), px(394), px(20)],
            ID_VERSION,
            self.bold,
        );
        let checked = add_control(
            hwnd,
            w!("STATIC"),
            "",
            label,
            [px(28), px(208), px(394), px(20)],
            ID_CHECKED,
            self.normal,
        );
        let downloaded = add_control(
            hwnd,
            w!("STATIC"),
            "",
            label,
            [px(28), px(232), px(394), px(20)],
            ID_DOWNLOADED,
            self.normal,
        );
        let status = add_control(
            hwnd,
            w!("STATIC"),
            "",
            label,
            [px(28), px(256), px(394), px(20)],
            ID_STATUS,
            self.normal,
        );
        let check = add_control(
            hwnd,
            w!("BUTTON"),
            s.check_now,
            tab | BS_PUSHBUTTON as u32,
            [px(28), px(284), px(186), px(30)],
            ID_CHECK,
            self.normal,
        );
        let restart = add_control(
            hwnd,
            w!("BUTTON"),
            s.restart_update,
            tab | BS_PUSHBUTTON as u32,
            [px(222), px(284), px(200), px(30)],
            ID_RESTART,
            self.normal,
        );
        (version, checked, downloaded, status, check, restart)
    }

    fn create_actions(&self, hwnd: HWND) {
        let px = |v: i32| scale(v, self.dpi);
        let tab = WS_TABSTOP.0;
        let s = &RU.settings;
        let _ = add_control(
            hwnd,
            w!("BUTTON"),
            s.save,
            tab | BS_DEFPUSHBUTTON as u32,
            [px(238), px(342), px(96), px(32)],
            ID_SAVE,
            self.normal,
        );
        let _ = add_control(
            hwnd,
            w!("BUTTON"),
            s.cancel,
            tab | BS_PUSHBUTTON as u32,
            [px(342), px(342), px(92), px(32)],
            IDCANCEL.0,
            self.normal,
        );
    }

    fn apply_update(&self, info: &UpdateInfo) {
        let Some(c) = self.controls.get() else {
            return;
        };
        let s = &RU.settings;
        set_text(c.version, &update::version_line(info, s));
        set_text(c.checked, &update::checked_line(info, format_unix_local, s));
        set_text(
            c.downloaded,
            &update::downloaded_line(info, format_unix_local, s),
        );
        set_text(c.status, &update::status_text(info, s));
        enable(c.check, !self.checking.get() && update::can_check(info));
        enable(c.restart, update::can_restart(info));
    }

    fn read_preferences(&self) -> Preferences {
        let Some(c) = self.controls.get() else {
            return self.initial;
        };
        let downloads = u8::try_from(combo_selected(c.downloads) + 1)
            .unwrap_or(2)
            .clamp(1, 4);
        Preferences {
            clipboard: is_checked(c.clipboard),
            close_to_tray: is_checked(c.tray),
            height_cap: update::height_cap_at(combo_selected(c.quality)),
            downloads,
        }
    }

    fn start_check(&self, hwnd: HWND) {
        let Some(c) = self.controls.get() else {
            return;
        };
        if self.checking.replace(true) {
            return;
        }
        enable(c.check, false);
        set_text(c.status, RU.settings.checking);

        let updater = Arc::clone(&self.updater);
        let pending = Arc::clone(&self.pending);
        let alive = Arc::clone(&self.alive);
        let raw_hwnd = hwnd.0.addr();
        let _ = thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let info = updater.check();
                if let Ok(mut slot) = pending.lock() {
                    *slot = Some(info);
                }
                if alive.load(Ordering::Acquire) {
                    let target = HWND(std::ptr::without_provenance_mut(raw_hwnd));
                    // SAFETY: `PostMessageW` safely ignores a handle if the dialog closed in flight.
                    let _ =
                        unsafe { PostMessageW(Some(target), WM_UPDATE_DONE, WPARAM(0), LPARAM(0)) };
                }
            });
    }

    fn finish_check(&self) {
        self.checking.set(false);
        let info = self
            .pending
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
            .unwrap_or_else(|| self.updater.status());
        self.apply_update(&info);
    }

    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM) -> Option<LRESULT> {
        match msg {
            WM_CREATE => self.create(hwnd),
            WM_CLOSE => self.done.set(true),
            WM_DESTROY => self.alive.store(false, Ordering::Release),
            DM_GETDEFID => {
                let packed = usize::try_from(ID_SAVE).unwrap_or(0) | (DC_HASDEFID as usize) << 16;
                return Some(LRESULT(packed.cast_signed()));
            }
            WM_UPDATE_DONE => self.finish_check(),
            WM_COMMAND => {
                let id = i32::try_from(wparam.0 & 0xFFFF).unwrap_or(0);
                let code = u32::try_from((wparam.0 >> 16) & 0xFFFF).unwrap_or(0);
                if code == BN_CLICKED {
                    match id {
                        ID_CHECK => self.start_check(hwnd),
                        ID_RESTART => {
                            if !self.busy
                                || shell::ask_owned(
                                    hwnd,
                                    RU.settings.title,
                                    RU.settings.restart_busy,
                                )
                            {
                                self.outcome.set(Outcome::Restart(self.read_preferences()));
                                self.done.set(true);
                            }
                        }
                        ID_SAVE => {
                            self.outcome.set(Outcome::Saved(self.read_preferences()));
                            self.done.set(true);
                        }
                        id if id == IDCANCEL.0 => self.done.set(true),
                        _ => {}
                    }
                }
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }
}

unsafe extern "system" fn dlgproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: `DialogState` is placed in GWLP_USERDATA at WM_NCCREATE and freed at WM_NCDESTROY.
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
        if state.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(state));
            return LRESULT(0);
        }
        match (*state).message(hwnd, msg, wparam) {
            Some(result) => result,
            None => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn add_control(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: u32,
    [x, y, w, h]: [i32; 4],
    id: i32,
    font: HFONT,
) -> HWND {
    let text = HSTRING::from(text);
    // SAFETY: parent belongs to this thread; strings outlive the call.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &text,
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(std::ptr::without_provenance_mut(
                usize::try_from(id).unwrap_or(0),
            ))),
            None,
            None,
        )
    }
    .unwrap_or_default();
    // SAFETY: `font` lives on the owner window for the duration of this modal dialog.
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            Some(WPARAM(font.0.addr())),
            Some(LPARAM(1)),
        )
    };
    hwnd
}

fn set_text(hwnd: HWND, text: &str) {
    let text = HSTRING::from(text);
    // SAFETY: `hwnd` belongs to this thread and `text` outlives the call.
    let _ = unsafe { SetWindowTextW(hwnd, &text) };
}

fn enable(hwnd: HWND, enabled: bool) {
    // SAFETY: `hwnd` belongs to this thread.
    let _ = unsafe { EnableWindow(hwnd, enabled) };
}

fn set_checked(hwnd: HWND, checked: bool) {
    let state = usize::from(checked) * BST_CHECKED;
    // SAFETY: `hwnd` is a checkbox on this thread.
    unsafe { SendMessageW(hwnd, BM_SETCHECK, Some(WPARAM(state)), Some(LPARAM(0))) };
}

fn is_checked(hwnd: HWND) -> bool {
    // SAFETY: `hwnd` is a checkbox on this thread.
    unsafe { SendMessageW(hwnd, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))) }.0
        == BST_CHECKED.cast_signed()
}

fn combo_add(hwnd: HWND, item: &str) {
    let item = HSTRING::from(item);
    // SAFETY: `item` outlives the synchronous CB_ADDSTRING.
    unsafe {
        SendMessageW(
            hwnd,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(item.as_ptr() as isize)),
        );
    }
}

fn combo_select(hwnd: HWND, index: usize) {
    // SAFETY: `hwnd` is a combobox on this thread.
    unsafe { SendMessageW(hwnd, CB_SETCURSEL, Some(WPARAM(index)), Some(LPARAM(0))) };
}

fn combo_selected(hwnd: HWND) -> usize {
    // SAFETY: `hwnd` is a combobox on this thread.
    let raw = unsafe { SendMessageW(hwnd, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) }.0;
    usize::try_from(raw).unwrap_or(0)
}

fn format_unix_local(secs: u64) -> String {
    const EPOCH_DIFF_SECS: u64 = 11_644_473_600;
    let ticks = secs
        .saturating_add(EPOCH_DIFF_SECS)
        .saturating_mul(10_000_000);
    let ft = FILETIME {
        dwLowDateTime: u32::try_from(ticks & 0xFFFF_FFFF).unwrap_or(0),
        dwHighDateTime: u32::try_from(ticks >> 32).unwrap_or(0),
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: `ft`, `utc` and `local` are stack values owned by this call.
    let ok = unsafe {
        FileTimeToSystemTime(&raw const ft, &raw mut utc).is_ok()
            && SystemTimeToTzSpecificLocalTime(None, &raw const utc, &raw mut local).is_ok()
    };
    if ok {
        update::format_date_time(
            local.wDay,
            local.wMonth,
            local.wYear,
            local.wHour,
            local.wMinute,
        )
    } else {
        RU.settings.unknown.to_owned()
    }
}
