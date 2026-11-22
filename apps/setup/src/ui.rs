//! The installer's three pages in one Windows task dialog: the question, the progress bar while
//! files unpack, and the outcome, which can start the program.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use pastegrab_deploy::{Entry, launch};
use pastegrab_i18n::RU;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{HWND, LPARAM, S_FALSE, S_OK, WPARAM};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOG_FLAGS, TASKDIALOG_NOTIFICATIONS,
    TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDCBF_CLOSE_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_CALLBACK_TIMER, TDF_SHOW_PROGRESS_BAR, TDM_ENABLE_BUTTON, TDM_NAVIGATE_PAGE,
    TDM_SET_PROGRESS_BAR_POS, TDM_SET_PROGRESS_BAR_RANGE, TDN_BUTTON_CLICKED, TDN_NAVIGATED,
    TDN_TIMER, TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IDCANCEL, MB_ICONERROR, MB_OK, MessageBoxW, SendMessageW,
};
use windows::core::{BOOL, HRESULT, HSTRING, PCWSTR};

const INSTALL: i32 = 100;
const OPEN: i32 = 101;
const STEPS: usize = 1000;

/// Checks the PC, then asks, unpacks and reports.
pub fn run(payload: &'static [u8], unpacked: u64) {
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
        unpacked,
        dest,
        shared: Arc::default(),
        page: Cell::new(Page::Ask),
        title: text.title.into(),
        ask: text.ask.into(),
        where_to: (text.where_to)(&(RU.size)(unpacked), &folder).into(),
        install: text.install.into(),
        installing: text.installing.into(),
        unpacking: text.unpacking.into(),
        done: text.done.into(),
        done_where: (text.done_where)(&folder).into(),
        open: text.open.into(),
        failed: text.failed.into(),
        failure: RefCell::default(),
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
    outcome: Mutex<Option<Result<(), String>>>,
}

/// Lives on the stack of `show` for as long as the dialog does; the pages point into it.
struct Dialog {
    payload: &'static [u8],
    unpacked: u64,
    dest: PathBuf,
    shared: Arc<Shared>,
    page: Cell<Page>,
    title: HSTRING,
    ask: HSTRING,
    where_to: HSTRING,
    install: HSTRING,
    installing: HSTRING,
    unpacking: HSTRING,
    done: HSTRING,
    done_where: HSTRING,
    open: HSTRING,
    failed: HSTRING,
    failure: RefCell<HSTRING>,
}

impl Dialog {
    fn show(&self) {
        let button = [button(INSTALL, &self.install)];
        let config = self.page(
            &self.ask,
            &self.where_to,
            &button,
            TDCBF_CANCEL_BUTTON,
            true,
        );
        // SAFETY: `config` and everything it points to live in `self` or on this stack frame
        // until the dialog returns.
        let _ = unsafe { TaskDialogIndirect(&raw const config, None, None, None) };
    }

    fn page(
        &self,
        main: &HSTRING,
        content: &HSTRING,
        buttons: &[TASKDIALOG_BUTTON],
        common: TASKDIALOG_COMMON_BUTTON_FLAGS,
        cancellable: bool,
    ) -> TASKDIALOGCONFIG {
        let mut flags = TASKDIALOG_FLAGS(0);
        if cancellable {
            flags |= TDF_ALLOW_DIALOG_CANCELLATION;
        }
        if self.page.get() == Page::Progress {
            flags |= TDF_SHOW_PROGRESS_BAR | TDF_CALLBACK_TIMER;
        }
        TASKDIALOGCONFIG {
            cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>()).unwrap_or(u32::MAX),
            dwFlags: flags,
            dwCommonButtons: common,
            pszWindowTitle: PCWSTR(self.title.as_ptr()),
            pszMainInstruction: PCWSTR(main.as_ptr()),
            pszContent: PCWSTR(content.as_ptr()),
            cButtons: u32::try_from(buttons.len()).unwrap_or(0),
            pButtons: buttons.as_ptr(),
            nDefaultButton: buttons.first().map_or(0, |b| b.nButtonID),
            pfCallback: Some(callback),
            lpCallbackData: std::ptr::from_ref(self) as isize,
            ..Default::default()
        }
    }

    fn navigate(&self, hwnd: HWND, page: Page) {
        self.page.set(page);
        let failure = self.failure.borrow().clone();
        let open = [button(OPEN, &self.open)];
        let config = match page {
            Page::Ask => return,
            Page::Progress => self.page(
                &self.installing,
                &self.unpacking,
                &[],
                TDCBF_CANCEL_BUTTON,
                false,
            ),
            Page::Outcome if failure.is_empty() => self.page(
                &self.done,
                &self.done_where,
                &open,
                TDCBF_CLOSE_BUTTON,
                true,
            ),
            Page::Outcome => self.page(&self.failed, &failure, &[], TDCBF_CLOSE_BUTTON, true),
        };
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
        // SAFETY: plain messages to the dialog that called us, with value arguments.
        unsafe {
            SendMessageW(
                hwnd,
                message_id(TDM_SET_PROGRESS_BAR_RANGE.0),
                Some(WPARAM(0)),
                Some(LPARAM(range)),
            );
            SendMessageW(
                hwnd,
                message_id(TDM_ENABLE_BUTTON.0),
                Some(WPARAM(usize::try_from(IDCANCEL.0).unwrap_or(0))),
                Some(LPARAM(0)),
            );
        }
        let (payload, dest, shared) = (self.payload, self.dest.clone(), Arc::clone(&self.shared));
        let size_kib = u32::try_from(self.unpacked / 1024).unwrap_or(u32::MAX);
        thread::spawn(move || {
            let result = pastegrab_deploy::install(payload, &dest, &shared.read)
                .and_then(|()| first_state(&dest))
                .and_then(|()| register(&dest, size_kib))
                .map_err(|e| e.to_string());
            *shared
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(result);
        });
    }

    fn tick(&self, hwnd: HWND) {
        let read = self.shared.read.load(Ordering::Relaxed);
        let total = u64::try_from(self.payload.len()).unwrap_or(u64::MAX).max(1);
        let step = usize::try_from(read.saturating_mul(STEPS as u64) / total).unwrap_or(STEPS);
        // SAFETY: a plain message to the dialog that called us, with a value argument.
        unsafe {
            SendMessageW(
                hwnd,
                message_id(TDM_SET_PROGRESS_BAR_POS.0),
                Some(WPARAM(step)),
                None,
            );
        }
        let outcome = self
            .shared
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(outcome) = outcome {
            if let Err(error) = outcome {
                let advice = RU.setup.failed_advice;
                *self.failure.borrow_mut() = format!("{error}\n\n{advice}").into();
            }
            self.navigate(hwnd, Page::Outcome);
        }
    }

    fn launch(&self) {
        let app = program(&self.dest);
        let dir = app.parent().unwrap_or(&self.dest);
        let _ = Command::new(&app).current_dir(dir).spawn();
    }
}

/// The launcher, which starts the version `state.json` names.
fn program(dest: &Path) -> PathBuf {
    dest.join("pastegrab.exe")
}

/// The launcher's first `state.json`, naming the version the payload carries.
fn first_state(dest: &Path) -> io::Result<()> {
    let state = launch::state(dest)
        .ok_or_else(|| io::Error::other("the payload carries no version of pastegrab"))?;
    launch::write_state(dest, &state)
}

fn register(dest: &Path, size_kib: u32) -> io::Result<()> {
    pastegrab_deploy::register(&Entry {
        exe: &program(dest),
        folder: dest,
        version: env!("CARGO_PKG_VERSION"),
        size_kib,
        description: RU.setup.shortcut,
    })
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
