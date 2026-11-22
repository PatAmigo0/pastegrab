//! The job list's custom painting, inline actions and row fades. The list still hands Narrator
//! the plain text.

use std::cell::RefCell;

use pastegrab_app::board::Row;
use pastegrab_domain::job::{JobId, JobState};
use pastegrab_i18n::RU;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_BTNSHADOW, COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT,
    COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateSolidBrush, DT_CALCRECT, DT_CENTER,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, FillRect,
    FrameRect, GetDC, GetSysColor, GetSysColorBrush, HDC, HFONT, ReleaseDC, SYS_COLOR_INDEX,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT, CDRF_NEWFONT,
    CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, CDRF_SKIPDEFAULT, CloseThemeData,
    DrawThemeBackground, GetThemeBackgroundContentRect, HIMAGELIST, ILC_COLOR32, ImageList_Create,
    ImageList_Destroy, LVIR_BOUNDS, LVIS_SELECTED, LVM_GETITEMSTATE, LVM_GETSUBITEMRECT,
    LVM_REDRAWITEMS, LVM_SETIMAGELIST, LVSIL_SMALL, NMLVCUSTOMDRAW, OpenThemeData, PBFS_NORMAL,
    PP_BAR, PP_FILL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SetTimer,
    SystemParametersInfoW,
};
use windows::core::w;

use super::fade::animations_on;
use crate::cell::{self, Rect, RowAction, scale};
use crate::{motion, text};

const STATE_COLUMN: i32 = 1;
/// Red 160, green and blue 40: muted, and 7:1 against white.
const FAILURE_RED: COLORREF = COLORREF(0x0028_28A0);
/// The row-fade timer on the owner window.
pub(super) const TIMER: usize = 2;
const STEP_MS: u32 = 15;
const CANCEL_GLYPH: &str = "×";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeKind {
    Row,
    Buttons,
}

#[derive(Debug, Clone, Copy)]
struct RowFade {
    id: JobId,
    kind: FadeKind,
    started: u64,
}

/// Active fades for newly added rows and newly revealed finished-job buttons.
#[derive(Default)]
pub(super) struct RowFades {
    entries: RefCell<Vec<RowFade>>,
}

impl RowFades {
    pub(super) fn add(&self, owner: HWND, id: JobId) {
        self.start(owner, id, FadeKind::Row);
    }

    pub(super) fn finish(&self, owner: HWND, id: JobId) {
        self.start(owner, id, FadeKind::Buttons);
    }

    fn start(&self, owner: HWND, id: JobId, kind: FadeKind) {
        if !animations_on() {
            return;
        }
        // SAFETY: a plain query.
        let now = unsafe { GetTickCount64() };
        let mut entries = self.entries.borrow_mut();
        entries.retain(|entry| !(entry.id == id && entry.kind == kind));
        entries.push(RowFade {
            id,
            kind,
            started: now,
        });
        drop(entries);
        // SAFETY: the timer belongs to `owner`, a window of this thread.
        unsafe { SetTimer(Some(owner), TIMER, STEP_MS, None) };
    }

    pub(super) fn alphas(&self, id: JobId, now: u64) -> (u8, u8) {
        let entries = self.entries.borrow();
        let mut row = u8::MAX;
        let mut buttons = u8::MAX;
        for entry in entries.iter().filter(|entry| entry.id == id) {
            let alpha = motion::opacity(now.saturating_sub(entry.started));
            match entry.kind {
                FadeKind::Row => row = alpha,
                FadeKind::Buttons => buttons = alpha,
            }
        }
        (row, buttons)
    }

    pub(super) fn tick(&self, owner: HWND, list: HWND, rows: &[Row]) {
        // SAFETY: a plain query.
        let now = unsafe { GetTickCount64() };
        let (indices, empty) = {
            let mut entries = self.entries.borrow_mut();
            let mut indices: Vec<usize> = entries
                .iter()
                .filter_map(|entry| rows.iter().position(|row| row.id == entry.id))
                .collect();
            indices.sort_unstable();
            indices.dedup();
            entries.retain(|entry| {
                now.saturating_sub(entry.started) < motion::FADE_MS
                    && rows.iter().any(|row| row.id == entry.id)
            });
            (indices, entries.is_empty())
        };
        if empty {
            // SAFETY: the timer belongs to `owner`, a window of this thread.
            let _ = unsafe { KillTimer(Some(owner), TIMER) };
        }
        for index in indices {
            redraw_item(list, index);
        }
    }
}

