//! The first-run guide panel and its target outline. Both exist as child windows only while the
//! guide runs, then fade out and destroy themselves.

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

use pastegrab_app::tutorial::{Target, Tutorial};
use pastegrab_i18n::RU;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_HIGHLIGHT, GetSysColorBrush, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::{SS_CENTERIMAGE, SS_ETCHEDHORZ, SS_NOPREFIX};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, HMENU,
    HTTRANSPARENT, HWND_TOP, MoveWindow, RegisterClassExW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SendMessageW, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_NCHITTEST, WM_SETFONT, WNDCLASSEXW, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::fade::{self, Fade};
use crate::cell::scale;
use crate::text;

/// Control ID of «Дальше» / «Готово».
pub(super) const ID_NEXT: i32 = 120;
/// Control ID of «Пропустить».
pub(super) const ID_SKIP: i32 = 121;
const ID_DIVIDER: i32 = 122;
const ID_BODY: i32 = 123;
const ID_STEP: i32 = 124;

const OUTLINE_CLASS: PCWSTR = w!("pastegrab_outline");
static OUTLINE_REGISTERED: OnceLock<()> = OnceLock::new();

struct Panel {
    divider: HWND,
    body: HWND,
    step: HWND,
    skip: HWND,
    next: HWND,
    outline: [HWND; 4],
}

impl Panel {
    fn create(parent: HWND, normal: HFONT, bold: HFONT) -> Self {
        register_outline();
        let tab = WS_TABSTOP.0;
        let divider = child(parent, w!("STATIC"), "", SS_ETCHEDHORZ.0, ID_DIVIDER, false);
        let body = child(parent, w!("STATIC"), "", SS_NOPREFIX.0, ID_BODY, false);
        let step = child(
            parent,
            w!("STATIC"),
            "",
            SS_NOPREFIX.0 | SS_CENTERIMAGE.0,
            ID_STEP,
            false,
        );
        let skip = child(
            parent,
            w!("BUTTON"),
            RU.tutorial.skip,
            tab | BS_PUSHBUTTON as u32,
            ID_SKIP,
            false,
        );
        let next = child(
            parent,
            w!("BUTTON"),
            RU.tutorial.next,
            tab | BS_DEFPUSHBUTTON as u32,
            ID_NEXT,
            false,
        );
        let outline = [
            outline_strip(parent),
            outline_strip(parent),
            outline_strip(parent),
            outline_strip(parent),
        ];
        let panel = Self {
            divider,
            body,
            step,
            skip,
            next,
            outline,
        };
        panel.apply_fonts(normal, bold);
        panel
    }

    fn apply_fonts(&self, normal: HFONT, bold: HFONT) {
        for control in [self.body, self.skip, self.next] {
            set_font(control, normal);
        }
        set_font(self.step, bold);
    }

    const fn controls(&self) -> [HWND; 5] {
        [self.divider, self.body, self.step, self.skip, self.next]
    }

    const fn all(&self) -> [HWND; 9] {
        [
            self.divider,
            self.body,
            self.step,
            self.skip,
            self.next,
            self.outline[0],
            self.outline[1],
            self.outline[2],
            self.outline[3],
        ]
    }

    fn destroy(self) {
        for control in self.all() {
            // SAFETY: each child belongs to this thread and is destroyed once.
            let _ = unsafe { DestroyWindow(control) };
        }
    }
}

/// Manages the tutorial presenter and its temporary child windows.
pub(super) struct Guide {
    presenter: RefCell<Tutorial>,
    panel: RefCell<Option<Panel>>,
    closing: Cell<bool>,
}

