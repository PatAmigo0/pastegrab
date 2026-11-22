//! The installer's three pages in one Windows task dialog: the question, the progress while
//! files unpack and the tools get tested, and the outcome with its report, which can start the
//! program.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use pastegrab_i18n::RU;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HWND, LPARAM, S_FALSE, S_OK, WPARAM};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOG_FLAGS, TASKDIALOG_NOTIFICATIONS,
    TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDCBF_CLOSE_BUTTON, TDE_CONTENT,
    TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDF_SHOW_PROGRESS_BAR, TDM_ENABLE_BUTTON,
    TDM_NAVIGATE_PAGE, TDM_SET_ELEMENT_TEXT, TDM_SET_MARQUEE_PROGRESS_BAR,
    TDM_SET_PROGRESS_BAR_MARQUEE, TDM_SET_PROGRESS_BAR_POS, TDM_SET_PROGRESS_BAR_RANGE,
    TDN_BUTTON_CLICKED, TDN_NAVIGATED, TDN_TIMER, TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDCANCEL, MB_ICONERROR, MB_OK, MessageBoxW, SendMessageW,
};
use windows::core::{BOOL, HRESULT, HSTRING, PCWSTR};

use crate::work::{self, Finished, Job, UNPACKING};

const INSTALL: i32 = 100;
const OPEN: i32 = 101;
const STEPS: usize = 1000;

/// Checks the PC, then asks, installs and reports.
pub fn run(payload: &'static [u8], redist: &'static [u8], unpacked: u64) {
    let text = &RU.setup;
    if !is_64_bit() {
        return message(text.needs_64_bit);
    }
    if !is_windows_8_or_later() {
        return message(text.needs_windows_8);
    }
    if payload.is_empty() {
        return message(text.empty);
    }
    let Some(dest) = pastegrab_deploy::install_folder() else {
        return message(text.failed);
    };
    let folder = dest.display().to_string();
    let dialog = Dialog {
        payload,
        redist,
        unpacked,
        dest,
        shared: Arc::default(),
        page: Cell::new(Page::Ask),
        step: Cell::new(UNPACKING),
        title: text.title.into(),
        ask: text.ask.into(),
        where_to: (text.where_to)(&(RU.size)(unpacked), &folder).into(),
        install: text.install.into(),
        installing: text.installing.into(),
        steps: [
            text.unpacking.into(),
            text.runtime.into(),
            text.checking.into(),
        ],
        done: text.done.into(),
        done_broken: text.done_broken.into(),
        done_where: (text.done_where)(&folder).into(),
        details: text.details.into(),
        open: text.open.into(),
        failed: text.failed.into(),
        outcome: RefCell::default(),
    };
    dialog.show();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Ask,
    Progress,
    Outcome,
}

#[derive(Default)]
struct Shared {
    read: AtomicU64,
    step: AtomicU8,
    outcome: Mutex<Option<Result<Finished, String>>>,
}

/// How the last page reads: the install's report, or why it failed.
#[derive(Default)]
enum Outcome {
    #[default]
    Pending,
    Installed {
        broken: bool,
        report: HSTRING,
    },
    Failed(HSTRING),
}

/// One page of the dialog.
struct Spec<'a> {
    main: &'a HSTRING,
    content: &'a HSTRING,
    details: Option<&'a HSTRING>,
    buttons: &'a [TASKDIALOG_BUTTON],
    common: TASKDIALOG_COMMON_BUTTON_FLAGS,
    cancellable: bool,
}

/// Lives on the stack of `show` for as long as the dialog does; the pages point into it.
struct Dialog {
    payload: &'static [u8],
    redist: &'static [u8],
    unpacked: u64,
    dest: PathBuf,
    shared: Arc<Shared>,
    page: Cell<Page>,
    step: Cell<u8>,
    title: HSTRING,
    ask: HSTRING,
    where_to: HSTRING,
    install: HSTRING,
    installing: HSTRING,
    steps: [HSTRING; 3],
    done: HSTRING,
    done_broken: HSTRING,
    done_where: HSTRING,
    details: HSTRING,
    open: HSTRING,
    failed: HSTRING,
    outcome: RefCell<Outcome>,
}

