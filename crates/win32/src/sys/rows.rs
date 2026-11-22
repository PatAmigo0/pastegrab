//! The state column's painting: a progress bar in the system's style beside a running step's
//! state, and failures in a muted red. The list still hands Narrator the plain text.

use pastegrab_app::board::Row;
use pastegrab_domain::job::JobState;
use pastegrab_i18n::RU;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_BTNSHADOW, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW,
    COLOR_WINDOWTEXT, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DrawTextW, FillRect,
    FrameRect, GetSysColor, GetSysColorBrush, HDC, SYS_COLOR_INDEX, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT, CDRF_NEWFONT,
    CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, CDRF_SKIPDEFAULT, CloseThemeData,
    DrawThemeBackground, GetThemeBackgroundContentRect, LVIR_BOUNDS, LVIS_SELECTED,
    LVM_GETITEMSTATE, LVM_GETSUBITEMRECT, NMLVCUSTOMDRAW, OpenThemeData, PBFS_NORMAL, PP_BAR,
    PP_FILL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SystemParametersInfoW,
};
use windows::core::w;

use super::dpi::scale;
use crate::text;

const STATE_COLUMN: i32 = 1;
/// Red 160, green and blue 40: muted, and 7:1 against white.
const FAILURE_RED: COLORREF = COLORREF(0x0028_28A0);

/// Answers the list's custom draw notification.
pub(super) fn paint(list: HWND, draw: &mut NMLVCUSTOMDRAW, rows: &[Row], dpi: i32) -> LRESULT {
    let stage = draw.nmcd.dwDrawStage;
    let answer = if stage == CDDS_PREPAINT {
        CDRF_NOTIFYITEMDRAW
    } else if stage == CDDS_ITEMPREPAINT {
        CDRF_NOTIFYSUBITEMDRAW
    } else if stage.0 == CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 && draw.iSubItem == STATE_COLUMN {
        state_cell(list, draw, rows, dpi)
    } else {
        CDRF_DODEFAULT
    };
    LRESULT(isize::try_from(answer).unwrap_or(0))
}

fn state_cell(list: HWND, draw: &mut NMLVCUSTOMDRAW, rows: &[Row], dpi: i32) -> u32 {
    let index = draw.nmcd.dwItemSpec;
    let Some(row) = rows.get(index) else {
        return CDRF_DODEFAULT;
    };
    let Some(cell) = cell(list, index) else {
        return CDRF_DODEFAULT;
    };
    let selected = is_selected(list, index);
    if let Some(share) = row.permille() {
        let canvas = Canvas {
            hdc: draw.nmcd.hdc,
            cell,
            dpi,
        };
        canvas.progress(list, &text::state(row, &RU), share, selected);
        return CDRF_SKIPDEFAULT;
    }
    if matches!(row.state, JobState::Failed(_)) && !selected && !high_contrast() {
        draw.clrText = FAILURE_RED;
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
    fn progress(&self, list: HWND, state: &str, share: u16, selected: bool) {
        let (back, fore) = colors(list, selected);
        let (pad, inset) = (scale(6, self.dpi), scale(4, self.dpi));
        let width = scale(96, self.dpi).min((self.cell.right - self.cell.left) / 3);
        let bar = RECT {
            left: self.cell.left + pad,
            top: self.cell.top + inset,
            right: self.cell.left + pad + width,
            bottom: self.cell.bottom - inset,
        };
        let mut label = RECT {
            left: bar.right + pad,
            right: self.cell.right - pad,
            ..self.cell
        };
        let mut units: Vec<u16> = state.encode_utf16().collect();
        // SAFETY: the list's device context is valid for this notification; the rectangles and
        // the text outlive the calls.
        unsafe {
            FillRect(self.hdc, &raw const self.cell, GetSysColorBrush(back));
            self.bar(list, bar, share);
            SetTextColor(self.hdc, COLORREF(GetSysColor(fore)));
            SetBkMode(self.hdc, TRANSPARENT);
            let flags = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX;
            DrawTextW(self.hdc, &mut units, &raw mut label, flags);
        }
    }

    /// The visual style's progress bar, or a framed fill in system colors without one.
    fn bar(&self, list: HWND, bar: RECT, share: u16) {
        let filled = |inner: RECT| RECT {
            right: inner.left + (inner.right - inner.left) * i32::from(share) / 1000,
            ..inner
        };
        // SAFETY: as in `progress`; the theme handle is closed before returning.
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
