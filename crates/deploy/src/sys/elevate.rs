//! Running an installer behind the UAC prompt and waiting for it.

use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;

use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

/// Runs `exe` with `args` as administrator, after the UAC prompt, and returns its exit code.
///
/// # Errors
///
/// A declined prompt or a program that can't start.
pub fn run_elevated(exe: &Path, args: &str) -> io::Result<u32> {
    let (file, parameters) = (HSTRING::from(exe), HSTRING::from(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points to outlive the call.
    unsafe { ShellExecuteExW(&raw mut info) }?;
    // A copy, because the 32-bit struct is packed and its fields can't be borrowed.
    let handle = info.hProcess;
    if handle.is_invalid() {
        return Err(io::Error::other(
            "the installer started without a process handle",
        ));
    }
    // SAFETY: SEE_MASK_NOCLOSEPROCESS hands the process handle to us, closed once by `OwnedHandle`.
    let process = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut code = 0;
    // SAFETY: the handle stays open while `process` lives, and `code` is writable.
    unsafe {
        WaitForSingleObject(handle, INFINITE);
        GetExitCodeProcess(handle, &raw mut code)?;
    }
    drop(process);
    Ok(code)
}
