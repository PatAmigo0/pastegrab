use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;

use pastegrab_app::board::{Board, Change, Row};
use pastegrab_app::ports::ViewUpdate;
use pastegrab_app::queue::{Input, Paste};
use pastegrab_app::runtime::Handle;
use pastegrab_app::tutorial::{self as guide, Target};
use pastegrab_domain::link::analyze;
use pastegrab_domain::profile::ProfileKind;
use pastegrab_i18n::RU;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, CreateFontIndirectW, DeleteObject, FW_SEMIBOLD, GetSysColorBrush, HFONT,
    UpdateWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::{
    SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_NOPREFIX, SS_PATHELLIPSIS,
};
use windows::Win32::UI::Controls::{
    EM_SETCUEBANNER, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVM_ENSUREVISIBLE,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_REDRAWITEMS, LVM_SETCOLUMNWIDTH,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMCOUNT, LVN_GETDISPINFOW, LVN_ITEMACTIVATE,
    LVN_KEYDOWN, LVNI_SELECTED, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_NOSORTHEADER,
    LVS_OWNERDATA, LVS_REPORT, LVS_SHAREIMAGELISTS, LVS_SHOWSELALWAYS, LVS_SINGLESEL,
    LVSICF_NOSCROLL, NM_CLICK, NMHDR, NMITEMACTIVATE, NMLVDISPINFOW, NMLVKEYDOWN, WC_LISTVIEWW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, VK_DELETE};
use windows::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL,
    CBN_SELCHANGE, CBS_DROPDOWNLIST, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    CreateWindowExW, DC_HASDEFID, DM_GETDEFID, DefWindowProcW, DispatchMessageW, EN_CHANGE,
    ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetMessageW, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, LoadCursorW,
    MINMAXINFO, MSG, MoveWindow, NONCLIENTMETRICSW, PostQuitMessage, RegisterClassExW,
    SM_CXVSCROLL, SPI_GETNONCLIENTMETRICS, SW_HIDE, SW_SHOW, SW_SHOWNORMAL,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, SystemParametersInfoW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND,
    WM_CREATE, WM_DESTROY, WM_GETMINMAXINFO, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_SETFONT,
    WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use windows::Win32::Foundation::POINT;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_STATE_FLAGS, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_SETITEMSTATE,
    NM_CUSTOMDRAW, NMLVCUSTOMDRAW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetDlgCtrlID, HMENU as MENU, IDCANCEL,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WA_INACTIVE, WM_ACTIVATE, WM_APP,
    WM_CONTEXTMENU, WM_COPYDATA, WM_DPICHANGED, WM_KEYDOWN, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE,
    WM_THEMECHANGED, WM_TIMER,
};

use super::bridge::{WM_CORE, WinView};
use super::dpi;
use super::fade::{self, Fade};
use super::instance::FORWARD;
use super::rows::{self, PaintContext, ProgressTheme, RowFades, RowHeight};
use super::shell;
use super::taskbar::Taskbar;
use super::tray::{Tray, WM_TRAY};
use super::tutorial::{self, Guide};
use crate::cell::{RowAction, scale};

/// The window class, which a second start looks for.
pub(super) const CLASS: PCWSTR = w!("pastegrab");
/// Ctrl+V outside the link box: paste the clipboard as a new link.
const WM_PASTE_ANYWHERE: u32 = WM_APP + 2;
const MENU_OPEN: i32 = 1;
const MENU_FOLDER: i32 = 2;
const MENU_RETRY: i32 = 3;
const MENU_CANCEL: i32 = 4;
use crate::text;

const ID_LINK: i32 = 101;
const ID_ANALYZE: i32 = 102;
const ID_TITLE: i32 = 103;
const ID_DETAIL: i32 = 104;
const ID_PROFILE: i32 = 105;
const ID_DOWNLOAD: i32 = 106;
const ID_LIST: i32 = 107;
const ID_FOLDER: i32 = 108;
const ID_CHANGE: i32 = 109;
const ID_CLEAR: i32 = 110;
const ID_HELP: i32 = 111;

/// What the window starts with.
#[derive(Debug, Clone)]
pub struct Startup {
    /// Where files go.
    pub dest: PathBuf,
    /// Links from the command line, analyzed as soon as the window shows.
    pub text: String,
    /// The highest tutorial version the person already saw.
    pub tutorial_seen: u32,
}