impl Dialog {
    fn show(&self) {
        let button = [button(INSTALL, &self.install)];
        let config = self.config(&Spec {
            main: &self.ask,
            content: &self.where_to,
            details: None,
            buttons: &button,
            common: TDCBF_CANCEL_BUTTON,
            cancellable: true,
        });
        // SAFETY: `config` and everything it points to live in `self` or on this stack frame
        // until the dialog returns.
        let _ = unsafe { TaskDialogIndirect(&raw const config, None, None, None) };
    }

    fn config(&self, spec: &Spec<'_>) -> TASKDIALOGCONFIG {
        let mut flags = TASKDIALOG_FLAGS(0);
        if spec.cancellable {
            flags |= TDF_ALLOW_DIALOG_CANCELLATION;
        }
        if self.page.get() == Page::Progress {
            flags |= TDF_SHOW_PROGRESS_BAR | TDF_CALLBACK_TIMER;
        }
        let text = |s: Option<&HSTRING>| s.map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr()));
        TASKDIALOGCONFIG {
            cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>()).unwrap_or(u32::MAX),
            dwFlags: flags,
            dwCommonButtons: spec.common,
            pszWindowTitle: PCWSTR(self.title.as_ptr()),
            pszMainInstruction: PCWSTR(spec.main.as_ptr()),
            pszContent: PCWSTR(spec.content.as_ptr()),
            pszExpandedInformation: text(spec.details),
            pszCollapsedControlText: text(spec.details.map(|_| &self.details)),
            cButtons: u32::try_from(spec.buttons.len()).unwrap_or(0),
            pButtons: spec.buttons.as_ptr(),
            nDefaultButton: spec.buttons.first().map_or(0, |b| b.nButtonID),
            pfCallback: Some(callback),
            lpCallbackData: std::ptr::from_ref(self) as isize,
            ..Default::default()
        }
    }

    fn navigate(&self, hwnd: HWND, page: Page) {
        self.page.set(page);
        let open = [button(OPEN, &self.open)];
        let outcome = self.outcome.borrow();
        let spec = match (page, &*outcome) {
            (Page::Ask, _) | (Page::Outcome, Outcome::Pending) => return,
            (Page::Progress, _) => Spec {
                main: &self.installing,
                content: &self.steps[usize::from(UNPACKING)],
                details: None,
                buttons: &[],
                common: TDCBF_CANCEL_BUTTON,
                cancellable: false,
            },
            (Page::Outcome, Outcome::Installed { broken, report }) => Spec {
                main: if *broken {
                    &self.done_broken
                } else {
                    &self.done
                },
                content: &self.done_where,
                details: Some(report),
                buttons: &open,
                common: TDCBF_CLOSE_BUTTON,
                cancellable: true,
            },
            (Page::Outcome, Outcome::Failed(why)) => Spec {
                main: &self.failed,
                content: why,
                details: None,
                buttons: &[],
                common: TDCBF_CLOSE_BUTTON,
                cancellable: true,
            },
        };
        let config = self.config(&spec);
        // SAFETY: the dialog reads `config` during the call; its strings live in `self`.
        unsafe {
            SendMessageW(
                hwnd,
                message_id(TDM_NAVIGATE_PAGE.0),
                Some(WPARAM(0)),
                Some(LPARAM(std::ptr::from_ref(&config) as isize)),
            );
        }
    }

    fn start(&self, hwnd: HWND) {
        let range = isize::try_from(STEPS).unwrap_or(0) << 16;
        send(hwnd, TDM_SET_PROGRESS_BAR_RANGE.0, 0, range);
        send(
            hwnd,
            TDM_ENABLE_BUTTON.0,
            usize::try_from(IDCANCEL.0).unwrap_or(0),
            0,
        );
        let (payload, redist, dest) = (self.payload, self.redist, self.dest.clone());
        let shared = Arc::clone(&self.shared);
        let size_kib = u32::try_from(self.unpacked / 1024).unwrap_or(u32::MAX);
        thread::spawn(move || {
            let job = Job {
                payload,
                redist,
                dest: &dest,
                size_kib,
                read: &shared.read,
                step: &shared.step,
            };
            let result = work::run(&job).map_err(|e| e.to_string());
            *shared
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(result);
        });
    }

    fn tick(&self, hwnd: HWND) {
        let step = self.shared.step.load(Ordering::Relaxed);
        if step != self.step.replace(step) {
            if let Some(line) = self.steps.get(usize::from(step)) {
                send(
                    hwnd,
                    TDM_SET_ELEMENT_TEXT.0,
                    usize::try_from(TDE_CONTENT.0).unwrap_or(0),
                    line.as_ptr() as isize,
                );
            }
            // Past unpacking nothing counts bytes, and a moving bar shows the work goes on.
            send(hwnd, TDM_SET_MARQUEE_PROGRESS_BAR.0, 1, 0);
            send(hwnd, TDM_SET_PROGRESS_BAR_MARQUEE.0, 1, 30);
        }
        if step == UNPACKING {
            let read = self.shared.read.load(Ordering::Relaxed);
            let total = u64::try_from(self.payload.len()).unwrap_or(u64::MAX).max(1);
            let done = usize::try_from(read.saturating_mul(STEPS as u64) / total).unwrap_or(STEPS);
            send(hwnd, TDM_SET_PROGRESS_BAR_POS.0, done, 0);
        }
        let finished = self
            .shared
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(finished) = finished {
            *self.outcome.borrow_mut() = match finished {
                Ok(Finished { report, broken }) => Outcome::Installed {
                    broken,
                    report: report.into(),
                },
                Err(error) => {
                    let advice = RU.setup.failed_advice;
                    Outcome::Failed(format!("{error}\n\n{advice}").into())
                }
            };
            self.navigate(hwnd, Page::Outcome);
        }
    }

    fn launch(&self) {
        let app = work::program(&self.dest);
        let dir = app.parent().unwrap_or(&self.dest);
        let _ = Command::new(&app).current_dir(dir).spawn();
    }
}