impl Guide {
    pub(super) const fn new(seen: u32) -> Self {
        Self {
            presenter: RefCell::new(Tutorial::for_seen(seen)),
            panel: RefCell::new(None),
            closing: Cell::new(false),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.presenter.borrow().is_open()
    }

    pub(super) fn height(&self, dpi: i32) -> i32 {
        if self.panel.borrow().is_some() {
            scale(98, dpi)
        } else {
            0
        }
    }

    pub(super) fn target(&self) -> Target {
        self.presenter
            .borrow()
            .current()
            .map_or(Target::None, |(step, _, _)| step.target)
    }

    pub(super) fn open_if_needed(&self, hwnd: HWND, normal: HFONT, bold: HFONT, fade: &Fade) {
        if !self.is_open() {
            return;
        }
        self.ensure_panel(hwnd, normal, bold, fade);
    }

    pub(super) fn replay(&self, hwnd: HWND, normal: HFONT, bold: HFONT, fade: &Fade) {
        fade.cancel(hwnd, fade::TUTORIAL);
        self.closing.set(false);
        self.presenter.borrow_mut().replay();
        self.ensure_panel(hwnd, normal, bold, fade);
        if let Some(panel) = self.panel.borrow().as_ref() {
            // SAFETY: `next` belongs to this thread.
            let _ = unsafe { SetFocus(Some(panel.next)) };
        }
    }

    /// Advances one step. Returns `true` when that finishes the guide.
    pub(super) fn next(&self, hwnd: HWND, fade: &Fade) -> bool {
        let finished = self.presenter.borrow_mut().next();
        if finished {
            self.begin_close(hwnd, fade);
        } else {
            self.refresh_text();
        }
        finished
    }

    /// Skips the guide. Returns `true` when it was open.
    pub(super) fn skip(&self, hwnd: HWND, fade: &Fade) -> bool {
        let closed = self.presenter.borrow_mut().skip();
        if closed {
            self.begin_close(hwnd, fade);
        }
        closed
    }

    /// Destroys the faded-out panel after its timer finishes.
    pub(super) fn finish_close(&self) -> bool {
        if !self.closing.replace(false) {
            return false;
        }
        if let Some(panel) = self.panel.borrow_mut().take() {
            panel.destroy();
        }
        true
    }

    /// Releases panel handles when the main window is being destroyed.
    pub(super) fn clear(&self) {
        self.closing.set(false);
        let _ = self.panel.borrow_mut().take();
    }

    pub(super) fn apply_fonts(&self, normal: HFONT, bold: HFONT) {
        if let Some(panel) = self.panel.borrow().as_ref() {
            panel.apply_fonts(normal, bold);
        }
    }

    pub(super) fn layout(&self, width: i32, top: i32, dpi: i32, target: Option<RECT>) {
        let panel = self.panel.borrow();
        let Some(panel) = panel.as_ref() else {
            return;
        };
        let px = |v: i32| scale(v, dpi);
        let (margin, gap, tall) = (px(16), px(8), px(32));
        let inner = width - 2 * margin;
        place(panel.divider, margin, top, inner, px(2).max(1));
        place(panel.body, margin, top + px(10), inner, px(44));
        let row = top + px(58);
        let next_w = px(110);
        let skip_w = px(124);
        place(panel.step, margin, row, px(160), tall);
        place(panel.next, width - margin - next_w, row, next_w, tall);
        place(
            panel.skip,
            width - margin - next_w - gap - skip_w,
            row,
            skip_w,
            tall,
        );
        if self.closing.get() {
            return;
        }
        match target {
            Some(rect) => place_outline(panel.outline, rect, dpi),
            None => {
                for strip in panel.outline {
                    show(strip, false);
                }
            }
        }
    }

    fn ensure_panel(&self, hwnd: HWND, normal: HFONT, bold: HFONT, fade: &Fade) {
        let created = {
            let mut slot = self.panel.borrow_mut();
            if slot.is_none() {
                *slot = Some(Panel::create(hwnd, normal, bold));
                true
            } else {
                false
            }
        };
        self.refresh_text();
        if let Some(panel) = self.panel.borrow().as_ref() {
            let controls = panel.controls();
            if created {
                fade.show(hwnd, fade::TUTORIAL, &controls);
            }
            for control in controls {
                show(control, true);
            }
        }
    }

    fn refresh_text(&self) {
        let Some((step, number, total)) = self.presenter.borrow().current() else {
            return;
        };
        let last = self.presenter.borrow().is_last();
        let panel = self.panel.borrow();
        let Some(panel) = panel.as_ref() else {
            return;
        };
        set_text(panel.body, text::tutorial_prompt(step.prompt, &RU));
        set_text(panel.step, &(RU.tutorial.step)(number, total));
        let label = if last {
            RU.tutorial.done
        } else {
            RU.tutorial.next
        };
        set_text(panel.next, label);
    }

    fn begin_close(&self, hwnd: HWND, fade: &Fade) {
        let handles = self.panel.borrow().as_ref().map(Panel::all);
        let Some(handles) = handles else {
            return;
        };
        if fade.hide(hwnd, fade::TUTORIAL, &handles) {
            self.closing.set(true);
        } else if let Some(panel) = self.panel.borrow_mut().take() {
            panel.destroy();
        }
    }
}

fn register_outline() {
    OUTLINE_REGISTERED.get_or_init(|| {
        // SAFETY: registers one class for the process; system brush needs no release.
        unsafe {
            let instance = GetModuleHandleW(None).map_or_default(|h| HINSTANCE(h.0));
            let wc = WNDCLASSEXW {
                cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
                lpfnWndProc: Some(outline_proc),
                hInstance: instance,
                hbrBackground: GetSysColorBrush(COLOR_HIGHLIGHT),
                lpszClassName: OUTLINE_CLASS,
                ..Default::default()
            };
            let _ = RegisterClassExW(&raw const wc);
        }
    });
}

unsafe extern "system" fn outline_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    // SAFETY: forwards every other message to Windows.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn outline_strip(parent: HWND) -> HWND {
    // SAFETY: `parent` belongs to this thread and the class name is static.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            OUTLINE_CLASS,
            PCWSTR::null(),
            WS_CHILD,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            None,
            None,
        )
    }
    .unwrap_or_default()
}