/// Shows the window, calls `shown` once it is on screen, and runs its message loop until the
/// person closes it.
///
/// # Errors
///
/// The system's reason when the window can't be created.
pub fn run(
    core: Handle,
    view: Arc<WinView>,
    startup: Startup,
    shown: impl FnOnce(),
) -> Result<(), String> {
    let common = INITCOMMONCONTROLSEX {
        dwSize: size_u32::<INITCOMMONCONTROLSEX>(),
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    // SAFETY: once per thread before any window or COM call; failures only cost the folder
    // picker or the visual style, and the structure outlives the call.
    let instance = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = InitCommonControlsEx(&raw const common);
        GetModuleHandleW(None).map_err(|e| e.to_string())?
    };
    let instance = HINSTANCE(instance.0);
    let class = CLASS;
    let wc = WNDCLASSEXW {
        cbSize: size_u32::<WNDCLASSEXW>(),
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        // SAFETY: system cursor and brush handles need no release.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        // SAFETY: as above.
        hbrBackground: unsafe { GetSysColorBrush(COLOR_BTNFACE) },
        hIcon: shell::icon(),
        hIconSm: shell::small_icon(),
        lpszClassName: class,
        ..Default::default()
    };
    // SAFETY: `wc` and the class name outlive the call.
    if unsafe { RegisterClassExW(&raw const wc) } == 0 {
        return Err("RegisterClassExW failed".into());
    }
    dpi::enable();
    let dpi = dpi::for_primary();
    let ui = Box::into_raw(Box::new(Ui::new(core, view, startup, dpi)));
    let title = HSTRING::from(RU.window_title);
    // SAFETY: `ui` stays alive until WM_NCDESTROY takes it back; the strings outlive the call.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &title,
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            scale(880, dpi),
            scale(560, dpi),
            None,
            None,
            Some(instance),
            Some(ui.cast_const().cast::<c_void>()),
        )
    }
    .map_err(|e| e.to_string())?;
    // SAFETY: the window belongs to this thread.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
    }
    shown();
    // SAFETY: as above, and `msg` is ours.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, None, 0, 0).0 > 0 {
            let ctrl = GetKeyState(i32::from(VK_CONTROL.0)) < 0;
            let paste = msg.message == WM_KEYDOWN && msg.wParam.0 == usize::from(b'V') && ctrl;
            if paste && GetDlgCtrlID(msg.hwnd) != ID_LINK {
                let _ = PostMessageW(Some(hwnd), WM_PASTE_ANYWHERE, WPARAM(0), LPARAM(0));
                continue;
            }
            if !IsDialogMessageW(hwnd, &raw const msg).as_bool() {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: Windows passes this window's own messages; the pointer in GWLP_USERDATA is the
    // `Ui` from `run`, set at WM_NCCREATE and taken back at WM_NCDESTROY.
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let ui = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ui;
        if ui.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ui));
            return LRESULT(0);
        }
        match (*ui).message(hwnd, msg, wparam, lparam) {
            Some(result) => result,
            None => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Controls {
    link: HWND,
    analyze: HWND,
    help: HWND,
    title: HWND,
    detail: HWND,
    profile: HWND,
    download: HWND,
    list: HWND,
    folder: HWND,
    change: HWND,
    clear: HWND,
}

impl Controls {
    /// Every child control of the window; `layout` places them.
    fn new(hwnd: HWND) -> Self {
        let tab = WS_TABSTOP.0;
        let label = SS_NOPREFIX.0 | SS_ENDELLIPSIS.0;
        Self {
            link: child(
                hwnd,
                w!("EDIT"),
                "",
                tab | ES_AUTOHSCROLL as u32,
                true,
                ID_LINK,
            ),
            analyze: child(
                hwnd,
                w!("BUTTON"),
                RU.analyze,
                tab | BS_PUSHBUTTON as u32,
                false,
                ID_ANALYZE,
            ),
            help: child(
                hwnd,
                w!("BUTTON"),
                RU.help,
                tab | BS_PUSHBUTTON as u32,
                false,
                ID_HELP,
            ),
            title: child(hwnd, w!("STATIC"), "", label, false, ID_TITLE),
            detail: child(hwnd, w!("STATIC"), "", label, false, ID_DETAIL),
            profile: child(
                hwnd,
                w!("COMBOBOX"),
                "",
                tab | CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0,
                false,
                ID_PROFILE,
            ),
            download: child(
                hwnd,
                w!("BUTTON"),
                RU.download,
                tab | BS_DEFPUSHBUTTON as u32,
                false,
                ID_DOWNLOAD,
            ),
            list: child(
                hwnd,
                WC_LISTVIEWW,
                "",
                tab | LVS_REPORT
                    | LVS_OWNERDATA
                    | LVS_SINGLESEL
                    | LVS_SHOWSELALWAYS
                    | LVS_NOSORTHEADER
                    | LVS_SHAREIMAGELISTS,
                true,
                ID_LIST,
            ),
            folder: child(
                hwnd,
                w!("STATIC"),
                "",
                SS_NOPREFIX.0 | SS_PATHELLIPSIS.0 | SS_CENTERIMAGE.0,
                false,
                ID_FOLDER,
            ),
            change: child(
                hwnd,
                w!("BUTTON"),
                RU.change_folder,
                tab | BS_PUSHBUTTON as u32,
                false,
                ID_CHANGE,
            ),
            clear: child(
                hwnd,
                w!("BUTTON"),
                RU.clear,
                tab | BS_PUSHBUTTON as u32,
                false,
                ID_CLEAR,
            ),
        }
    }
}

struct Fonts {
    normal: HFONT,
    bold: HFONT,
}

impl Drop for Fonts {
    fn drop(&mut self) {
        // SAFETY: the window that used the fonts is gone when its `Ui` drops.
        unsafe {
            let _ = DeleteObject(self.normal.into());
            let _ = DeleteObject(self.bold.into());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Card {
    Hidden,
    Notice,
    Shown,
}

/// The window's state. Each field is borrowed only between Win32 calls, because a message sent
/// to a control can come back into the window procedure before it returns.
struct Ui {
    core: Handle,
    view: Arc<WinView>,
    board: RefCell<Board>,
    dest: RefCell<PathBuf>,
    startup: RefCell<Option<String>>,
    controls: OnceCell<Controls>,
    fonts: RefCell<Option<Fonts>>,
    dpi: Cell<i32>,
    card: Cell<Card>,
    notice: Cell<&'static str>,
    link_len: Cell<usize>,
    clipboard: RefCell<String>,
    taskbar: Taskbar,
    tray: Tray,
    fade: Fade,
    row_fades: RowFades,
    button_widths: Cell<(i32, i32)>,
    row_height: RefCell<Option<RowHeight>>,
    progress_theme: RefCell<Option<ProgressTheme>>,
    high_contrast: Cell<bool>,
    clear_enabled: Cell<bool>,
    guide: Guide,
}

impl Ui {
    fn new(core: Handle, view: Arc<WinView>, startup: Startup, dpi: i32) -> Self {
        Self {
            core,
            view,
            board: RefCell::default(),
            dest: RefCell::new(startup.dest),
            startup: RefCell::new(Some(startup.text).filter(|text| !text.trim().is_empty())),
            controls: OnceCell::new(),
            fonts: RefCell::new(None),
            dpi: Cell::new(dpi),
            card: Cell::new(Card::Hidden),
            notice: Cell::new(""),
            link_len: Cell::new(0),
            clipboard: RefCell::default(),
            taskbar: Taskbar::new(),
            tray: Tray::new(),
            fade: Fade::default(),
            row_fades: RowFades::default(),
            button_widths: Cell::new((64, 48)),
            row_height: RefCell::new(None),
            progress_theme: RefCell::new(None),
            high_contrast: Cell::new(rows::high_contrast()),
            clear_enabled: Cell::new(false),
            guide: Guide::new(startup.tutorial_seen),
        }
    }

    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_CREATE => self.create(hwnd),
            WM_SIZE => self.layout(hwnd),
            WM_DPICHANGED => self.dpi_changed(hwnd, wparam, lparam),
            WM_GETMINMAXINFO => {
                // SAFETY: WM_GETMINMAXINFO's lparam points to a MINMAXINFO for this call.
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                let dpi = self.dpi.get();
                info.ptMinTrackSize.x = scale(600, dpi);
                info.ptMinTrackSize.y = scale(420, dpi);
            }
            WM_COMMAND => self.command(hwnd, low(wparam.0), high(wparam.0)),
            WM_NOTIFY => return self.notify(lparam),
            DM_GETDEFID => {
                let id = self.default_button();
                let packed = usize::try_from(id).unwrap_or(0) | (DC_HASDEFID as usize) << 16;
                return Some(LRESULT(packed.cast_signed()));
            }
            WM_CORE => self.drain(hwnd),
            created if created == self.taskbar.created() => self.taskbar.attach(hwnd),
            WM_TRAY => self.tray.message(hwnd, lparam),
            WM_TIMER if wparam.0 == fade::TIMER => {
                let ended = self.fade.tick(hwnd);
                if ended & fade::TUTORIAL != 0 && self.guide.finish_close() {
                    self.layout(hwnd);
                }
            }
            WM_TIMER if wparam.0 == rows::TIMER => {
                let board = self.board.borrow();
                self.row_fades
                    .tick(hwnd, self.controls().list, board.rows());
            }
            WM_THEMECHANGED => {
                let list = self.controls().list;
                let _ = self.progress_theme.replace(ProgressTheme::open(list));
                self.high_contrast.set(rows::high_contrast());
                return None;
            }
            WM_SYSCOLORCHANGE | WM_SETTINGCHANGE => {
                self.high_contrast.set(rows::high_contrast());
                return None;
            }
            WM_ACTIVATE => {
                if u32::try_from(low(wparam.0)).unwrap_or(0) != WA_INACTIVE {
                    self.tray.remove(hwnd);
                    self.pick_up(hwnd, false);
                }
                return None;
            }
            WM_PASTE_ANYWHERE => self.pick_up(hwnd, true),
            WM_CONTEXTMENU if wparam.0 == self.controls().list.0.addr() => self.context_menu(hwnd),
            WM_COPYDATA => {
                // SAFETY: WM_COPYDATA's lparam points to a COPYDATASTRUCT valid for this call.
                let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
                if data.dwData == FORWARD && !data.lpData.is_null() {
                    let units = usize::try_from(data.cbData).unwrap_or(0) / 2;
                    // SAFETY: the sender's buffer holds `cbData` bytes of UTF-16 for this call.
                    let text =
                        unsafe { std::slice::from_raw_parts(data.lpData.cast::<u16>(), units) };
                    let text = String::from_utf16_lossy(text);
                    if !text.trim().is_empty() {
                        self.submit(text);
                    }
                }
                return Some(LRESULT(1));
            }
            WM_DESTROY => {
                self.guide.clear();
                self.tray.remove(hwnd);
                // SAFETY: ends this thread's message loop.
                unsafe { PostQuitMessage(0) };
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn controls(&self) -> Controls {
        *self.controls.get().unwrap_or(&Controls {
            link: HWND::default(),
            analyze: HWND::default(),
            help: HWND::default(),
            title: HWND::default(),
            detail: HWND::default(),
            profile: HWND::default(),
            download: HWND::default(),
            list: HWND::default(),
            folder: HWND::default(),
            change: HWND::default(),
            clear: HWND::default(),
        })
    }

    fn create(&self, hwnd: HWND) {
        let dpi = dpi::for_window(hwnd);
        self.dpi.set(dpi);
        let controls = Controls::new(hwnd);
        let _ = self.controls.set(controls);
        let next_fonts = fonts(dpi);
        let (normal, bold) = (next_fonts.normal, next_fonts.bold);
        self.apply_fonts(normal, bold);
        self.button_widths
            .set(rows::measure_buttons(controls.list, normal));
        let _ = self.fonts.replace(Some(next_fonts));
        let _ = self
            .row_height
            .replace(RowHeight::apply(controls.list, dpi));
        let _ = self
            .progress_theme
            .replace(ProgressTheme::open(controls.list));
        enable(controls.clear, false);
        cue_banner(controls.link, RU.paste_hint);
        set_list_style(controls.list, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
        insert_column(controls.list, 0, RU.columns.title, scale(360, dpi));
        insert_column(controls.list, 1, RU.columns.state, scale(330, dpi));
        for name in RU.profiles {
            combo_add(controls.profile, name);
        }
        self.show_folder();
        self.refresh_card(hwnd);
        self.guide.open_if_needed(hwnd, normal, bold, &self.fade);
        self.view.attach(hwnd);
        let startup = self.startup.borrow_mut().take();
        if let Some(text) = startup {
            self.submit(text);
        }
    }

    fn apply_fonts(&self, normal: HFONT, bold: HFONT) {
        let c = self.controls();
        for control in [
            c.link, c.analyze, c.help, c.detail, c.profile, c.download, c.list, c.folder, c.change,
            c.clear,
        ] {
            set_font(control, normal);
        }
        set_font(c.title, bold);
        self.guide.apply_fonts(normal, bold);
    }

    fn dpi_changed(&self, hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
        let next = i32::try_from(high(wparam.0)).unwrap_or(0);
        if next <= 0 {
            return;
        }
        self.dpi.set(next);
        let next_fonts = fonts(next);
        self.apply_fonts(next_fonts.normal, next_fonts.bold);
        let list = self.controls().list;
        self.button_widths
            .set(rows::measure_buttons(list, next_fonts.normal));
        let _old = self.fonts.replace(Some(next_fonts));
        let _old_height = self.row_height.replace(RowHeight::apply(list, next));
        // SAFETY: WM_DPICHANGED's lparam points to Windows' suggested RECT for this call.
        let rect = unsafe { &*(lparam.0 as *const RECT) };
        // SAFETY: `hwnd` belongs to this thread.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        self.layout(hwnd);
    }

    fn layout(&self, hwnd: HWND) {
        let c = self.controls();
        let dpi = self.dpi.get();
        let px = |v: i32| scale(v, dpi);
        let mut client = RECT::default();
        // SAFETY: `client` outlives the call.
        let _ = unsafe { GetClientRect(hwnd, &raw mut client) };
        let (width, height) = (client.right, client.bottom);
        let (margin, gap, tall, button, wide) = (px(16), px(8), px(32), px(104), px(150));
        let help_x = width - margin - tall;
        let analyze_x = help_x - gap - button;
        let link_w = analyze_x - gap - margin;
        place(c.link, margin, margin, link_w, tall);
        place(c.analyze, analyze_x, margin, button, tall);
        place(c.help, help_x, margin, tall, tall);
        let link_rect = RECT {
            left: margin,
            top: margin,
            right: margin + link_w,
            bottom: margin + tall,
        };
        let mut top = margin + tall + px(12);
        let mut card_rect = None;
        match self.card.get() {
            Card::Hidden => {}
            Card::Notice => {
                place(c.title, margin, top, width - 2 * margin, px(26));
                top += px(26) + px(12);
            }
            Card::Shown => {
                place(c.title, margin, top, width - 2 * margin, px(26));
                place(c.detail, margin, top + px(28), width - 2 * margin, px(22));
                let row = top + px(56);
                place(c.profile, margin, row, px(340), tall + px(220));
                place(c.download, width - margin - wide, row, wide, tall);
                card_rect = Some(RECT {
                    left: margin,
                    top: row,
                    right: width - margin,
                    bottom: row + tall,
                });
                top = row + tall + px(12);
            }
        }
        let guide_h = self.guide.height(dpi);
        let guide_gap = if guide_h > 0 { gap } else { 0 };
        let bottom = height - margin - guide_h - guide_gap - tall;
        let list_width = width - 2 * margin;
        let list_height = (bottom - gap - top).max(px(80));
        place(c.list, margin, top, list_width, list_height);
        let list_rect = RECT {
            left: margin,
            top,
            right: margin + list_width,
            bottom: top + list_height,
        };
        // SAFETY: a plain query without pointers.
        let scrollbar = unsafe { GetSystemMetrics(SM_CXVSCROLL) };
        let state_width = px(330).max(list_width / 2);
        set_column_width(c.list, 0, list_width - state_width - scrollbar - px(4));
        set_column_width(c.list, 1, state_width);
        place(c.clear, margin, bottom, wide, tall);
        let folder_x = margin + wide + px(12);
        place(
            c.folder,
            folder_x,
            bottom,
            width - 2 * margin - 2 * gap - button - wide,
            tall,
        );
        place(c.change, width - margin - button, bottom, button, tall);
        let folder_rect = RECT {
            left: folder_x,
            top: bottom,
            right: width - margin,
            bottom: bottom + tall,
        };
        let target = match self.guide.target() {
            Target::None => None,
            Target::Link => Some(link_rect),
            Target::Card => card_rect,
            Target::List => Some(list_rect),
            Target::Folder => Some(folder_rect),
        };
        self.guide.layout(width, bottom + tall + gap, dpi, target);
    }

    fn command(&self, hwnd: HWND, id: i32, code: u32) {
        let c = self.controls();
        match (id, code) {
            (ID_LINK, EN_CHANGE) => {
                let text = window_text(c.link);
                let len = text.chars().count();
                let pasted = len > self.link_len.get() + 1;
                self.link_len.set(len);
                if pasted && !analyze(&text).is_empty() {
                    self.submit(text);
                }
            }
            (ID_ANALYZE, BN_CLICKED) => {
                let text = window_text(c.link);
                if !text.trim().is_empty() {
                    self.submit(text);
                }
            }
            (ID_DOWNLOAD, BN_CLICKED) => {
                let ids: Vec<_> = {
                    let board = self.board.borrow();
                    board
                        .waiting()
                        .filter(|row| row.plan.is_some())
                        .map(|row| row.id)
                        .collect()
                };
                for id in ids {
                    let _ = self.core.send(Input::Start(id));
                }
            }
            (ID_PROFILE, CBN_SELCHANGE) => {
                let Some(&profile) = ProfileKind::ALL.get(combo_selected(c.profile)) else {
                    return;
                };
                let ids: Vec<_> = self.board.borrow().waiting().map(|row| row.id).collect();
                for id in ids {
                    let _ = self.core.send(Input::Profile(id, profile));
                }
                let _ = self.core.send(Input::DefaultProfile(profile));
            }
            (ID_CLEAR, BN_CLICKED) => self.clear_ended(),
            (ID_HELP, BN_CLICKED) => {
                if let Some((normal, bold)) =
                    self.fonts.borrow().as_ref().map(|f| (f.normal, f.bold))
                {
                    self.guide.replay(hwnd, normal, bold, &self.fade);
                    self.layout(hwnd);
                }
            }
            (tutorial::ID_NEXT, BN_CLICKED) => {
                if self.guide.next(hwnd, &self.fade) {
                    let _ = self.core.send(Input::TutorialSeen(guide::VERSION));
                }
                self.layout(hwnd);
            }
            (id, BN_CLICKED) if id == tutorial::ID_SKIP || id == IDCANCEL.0 => {
                if self.guide.skip(hwnd, &self.fade) {
                    let _ = self.core.send(Input::TutorialSeen(guide::VERSION));
                    self.layout(hwnd);
                }
            }
            (ID_CHANGE, BN_CLICKED) => {
                let start = self.dest.borrow().clone();
                if let Some(folder) = shell::pick_folder(hwnd, &start) {
                    self.dest.borrow_mut().clone_from(&folder);
                    let _ = self.core.send(Input::Dest(folder));
                    self.show_folder();
                }
            }
            _ => {}
        }
    }

    fn notify(&self, lparam: LPARAM) -> Option<LRESULT> {
        // SAFETY: WM_NOTIFY's lparam points to an NMHDR, the head of the notification.
        let header = unsafe { &*(lparam.0 as *const NMHDR) };
        if header.idFrom != usize::try_from(ID_LIST).unwrap_or(0) {
            return None;
        }
        match header.code {
            LVN_GETDISPINFOW => {
                // SAFETY: LVN_GETDISPINFOW comes with an NMLVDISPINFOW the list owns.
                let info = unsafe { &mut *(lparam.0 as *mut NMLVDISPINFOW) };
                if info.item.mask.0 & LVIF_TEXT.0 != 0 {
                    let board = self.board.borrow();
                    let row = usize::try_from(info.item.iItem)
                        .ok()
                        .and_then(|i| board.rows().get(i));
                    let text = match (row, info.item.iSubItem) {
                        (Some(row), 0) => row.name().to_owned(),
                        (Some(row), _) => text::state(row, &RU),
                        (None, _) => String::new(),
                    };
                    fill(info.item.pszText, info.item.cchTextMax, &text);
                }
            }
            NM_CUSTOMDRAW => {
                // SAFETY: a report list view's NM_CUSTOMDRAW comes with an NMLVCUSTOMDRAW.
                let draw = unsafe { &mut *(lparam.0 as *mut NMLVCUSTOMDRAW) };
                let board = self.board.borrow();
                let theme = self.progress_theme.borrow();
                let ctx = PaintContext {
                    rows: board.rows(),
                    fades: &self.row_fades,
                    theme: theme.as_ref(),
                    high_contrast: self.high_contrast.get(),
                    dpi: self.dpi.get(),
                    button_widths: self.button_widths.get(),
                };
                return Some(rows::paint(header.hwndFrom, draw, &ctx));
            }
            NM_CLICK => {
                // SAFETY: NM_CLICK on a list view comes with an NMITEMACTIVATE.
                let item = unsafe { &*(lparam.0 as *const NMITEMACTIVATE) };
                self.row_click(header.hwndFrom, item);
            }
            LVN_ITEMACTIVATE => {
                // SAFETY: LVN_ITEMACTIVATE comes with an NMITEMACTIVATE.
                let item = unsafe { &*(lparam.0 as *const NMITEMACTIVATE) };
                let file = {
                    let board = self.board.borrow();
                    let index = usize::try_from(item.iItem).ok();
                    let row = index.and_then(|i| board.rows().get(i));
                    let on_button = index.zip(row).is_some_and(|(i, row)| {
                        rows::hit(
                            header.hwndFrom,
                            row,
                            i,
                            item.ptAction,
                            self.dpi.get(),
                            self.button_widths.get(),
                        )
                        .is_some()
                    });
                    if on_button {
                        None
                    } else {
                        row.and_then(|row| row.file.clone())
                    }
                };
                if let Some(file) = file {
                    shell::open(&file);
                }
            }
            LVN_KEYDOWN => {
                // SAFETY: LVN_KEYDOWN comes with an NMLVKEYDOWN.
                let key = unsafe { &*(lparam.0 as *const NMLVKEYDOWN) };
                // SAFETY: a plain query without pointers.
                let ctrl = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
                let row = self.selected_row();
                match (key.wVKey, row) {
                    (k, Some((id, ..))) if k == VK_DELETE.0 => {
                        let _ = self.core.send(Input::Cancel(id));
                    }
                    (k, Some((_, Some(file), _))) if ctrl && k == u16::from(b'O') => {
                        shell::show_in_folder(&file);
                    }
                    _ => {}
                }
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn row_click(&self, list: HWND, item: &NMITEMACTIVATE) {
        let Ok(index) = usize::try_from(item.iItem) else {
            return;
        };
        let picked = {
            let board = self.board.borrow();
            let Some(row) = board.rows().get(index) else {
                return;
            };
            let action = rows::hit(
                list,
                row,
                index,
                item.ptAction,
                self.dpi.get(),
                self.button_widths.get(),
            );
            action.map(|action| (action, row.id, row.file.clone()))
        };
        match picked {
            Some((RowAction::Cancel, id, _)) => {
                let _ = self.core.send(Input::Cancel(id));
            }
            Some((RowAction::Open, _, Some(file))) if file.exists() => shell::open(&file),
            Some((RowAction::Folder, _, Some(file))) if file.exists() => {
                shell::show_in_folder(&file);
            }
            _ => {}
        }
    }

    fn default_button(&self) -> i32 {
        let c = self.controls();
        if !window_text(c.link).trim().is_empty() {
            ID_ANALYZE
        } else if self.card.get() == Card::Shown {
            ID_DOWNLOAD
        } else if self.guide.is_open() {
            tutorial::ID_NEXT
        } else {
            0
        }
    }

    fn submit(&self, text: String) {
        self.notice.set("");
        let paste = Paste {
            text,
            profile: None,
            section: None,
            hold: true,
        };
        let _ = self.core.send(Input::Paste(paste));
        let link = self.controls().link;
        self.link_len.set(0);
        set_text(link, "");
    }

    fn drain(&self, hwnd: HWND) {
        let (updates, progress) = self.view.drain();
        let mut added = false;
        let mut added_ids = Vec::new();
        let mut finished_ids = Vec::new();
        let mut card = false;
        let mut rows = Vec::new();
        let mut ended = None;
        let (count, notice, activity, has_ended) = {
            let mut board = self.board.borrow_mut();
            for update in updates {
                let added_id = match update {
                    ViewUpdate::Added { id, .. } => Some(id),
                    _ => None,
                };
                let finished_id = match update {
                    ViewUpdate::Finished { id, .. } => Some(id),
                    _ => None,
                };
                let ends = matches!(
                    update,
                    ViewUpdate::Finished { .. } | ViewUpdate::Failed { .. }
                );
                match board.apply(update) {
                    Change::Added => {
                        added = true;
                        if let Some(id) = added_id {
                            added_ids.push(id);
                        }
                    }
                    Change::Row(index) => {
                        rows.push(index);
                        card = true;
                        ended = ends.then_some(index).or(ended);
                        if let Some(id) = finished_id {
                            finished_ids.push(id);
                        }
                    }
                    Change::NoLinks => {
                        self.notice.set(RU.no_links);
                        card = true;
                    }
                    Change::Nothing => {}
                }
            }
            for (id, phase, numbers) in progress {
                if let Change::Row(index) = board.progress(id, phase, numbers) {
                    rows.push(index);
                }
            }
            let notice = ended
                .and_then(|index| board.rows().get(index))
                .and_then(|row| text::notice(row, &RU));
            (
                board.rows().len(),
                notice,
                board.activity(),
                board.has_ended(),
            )
        };
        for id in added_ids {
            self.row_fades.add(hwnd, id);
        }
        for id in finished_ids {
            self.row_fades.finish(hwnd, id);
        }
        if let Some(notice) = notice {
            self.tray.announce(hwnd, &notice);
        }
        let list = self.controls().list;
        if added {
            set_item_count(list, count);
            ensure_visible(list, count.saturating_sub(1));
        }
        if rows.len() > 1 {
            rows.sort_unstable();
            rows.dedup();
        }
        for index in rows {
            redraw_item(list, index);
        }
        if card || added {
            self.refresh_card(hwnd);
            self.layout(hwnd);
        }
        self.taskbar.show(hwnd, activity);
        if self.clear_enabled.replace(has_ended) != has_ended {
            enable(self.controls().clear, has_ended);
        }
    }

    /// Drops the rows that ended; rows that run or wait stay.
    fn clear_ended(&self) {
        let c = self.controls();
        let count = {
            let mut board = self.board.borrow_mut();
            board.clear_ended();
            board.rows().len()
        };
        deselect_all(c.list);
        set_item_count(c.list, count);
        self.clear_enabled.set(false);
        enable(c.clear, false);
    }

    fn refresh_card(&self, hwnd: HWND) {
        let c = self.controls();
        let (state, title, detail, profile, can_download) = {
            let board = self.board.borrow();
            let waiting: Vec<&Row> = board.waiting().collect();
            match waiting.first() {
                Some(newest) => (
                    Card::Shown,
                    text::card_title(&waiting, &RU),
                    text::card_detail(newest, &RU),
                    ProfileKind::ALL
                        .iter()
                        .position(|&kind| kind == newest.profile)
                        .unwrap_or(0),
                    waiting.iter().any(|row| row.plan.is_some()),
                ),
                None if !self.notice.get().is_empty() => (
                    Card::Notice,
                    self.notice.get().to_owned(),
                    String::new(),
                    0,
                    false,
                ),
                None => (Card::Hidden, String::new(), String::new(), 0, false),
            }
        };
        let before = self.card.replace(state);
        set_text(c.title, &title);
        set_text(c.detail, &detail);
        combo_select(c.profile, profile);
        enable(c.download, can_download);
        if before == Card::Hidden {
            match state {
                Card::Shown => self.fade.show(
                    hwnd,
                    fade::CARD,
                    &[c.title, c.detail, c.profile, c.download],
                ),
                Card::Notice => self.fade.show(hwnd, fade::CARD, &[c.title]),
                Card::Hidden => {}
            }
        }
        show(c.title, state != Card::Hidden);
        for control in [c.detail, c.profile, c.download] {
            show(control, state == Card::Shown);
        }
    }

    /// Reads the clipboard: on Ctrl+V always, on activation only when its text changed since the
    /// last look, so returning to the window doesn't add the same link again.
    fn pick_up(&self, hwnd: HWND, asked: bool) {
        let Some(text) = shell::clipboard_text(hwnd) else {
            return;
        };
        let seen = *self.clipboard.borrow() == text;
        self.clipboard.borrow_mut().clone_from(&text);
        if (asked || !seen) && !analyze(&text).is_empty() {
            self.submit(text);
        }
    }

    fn selected_row(
        &self,
    ) -> Option<(
        pastegrab_domain::job::JobId,
        Option<PathBuf>,
        pastegrab_domain::job::JobState,
    )> {
        let index = selected(self.controls().list)?;
        let board = self.board.borrow();
        let row = board.rows().get(index)?;
        Some((row.id, row.file.clone(), row.state))
    }

    fn context_menu(&self, hwnd: HWND) {
        let Some((id, file, state)) = self.selected_row() else {
            return;
        };
        let file = file.filter(|file| file.exists());
        let failed = matches!(
            state,
            pastegrab_domain::job::JobState::Failed(_) | pastegrab_domain::job::JobState::Cancelled
        );
        let menu = &RU.menu;
        let items = [
            (MENU_OPEN, menu.open, file.is_some()),
            (MENU_FOLDER, menu.show_in_folder, file.is_some()),
            (0, "", false),
            (MENU_RETRY, menu.retry, failed),
            (MENU_CANCEL, menu.cancel, !state.is_over()),
        ];
        let Some(chosen) = popup(hwnd, &items) else {
            return;
        };
        match (chosen, file) {
            (MENU_OPEN, Some(file)) => shell::open(&file),
            (MENU_FOLDER, Some(file)) => shell::show_in_folder(&file),
            (MENU_RETRY, _) => {
                let _ = self.core.send(Input::Retry(id));
            }
            (MENU_CANCEL, _) => {
                let _ = self.core.send(Input::Cancel(id));
            }
            _ => {}
        }
    }

    fn show_folder(&self) {
        let text = format!("{} {}", RU.save_to, self.dest.borrow().display());
        set_text(self.controls().folder, &text);
    }
}

fn child(parent: HWND, class: PCWSTR, text: &str, style: u32, edge: bool, id: i32) -> HWND {
    let text = HSTRING::from(text);
    let ex = if edge {
        WS_EX_CLIENTEDGE
    } else {
        WINDOW_EX_STYLE(0)
    };
    let menu = HMENU(std::ptr::without_provenance_mut(
        usize::try_from(id).unwrap_or(0),
    ));
    // SAFETY: the parent is this thread's window, and the strings outlive the call.
    unsafe {
        CreateWindowExW(
            ex,
            class,
            &text,
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
            0,
            0,
            0,
            0,
            Some(parent),
            Some(menu),
            None,
            None,
        )
    }
    .unwrap_or_default()
}

fn fonts(dpi: i32) -> Fonts {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_u32::<NONCLIENTMETRICSW>(),
        ..Default::default()
    };
    // SAFETY: `metrics` is writable, its size is set, and it outlives the call.
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some((&raw mut metrics).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    let mut face = metrics.lfMessageFont;
    // 11 points instead of the default 9: small grey text is what an older reader loses first.
    face.lfHeight = -(11 * dpi + 36) / 72;
    let mut heavy = face;
    heavy.lfWeight = FW_SEMIBOLD.0.cast_signed();
    // SAFETY: both descriptions outlive the calls; `Fonts` deletes the handles.
    unsafe {
        Fonts {
            normal: CreateFontIndirectW(&raw const face),
            bold: CreateFontIndirectW(&raw const heavy),
        }
    }
}

fn size_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

fn low(value: usize) -> i32 {
    u16::try_from(value & 0xFFFF).map_or(0, i32::from)
}

fn high(value: usize) -> u32 {
    u16::try_from((value >> 16) & 0xFFFF).map_or(0, u32::from)
}

fn place(control: HWND, x: i32, y: i32, width: i32, height: i32) {
    // SAFETY: a plain call on this thread's control.
    let _ = unsafe { MoveWindow(control, x, y, width.max(0), height.max(0), true) };
}

fn show(control: HWND, visible: bool) {
    // SAFETY: a plain call on this thread's control.
    let _ = unsafe { ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE }) };
}

fn enable(control: HWND, enabled: bool) {
    // SAFETY: a plain call on this thread's control.
    let _ = unsafe { EnableWindow(control, enabled) };
}

fn set_text(control: HWND, text: &str) {
    let text = HSTRING::from(text);
    // SAFETY: the string outlives the call.
    let _ = unsafe { SetWindowTextW(control, &text) };
}

fn window_text(control: HWND) -> String {
    // SAFETY: the buffer is as long as the length asked for, terminator included.
    unsafe {
        let len = usize::try_from(GetWindowTextLengthW(control)).unwrap_or(0);
        let mut buf = vec![0; len + 1];
        let copied = usize::try_from(GetWindowTextW(control, &mut buf)).unwrap_or(0);
        String::from_utf16_lossy(&buf[..copied.min(len)])
    }
}

/// Copies `text` into a buffer a control lends for one notification.
fn fill(buffer: PWSTR, capacity: i32, text: &str) {
    let Ok(capacity) = usize::try_from(capacity) else {
        return;
    };
    if capacity == 0 || buffer.is_null() {
        return;
    }
    // SAFETY: the control lends `capacity` units at `buffer` for the length of the call.
    let slots = unsafe { std::slice::from_raw_parts_mut(buffer.0, capacity) };
    let mut written = 0;
    for (slot, unit) in slots.iter_mut().zip(text.encode_utf16().take(capacity - 1)) {
        *slot = unit;
        written += 1;
    }
    slots[written] = 0;
}

fn set_font(control: HWND, font: HFONT) {
    // SAFETY: WM_SETFONT takes the font handle by value; the fonts outlive the controls.
    unsafe {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0.addr())),
            Some(LPARAM(1)),
        )
    };
}

fn cue_banner(edit: HWND, hint: &str) {
    let hint = HSTRING::from(hint);
    // SAFETY: the edit copies the string during the call.
    unsafe {
        SendMessageW(
            edit,
            EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(hint.as_ptr() as isize)),
        )
    };
}

fn combo_add(combo: HWND, item: &str) {
    let item = HSTRING::from(item);
    // SAFETY: the combo box copies the string during the call.
    unsafe {
        SendMessageW(
            combo,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(item.as_ptr() as isize)),
        )
    };
}

fn combo_select(combo: HWND, index: usize) {
    // SAFETY: a message with value arguments.
    unsafe { SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(index)), Some(LPARAM(0))) };
}