/// A message to the dialog that called us, with value arguments.
fn send(hwnd: HWND, message: i32, wparam: usize, lparam: isize) {
    // SAFETY: the dialog belongs to this thread, and every pointer passed lives in `Dialog`.
    unsafe {
        SendMessageW(
            hwnd,
            message_id(message),
            Some(WPARAM(wparam)),
            Some(LPARAM(lparam)),
        );
    }
}

fn button(id: i32, text: &HSTRING) -> TASKDIALOG_BUTTON {
    TASKDIALOG_BUTTON {
        nButtonID: id,
        pszButtonText: PCWSTR(text.as_ptr()),
    }
}

const fn message_id(message: i32) -> u32 {
    message.cast_unsigned()
}

unsafe extern "system" fn callback(
    hwnd: HWND,
    notification: TASKDIALOG_NOTIFICATIONS,
    wparam: WPARAM,
    _: LPARAM,
    data: isize,
) -> HRESULT {
    // SAFETY: `data` is the address of the `Dialog` that `show` keeps alive until the dialog
    // returns, and the dialog calls back only on this thread.
    let dialog = unsafe { &*(data as *const Dialog) };
    match (notification, dialog.page.get()) {
        (TDN_BUTTON_CLICKED, Page::Ask) if wparam.0 == usize::try_from(INSTALL).unwrap_or(0) => {
            dialog.navigate(hwnd, Page::Progress);
            S_FALSE
        }
        (TDN_BUTTON_CLICKED, Page::Progress) => S_FALSE,
        (TDN_BUTTON_CLICKED, Page::Outcome) if wparam.0 == usize::try_from(OPEN).unwrap_or(0) => {
            dialog.launch();
            S_OK
        }
        (TDN_NAVIGATED, Page::Progress) => {
            dialog.start(hwnd);
            S_OK
        }
        (TDN_TIMER, Page::Progress) => {
            dialog.tick(hwnd);
            S_OK
        }
        _ => S_OK,
    }
}

fn message(text: &str) {
    let (text, title) = (HSTRING::from(text), HSTRING::from(RU.setup.title));
    // SAFETY: both strings outlive the call.
    unsafe { MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR) };
}

fn is_64_bit() -> bool {
    if cfg!(target_pointer_width = "64") {
        return true;
    }
    let mut wow = BOOL(0);
    // SAFETY: the pseudo handle needs no closing, and `wow` outlives the call.
    unsafe { IsWow64Process(GetCurrentProcess(), &raw mut wow) }.is_ok() && wow.as_bool()
}

fn is_windows_8_or_later() -> bool {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `info` is writable and its size field is set, as RtlGetVersion requires.
    let status = unsafe { RtlGetVersion(&raw mut info) };
    status.is_ok() && (info.dwMajorVersion, info.dwMinorVersion) >= (6, 2)
}
