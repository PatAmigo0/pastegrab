//! The taskbar button's progress and the PC's sleep timer, both following the board.

use std::cell::{Cell, RefCell};

use pastegrab_app::board::Activity;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Power::{ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
use windows::Win32::UI::Shell::{ITaskbarList3, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TaskbarList};
use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
use windows::core::w;

pub(super) struct Taskbar {
    created: u32,
    list: RefCell<Option<ITaskbarList3>>,
    shown: Cell<Option<Activity>>,
}

impl Taskbar {
    pub(super) fn new() -> Self {
        // SAFETY: registers a constant name; nothing is kept.
        let created = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
        Self {
            created,
            list: RefCell::default(),
            shown: Cell::new(None),
        }
    }

    /// The message Explorer sends once the button exists, and again after Explorer restarts.
    pub(super) const fn created(&self) -> u32 {
        self.created
    }

    pub(super) fn attach(&self, hwnd: HWND) {
        // SAFETY: `run` initialized COM on this thread.
        let list: Option<ITaskbarList3> =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }.ok();
        // SAFETY: as above.
        *self.list.borrow_mut() = list.filter(|list| unsafe { list.HrInit() }.is_ok());
        if let Some(activity) = self.shown.take() {
            self.show(hwnd, activity);
        }
    }

    pub(super) fn show(&self, hwnd: HWND, activity: Activity) {
        let before = self.shown.replace(Some(activity));
        if before == Some(activity) {
            return;
        }
        let busy = activity != Activity::Idle;
        if before.is_none_or(|before| (before != Activity::Idle) != busy) {
            let awake = if busy {
                ES_CONTINUOUS | ES_SYSTEM_REQUIRED
            } else {
                ES_CONTINUOUS
            };
            // SAFETY: a flag on this thread, which lives as long as the window.
            unsafe { SetThreadExecutionState(awake) };
        }
        let list = self.list.borrow();
        let Some(list) = list.as_ref() else {
            return;
        };
        // SAFETY: the window is ours, and the list stays alive while borrowed.
        let _ = unsafe {
            match activity {
                Activity::Idle => list.SetProgressState(hwnd, TBPF_NOPROGRESS),
                Activity::Busy => list.SetProgressState(hwnd, TBPF_INDETERMINATE),
                Activity::Permille(done) => list.SetProgressValue(hwnd, u64::from(done), 1000),
            }
        };
    }
}