fn combo_selected(combo: HWND) -> usize {
    // SAFETY: a message with value arguments.
    let index = unsafe { SendMessageW(combo, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) };
    usize::try_from(index.0).unwrap_or(usize::MAX)
}

fn set_list_style(list: HWND, style: u32) {
    // SAFETY: a message with value arguments.
    unsafe {
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(style as usize)),
            Some(LPARAM(isize::try_from(style).unwrap_or(0))),
        )
    };
}

fn insert_column(list: HWND, index: usize, title: &str, width: i32) {
    let title = HSTRING::from(title);
    let column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: width,
        pszText: PWSTR(title.as_ptr().cast_mut()),
        ..Default::default()
    };
    // SAFETY: the list copies the column and its title during the call.
    unsafe {
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(index)),
            Some(LPARAM(std::ptr::from_ref(&column) as isize)),
        )
    };
}

fn set_column_width(list: HWND, index: usize, width: i32) {
    // SAFETY: a message with value arguments.
    unsafe {
        SendMessageW(
            list,
            LVM_SETCOLUMNWIDTH,
            Some(WPARAM(index)),
            Some(LPARAM(width.max(0) as isize)),
        )
    };
}

/// Clears the selection, whose indices would point at other rows once some are gone.
fn deselect_all(list: HWND) {
    let item = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        ..Default::default()
    };
    // SAFETY: `item` outlives the synchronous message; -1 means every item.
    unsafe {
        SendMessageW(
            list,
            LVM_SETITEMSTATE,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(std::ptr::from_ref(&item) as isize)),
        )
    };
}

