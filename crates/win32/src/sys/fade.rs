//! Fades for child windows: one alpha value on a timer that stops when the fade ends. Windows 8
//! layers child windows only for exes whose manifest names it.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, KillTimer, LWA_ALPHA, SPI_GETCLIENTAREAANIMATION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW,
    SystemParametersInfoW, WS_EX_LAYERED,
};
use windows::core::BOOL;

use crate::motion;

/// The fade's timer on the owner window.
pub(super) const TIMER: usize = 1;
const STEP_MS: u32 = 15;

#[derive(Default)]
pub(super) struct Fade {
    started: Cell<Option<u64>>,
    windows: RefCell<Vec<HWND>>,
}

impl Fade {
    /// Fades `windows` in, unless «Показывать анимацию в Windows» is off. Call it before they
    /// show, so they appear clear.
    pub(super) fn show(&self, owner: HWND, windows: &[HWND]) {
        self.finish(owner);
        if !animations_on() {
            return;
        }
        for &window in windows {
            layer(window, true);
            set_alpha(window, 0);
        }
        windows.clone_into(&mut self.windows.borrow_mut());
        // SAFETY: plain calls; the timer belongs to `owner`, a window of this thread.
        unsafe {
            self.started.set(Some(GetTickCount64()));
            SetTimer(Some(owner), TIMER, STEP_MS, None);
        }
    }

    /// One step on the timer.
    pub(super) fn tick(&self, owner: HWND) {
        let Some(started) = self.started.get() else {
            return self.finish(owner);
        };
        // SAFETY: a plain query.
        let opacity = motion::opacity(unsafe { GetTickCount64() }.saturating_sub(started));
        if opacity == u8::MAX {
            return self.finish(owner);
        }
        for &window in self.windows.borrow().iter() {
            set_alpha(window, opacity);
        }
    }

    /// Leaves every window opaque and unlayered, which brings back sharp text.
    fn finish(&self, owner: HWND) {
        self.started.set(None);
        // SAFETY: as in `show`.
        let _ = unsafe { KillTimer(Some(owner), TIMER) };
        for window in self.windows.borrow_mut().drain(..) {
            layer(window, false);
        }
    }
}

fn layer(window: HWND, on: bool) {
    let flag = isize::try_from(WS_EX_LAYERED.0).unwrap_or(0);
    // SAFETY: style changes on a child window of this thread; the repaint follows the change.
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let style = if on { style | flag } else { style & !flag };
        SetWindowLongPtrW(window, GWL_EXSTYLE, style);
        if !on {
            let _ = InvalidateRect(Some(window), None, true);
        }
    }
}

fn set_alpha(window: HWND, alpha: u8) {
    // SAFETY: as above.
    let _ = unsafe { SetLayeredWindowAttributes(window, COLORREF(0), alpha, LWA_ALPHA) };
}

fn animations_on() -> bool {
    let mut on = BOOL(1);
    // SAFETY: `on` is writable and outlives the call.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut on).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || on.as_bool()
}