fn place_outline(strips: [HWND; 4], target: RECT, dpi: i32) {
    let pad = scale(2, dpi);
    let thick = scale(2, dpi).max(1);
    let left = target.left - pad - thick;
    let top = target.top - pad - thick;
    let right = target.right + pad + thick;
    let bottom = target.bottom + pad + thick;
    let boxes = [
        (left, top, right - left, thick),
        (left, bottom - thick, right - left, thick),
        (left, top + thick, thick, bottom - top - 2 * thick),
        (right - thick, top + thick, thick, bottom - top - 2 * thick),
    ];
    for (strip, (x, y, w, h)) in strips.into_iter().zip(boxes) {
        // SAFETY: child window of this thread, lifted above siblings without taking focus.
        let _ = unsafe {
            SetWindowPos(
                strip,
                Some(HWND_TOP),
                x,
                y,
                w.max(0),
                h.max(0),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        };
    }
}

fn child(parent: HWND, class: PCWSTR, text: &str, style: u32, id: i32, visible: bool) -> HWND {
    let text = HSTRING::from(text);
    let vis = if visible { WS_VISIBLE.0 } else { 0 };
    let menu = HMENU(std::ptr::without_provenance_mut(
        usize::try_from(id).unwrap_or(0),
    ));
    // SAFETY: `parent` belongs to this thread and the strings outlive the call.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            &text,
            WINDOW_STYLE(WS_CHILD.0 | vis | style),
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

fn place(control: HWND, x: i32, y: i32, width: i32, height: i32) {
    // SAFETY: a plain call on this thread's control.
    let _ = unsafe { MoveWindow(control, x, y, width.max(0), height.max(0), true) };
}

fn show(control: HWND, visible: bool) {
    // SAFETY: a plain call on this thread's control.
    let _ = unsafe { ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE }) };
}

fn set_text(control: HWND, text: &str) {
    let text = HSTRING::from(text);
    // SAFETY: the string outlives the call.
    let _ = unsafe { SetWindowTextW(control, &text) };
}

fn set_font(control: HWND, font: HFONT) {
    // SAFETY: `font` outlives every control.
    unsafe {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0.addr())),
            Some(LPARAM(1)),
        )
    };
}
