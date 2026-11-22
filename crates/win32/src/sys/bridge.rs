use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use pastegrab_app::ports::{Phase, Progress, View, ViewUpdate};
use pastegrab_domain::job::JobId;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Tells the window that updates wait for it.
pub(super) const WM_CORE: u32 = WM_APP + 1;

/// The core's updates for the window. They queue up here, and the window drains them on its own
/// thread; one posted message stands for any number of them.
#[derive(Debug, Default)]
pub struct WinView {
    window: AtomicUsize,
    updates: Mutex<Vec<ViewUpdate>>,
    progress: Mutex<Vec<(JobId, Phase, Progress)>>,
    posted: AtomicBool,
}

pub(super) type Drained = (Vec<ViewUpdate>, Vec<(JobId, Phase, Progress)>);

impl WinView {
    pub(super) fn attach(&self, window: HWND) {
        self.window.store(window.0.addr(), Ordering::Release);
        self.wake();
    }

    pub(super) fn drain(&self) -> Drained {
        self.posted.store(false, Ordering::Release);
        let updates = std::mem::take(&mut *lock(&self.updates));
        let progress = std::mem::take(&mut *lock(&self.progress));
        (updates, progress)
    }

    fn wake(&self) {
        let window = self.window.load(Ordering::Acquire);
        if window == 0 || self.posted.swap(true, Ordering::AcqRel) {
            return;
        }
        let hwnd = HWND(std::ptr::without_provenance_mut(window));
        // SAFETY: posting only queues a message; a window that is gone makes the call fail.
        let _ = unsafe { PostMessageW(Some(hwnd), WM_CORE, WPARAM(0), LPARAM(0)) };
    }
}

impl View for WinView {
    fn apply(&self, update: ViewUpdate) {
        lock(&self.updates).push(update);
        self.wake();
    }

    fn progress(&self, id: JobId, phase: Phase, progress: Progress) {
        {
            let mut latest = lock(&self.progress);
            match latest.iter_mut().find(|(job, ..)| *job == id) {
                Some(slot) => *slot = (id, phase, progress),
                None => latest.push((id, phase, progress)),
            }
        }
        self.wake();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