/// An empty 1-px image list that sets the list view's row height at the current DPI.
pub(super) struct RowHeight(HIMAGELIST);

impl RowHeight {
    pub(super) fn apply(list: HWND, dpi: i32) -> Option<Self> {
        let height = scale(28, dpi).max(1);
        // SAFETY: `LVS_SHAREIMAGELISTS` keeps the list view from destroying this handle, so
        // `Drop` owns it alone.
        let handle = unsafe { ImageList_Create(1, height, ILC_COLOR32, 1, 0) };
        if handle.is_invalid() {
            return None;
        }
        // SAFETY: assigns our image list to a list view of this thread.
        unsafe {
            SendMessageW(
                list,
                LVM_SETIMAGELIST,
                Some(WPARAM(usize::try_from(LVSIL_SMALL).unwrap_or(0))),
                Some(LPARAM(handle.0)),
            );
        }
        Some(Self(handle))
    }
}

impl Drop for RowHeight {
    fn drop(&mut self) {
        // SAFETY: this wrapper is the sole owner of the image list handle.
        let _ = unsafe { ImageList_Destroy(Some(self.0)) };
    }
}

/// Pixel widths of the finished-row button labels in `font`.
pub(super) fn measure_buttons(list: HWND, font: HFONT) -> (i32, i32) {
    // SAFETY: the DC is restored and released before returning.
    unsafe {
        let dc = GetDC(Some(list));
        if dc.is_invalid() {
            return (64, 48);
        }
        let prev = SelectObject(dc, font.into());
        let open = text_width(dc, RU.row_actions.open);
        let folder = text_width(dc, RU.row_actions.folder);
        SelectObject(dc, prev);
        ReleaseDC(Some(list), dc);
        (open, folder)
    }
}