fn set_item_count(list: HWND, count: usize) {
    // SAFETY: a message with value arguments.
    unsafe {
        SendMessageW(
            list,
            LVM_SETITEMCOUNT,
            Some(WPARAM(count)),
            Some(LPARAM(isize::try_from(LVSICF_NOSCROLL).unwrap_or(0))),
        )
    };
}

fn ensure_visible(list: HWND, index: usize) {
    // SAFETY: a message with value arguments.
    unsafe {
        SendMessageW(
            list,
            LVM_ENSUREVISIBLE,
            Some(WPARAM(index)),
            Some(LPARAM(0)),
        )
    };
}

fn redraw_item(list: HWND, index: usize) {
    let index = isize::try_from(index).unwrap_or(isize::MAX);
    // SAFETY: a message with value arguments.
    unsafe {
        SendMessageW(
            list,
            LVM_REDRAWITEMS,
            Some(WPARAM(index.cast_unsigned())),
            Some(LPARAM(index)),
        )
    };
}

fn selected(list: HWND) -> Option<usize> {
    // SAFETY: a message with value arguments.
    let index = unsafe {
        SendMessageW(
            list,
            LVM_GETNEXTITEM,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(isize::try_from(LVNI_SELECTED).unwrap_or(0))),
        )
    };
    usize::try_from(index.0).ok()
}

/// Shows a menu at the mouse and returns the chosen item; an id of 0 is a separator.
fn popup(owner: HWND, items: &[(i32, &str, bool)]) -> Option<i32> {
    // SAFETY: the menu is created and destroyed here; the item strings outlive each call.
    unsafe {
        let menu: MENU = CreatePopupMenu().ok()?;
        for &(id, text, enabled) in items {
            if id == 0 {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                continue;
            }
            let flags = if enabled {
                MF_STRING
            } else {
                MF_STRING | MF_GRAYED
            };
            let text = HSTRING::from(text);
            let _ = AppendMenuW(menu, flags, usize::try_from(id).unwrap_or(0), &text);
        }
        let mut at = POINT::default();
        let _ = GetCursorPos(&raw mut at);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            at.x,
            at.y,
            None,
            owner,
            None,
        );
        let _ = DestroyMenu(menu);
        Some(chosen.0).filter(|&id| id > 0)
    }
}
