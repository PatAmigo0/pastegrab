//! Fades for child windows: one alpha value per track on a timer that stops when every fade ends.
//! Windows 8 layers child windows only for exes whose manifest names it.

use std::cell::RefCell;

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
/// Tag bit for the preview card's fade.
pub(super) const CARD: u8 = 1 << 0;
/// Tag bit for the tutorial panel's fade.
pub(super) const TUTORIAL: u8 = 1 << 1;
const STEP_MS: u32 = 15;

struct Track {
    tag: u8,
    started: u64,
    fade_in: bool,
    windows: Vec<HWND>,
}

#[derive(Default)]
pub(super) struct Fade {
    tracks: RefCell<Vec<Track>>,
}

impl Fade {
    /// Fades `windows` in under `tag`, unless «Показывать анимацию в Windows» is off. Call it
    /// before they show, so they appear clear.
    pub(super) fn show(&self, owner: HWND, tag: u8, windows: &[HWND]) {
        self.cancel(owner, tag);
        if !animations_on() || windows.is_empty() {
            return;
        }
        for &window in windows {
            layer(window, true);
            set_alpha(window, 0);
        }
        // SAFETY: a plain query.
        let started = unsafe { GetTickCount64() };
        self.tracks.borrow_mut().push(Track {
            tag,
            started,
            fade_in: true,
            windows: windows.to_vec(),
        });
        // SAFETY: the timer belongs to `owner`, a window of this thread.
        unsafe { SetTimer(Some(owner), TIMER, STEP_MS, None) };
    }

    /// Fades `windows` out under `tag`. Returns `true` when the fade is running and the caller
    /// should wait for `tick` to report `tag` before hiding or destroying them.
    pub(super) fn hide(&self, owner: HWND, tag: u8, windows: &[HWND]) -> bool {
        self.cancel(owner, tag);
        if !animations_on() || windows.is_empty() {
            return false;
        }
        for &window in windows {
            layer(window, true);
            set_alpha(window, u8::MAX);
        }
        // SAFETY: a plain query.
        let started = unsafe { GetTickCount64() };
        self.tracks.borrow_mut().push(Track {
            tag,
            started,
            fade_in: false,
            windows: windows.to_vec(),
        });
        // SAFETY: as in `show`.
        unsafe { SetTimer(Some(owner), TIMER, STEP_MS, None) };
        true
    }

    /// Stops any active fade for `tag` and leaves its windows unlayered.
    pub(super) fn cancel(&self, owner: HWND, tag: u8) {
        let (removed, empty) = {
            let mut tracks = self.tracks.borrow_mut();
            let mut removed = Vec::new();
            let mut i = 0;
            while i < tracks.len() {
                if tracks[i].tag == tag {
                    removed.push(tracks.swap_remove(i));
                } else {
                    i += 1;
                }
            }
            (removed, tracks.is_empty())
        };
        if empty {
            // SAFETY: as in `show`.
            let _ = unsafe { KillTimer(Some(owner), TIMER) };
        }
        for track in removed {
            for window in track.windows {
                layer(window, false);
            }
        }
    }

    /// One step on the timer. Returns a bitmask of tags whose fade-out just reached clear.
    pub(super) fn tick(&self, owner: HWND) -> u8 {
        // SAFETY: a plain query.
        let now = unsafe { GetTickCount64() };
        let mut unlayer = Vec::new();
        let mut hidden = 0_u8;
        let empty = {
            let mut tracks = self.tracks.borrow_mut();
            let mut i = 0;
            while i < tracks.len() {
                let elapsed = now.saturating_sub(tracks[i].started);
                let alpha = if tracks[i].fade_in {
                    motion::opacity(elapsed)
                } else {
                    motion::opacity_out(elapsed)
                };
                let done = if tracks[i].fade_in {
                    alpha == u8::MAX
                } else {
                    alpha == 0
                };
                if done {
                    let track = tracks.swap_remove(i);
                    if track.fade_in {
                        unlayer.extend(track.windows);
                    } else {
                        for &window in &track.windows {
                            set_alpha(window, 0);
                        }
                        hidden |= track.tag;
                    }
                } else {
                    for &window in &tracks[i].windows {
                        set_alpha(window, alpha);
                    }
                    i += 1;
                }
            }
            tracks.is_empty()
        };
        if empty {
            // SAFETY: as in `show`.
            let _ = unsafe { KillTimer(Some(owner), TIMER) };
        }
        for window in unlayer {
            layer(window, false);
        }
        hidden
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

pub(super) fn animations_on() -> bool {
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