fn text_width(dc: HDC, text: &str) -> i32 {
    let mut rect = RECT::default();
    let mut units: Vec<u16> = text.encode_utf16().collect();
    // SAFETY: `dc` is valid and `units` and `rect` outlive the call.
    unsafe {
        DrawTextW(
            dc,
            &mut units,
            &raw mut rect,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
    rect.right - rect.left
}

/// Which inline action lies under `point` on row `index`, if any.
pub(super) fn hit(
    list: HWND,
    row: &Row,
    index: usize,
    point: POINT,
    dpi: i32,
    button_widths: (i32, i32),
) -> Option<RowAction> {
    let rect = cell(list, index)?;
    cell::hit(
        row,
        from_win(rect),
        point.x,
        point.y,
        dpi,
        button_widths.0,
        button_widths.1,
    )
}

/// Answers the list's custom draw notification.
pub(super) fn paint(
    list: HWND,
    draw: &mut NMLVCUSTOMDRAW,
    rows: &[Row],
    fades: &RowFades,
    dpi: i32,
    button_widths: (i32, i32),
) -> LRESULT {
    let stage = draw.nmcd.dwDrawStage;
    let answer = if stage == CDDS_PREPAINT {
        CDRF_NOTIFYITEMDRAW
    } else if stage == CDDS_ITEMPREPAINT {
        CDRF_NOTIFYSUBITEMDRAW
    } else if stage.0 == CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 {
        subitem(list, draw, rows, fades, dpi, button_widths)
    } else {
        CDRF_DODEFAULT
    };
    LRESULT(isize::try_from(answer).unwrap_or(0))
}

fn subitem(
    list: HWND,
    draw: &mut NMLVCUSTOMDRAW,
    rows: &[Row],
    fades: &RowFades,
    dpi: i32,
    button_widths: (i32, i32),
) -> u32 {
    let index = draw.nmcd.dwItemSpec;
    let Some(row) = rows.get(index) else {
        return CDRF_DODEFAULT;
    };
    // SAFETY: a plain query.
    let now = unsafe { GetTickCount64() };
    let (row_alpha, buttons_alpha) = fades.alphas(row.id, now);
    let selected = is_selected(list, index);
    let (back, fore) = colors(list, selected);
    if draw.iSubItem == 0 {
        if row_alpha < u8::MAX {
            draw.clrText = COLORREF(motion::blend(sys_color(back), sys_color(fore), row_alpha));
            return CDRF_NEWFONT;
        }
        return CDRF_DODEFAULT;
    }
    if draw.iSubItem != STATE_COLUMN {
        return CDRF_DODEFAULT;
    }
    let Some(rect) = cell(list, index) else {
        return CDRF_DODEFAULT;
    };
    let canvas = Canvas {
        hdc: draw.nmcd.hdc,
        cell: rect,
        dpi,
    };
    if !row.state.is_over() {
        canvas.cancellable(
            list,
            &text::state(row, &RU),
            row.permille(),
            selected,
            row_alpha,
        );
        return CDRF_SKIPDEFAULT;
    }
    if row.state == JobState::Done && row.file.is_some() {
        canvas.done(
            list,
            &text::state(row, &RU),
            selected,
            row_alpha,
            buttons_alpha.min(row_alpha),
            button_widths,
        );
        return CDRF_SKIPDEFAULT;
    }
    if matches!(row.state, JobState::Failed(_)) {
        let target = if !selected && !high_contrast() {
            FAILURE_RED.0
        } else {
            sys_color(fore)
        };
        let color = if row_alpha < u8::MAX {
            motion::blend(sys_color(back), target, row_alpha)
        } else {
            target
        };
        draw.clrText = COLORREF(color);
        return CDRF_NEWFONT;
    }
    if row_alpha < u8::MAX {
        draw.clrText = COLORREF(motion::blend(sys_color(back), sys_color(fore), row_alpha));
        return CDRF_NEWFONT;
    }
    CDRF_DODEFAULT
}

/// One cell of the state column and the context to draw it in.
struct Canvas {
    hdc: HDC,
    cell: RECT,
    dpi: i32,
}

impl Canvas {
    fn cancellable(&self, list: HWND, state: &str, share: Option<u16>, selected: bool, alpha: u8) {
        let (back, fore) = colors(list, selected);
        let (bg, fg) = (sys_color(back), sys_color(fore));
        let layout = cell::cancel_layout(from_win(self.cell), self.dpi, share.is_some());
        let mut label = to_win(layout.label);
        let mut cancel = to_win(layout.cancel);
        let mut units: Vec<u16> = state.encode_utf16().collect();
        let mut cross: Vec<u16> = CANCEL_GLYPH.encode_utf16().collect();
        let cross_target = if selected || high_contrast() {
            fg
        } else {
            sys_color(COLOR_GRAYTEXT)
        };
        // SAFETY: the list's device context is valid for this notification; the rectangles and
        // the text outlive the calls.
        unsafe {
            FillRect(self.hdc, &raw const self.cell, GetSysColorBrush(back));
            if let (Some(bar), Some(share)) = (layout.bar, share) {
                self.bar(list, to_win(bar), share);
            }
            SetBkMode(self.hdc, TRANSPARENT);
            SetTextColor(self.hdc, COLORREF(motion::blend(bg, fg, alpha)));
            let flags = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX;
            DrawTextW(self.hdc, &mut units, &raw mut label, flags);
            SetTextColor(self.hdc, COLORREF(motion::blend(bg, cross_target, alpha)));
            DrawTextW(
                self.hdc,
                &mut cross,
                &raw mut cancel,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );
        }
    }

    fn done(
        &self,
        list: HWND,
        state: &str,
        selected: bool,
        row_alpha: u8,
        buttons_alpha: u8,
        button_widths: (i32, i32),
    ) {
        let (back, fore) = colors(list, selected);
        let (bg, fg) = (sys_color(back), sys_color(fore));
        let layout = cell::done_layout(
            from_win(self.cell),
            self.dpi,
            button_widths.0,
            button_widths.1,
        );
        let mut label = to_win(layout.label);
        let mut units: Vec<u16> = state.encode_utf16().collect();
        // SAFETY: the list's device context is valid for this notification; the rectangles and
        // the text outlive the calls.
        unsafe {
            FillRect(self.hdc, &raw const self.cell, GetSysColorBrush(back));
            SetBkMode(self.hdc, TRANSPARENT);
            SetTextColor(self.hdc, COLORREF(motion::blend(bg, fg, row_alpha)));
            let flags = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX;
            DrawTextW(self.hdc, &mut units, &raw mut label, flags);
        }
        if let Some((open, folder)) = layout.buttons {
            self.button(to_win(open), RU.row_actions.open, bg, buttons_alpha, true);
            self.button(
                to_win(folder),
                RU.row_actions.folder,
                bg,
                buttons_alpha,
                false,
            );
        }
    }

    fn button(&self, mut rect: RECT, label: &str, bg: u32, alpha: u8, primary: bool) {
        if alpha == 0 {
            return;
        }
        let fill_color = COLORREF(motion::blend(bg, sys_color(COLOR_BTNFACE), alpha));
        let border_target = if primary && !high_contrast() {
            sys_color(COLOR_HIGHLIGHT)
        } else {
            sys_color(COLOR_BTNSHADOW)
        };
        let border_color = COLORREF(motion::blend(bg, border_target, alpha));
        let text_color = COLORREF(motion::blend(bg, sys_color(COLOR_BTNTEXT), alpha));
        let mut units: Vec<u16> = label.encode_utf16().collect();
        // SAFETY: both GDI brushes are deleted before returning, and `rect` and `units` outlive
        // the calls.
        unsafe {
            let fill = CreateSolidBrush(fill_color);
            FillRect(self.hdc, &raw const rect, fill);
            let _ = DeleteObject(fill.into());
            let border = CreateSolidBrush(border_color);
            FrameRect(self.hdc, &raw const rect, border);
            let _ = DeleteObject(border.into());
            SetBkMode(self.hdc, TRANSPARENT);
            SetTextColor(self.hdc, text_color);
            DrawTextW(
                self.hdc,
                &mut units,
                &raw mut rect,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );
        }
    }

    /// The visual style's progress bar, or a framed fill in system colors without one.
    fn bar(&self, list: HWND, bar: RECT, share: u16) {
        let filled = |inner: RECT| RECT {
            right: inner.left + (inner.right - inner.left) * i32::from(share) / 1000,
            ..inner
        };
        // SAFETY: as in `cancellable`; the theme handle is closed before returning.
        unsafe {
            let theme = OpenThemeData(Some(list), w!("PROGRESS"));
            if theme.is_invalid() {
                FrameRect(self.hdc, &raw const bar, GetSysColorBrush(COLOR_BTNSHADOW));
                let inner = RECT {
                    left: bar.left + 1,
                    top: bar.top + 1,
                    right: bar.right - 1,
                    bottom: bar.bottom - 1,
                };
                let part = filled(inner);
                FillRect(self.hdc, &raw const part, GetSysColorBrush(COLOR_HIGHLIGHT));
                return;
            }
            let _ = DrawThemeBackground(theme, self.hdc, PP_BAR.0, 0, &raw const bar, None);
            let inner =
                GetThemeBackgroundContentRect(theme, Some(self.hdc), PP_BAR.0, 0, &raw const bar)
                    .unwrap_or(bar);
            let part = filled(inner);
            let _ = DrawThemeBackground(
                theme,
                self.hdc,
                PP_FILL.0,
                PBFS_NORMAL.0,
                &raw const part,
                None,
            );
            let _ = CloseThemeData(theme);
        }
    }
}

fn sys_color(index: SYS_COLOR_INDEX) -> u32 {
    // SAFETY: a plain query.
    unsafe { GetSysColor(index) }
}

const fn from_win(rect: RECT) -> Rect {
    Rect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

const fn to_win(rect: Rect) -> RECT {
    RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
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

/// The selection's colors as the list paints them: strong with focus, faint without.
fn colors(list: HWND, selected: bool) -> (SYS_COLOR_INDEX, SYS_COLOR_INDEX) {
    // SAFETY: a plain query.
    let focused = unsafe { GetFocus() } == list;
    match (selected, focused) {
        (true, true) => (COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT),
        (true, false) => (COLOR_BTNFACE, COLOR_WINDOWTEXT),
        (false, _) => (COLOR_WINDOW, COLOR_WINDOWTEXT),
    }
}

fn cell(list: HWND, index: usize) -> Option<RECT> {
    let mut rect = RECT {
        top: STATE_COLUMN,
        left: i32::try_from(LVIR_BOUNDS).ok()?,
        ..RECT::default()
    };
    // SAFETY: `rect` is writable and outlives the synchronous message.
    let found = unsafe {
        SendMessageW(
            list,
            LVM_GETSUBITEMRECT,
            Some(WPARAM(index)),
            Some(LPARAM(std::ptr::from_mut(&mut rect) as isize)),
        )
    };
    (found.0 != 0).then_some(rect)
}

fn is_selected(list: HWND, index: usize) -> bool {
    let mask = usize::try_from(LVIS_SELECTED.0).unwrap_or(0);
    // SAFETY: a plain message with value arguments.
    let state = unsafe {
        SendMessageW(
            list,
            LVM_GETITEMSTATE,
            Some(WPARAM(index)),
            Some(LPARAM(mask.cast_signed())),
        )
    };
    state.0.cast_unsigned() & mask != 0
}

fn high_contrast() -> bool {
    let mut contrast = HIGHCONTRASTW {
        cbSize: u32::try_from(size_of::<HIGHCONTRASTW>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `contrast` is writable, its size is set, and it outlives the call.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&raw mut contrast).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && contrast.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
}
