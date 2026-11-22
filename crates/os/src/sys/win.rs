#![allow(unsafe_code)]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::{ChildExt, CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

use pastegrab_app::ports::Exit;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    GetShortPathNameW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::Win32::System::Diagnostics::Debug::{
    GetErrorMode, SEM_FAILCRITICALERRORS, SetErrorMode, THREAD_ERROR_MODE,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
use windows::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW, CREATE_SUSPENDED, ResumeThread,
};
use windows::core::{HSTRING, PCWSTR};

use super::lock;

pub(crate) fn harden_dll_search() -> io::Result<()> {
    // SAFETY: a flag argument and no pointers.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) }.map_err(io::Error::from)
}

/// Children inherit this, and a missing DLL then fails with an exit code instead of a dialog.
pub(crate) fn quiet_child_errors() {
    // SAFETY: flag arguments and no pointers.
    unsafe {
        let mode = THREAD_ERROR_MODE(GetErrorMode());
        SetErrorMode(mode | SEM_FAILCRITICALERRORS);
    }
}

/// The child starts suspended, so nothing it spawns runs before the job owns it.
pub(crate) fn configure(command: &mut Command, low_priority: bool) {
    let mut flags = CREATE_SUSPENDED | CREATE_NO_WINDOW;
    if low_priority {
        flags |= BELOW_NORMAL_PRIORITY_CLASS;
    }
    command.creation_flags(flags.0);
}

pub(crate) struct Contained {
    child: Child,
    job: Arc<Mutex<Option<OwnedHandle>>>,
}

impl Contained {
    pub(crate) fn new(mut child: Child) -> io::Result<Self> {
        match contain(&child) {
            Ok(job) => Ok(Self {
                child,
                job: Arc::new(Mutex::new(Some(job))),
            }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        }
    }

    pub(crate) fn killer(&self) -> Killer {
        Killer(Arc::clone(&self.job))
    }

    pub(crate) fn wait(mut self) -> io::Result<Exit> {
        let status = self.child.wait()?;
        // Closing the job ends whatever the child left running.
        drop(lock(&self.job).take());
        Ok(Exit {
            code: status.code(),
        })
    }
}

impl Drop for Contained {
    fn drop(&mut self) {
        if let Some(job) = lock(&self.job).take() {
            terminate(&job);
        }
    }
}

#[derive(Clone)]
pub(crate) struct Killer(Arc<Mutex<Option<OwnedHandle>>>);

impl Killer {
    pub(crate) fn kill(&self) {
        if let Some(job) = &*lock(&self.0) {
            terminate(job);
        }
    }
}

fn contain(child: &Child) -> io::Result<OwnedHandle> {
    // SAFETY: the handle is new, and `OwnedHandle` closes it exactly once.
    let job = unsafe { OwnedHandle::from_raw_handle(CreateJobObjectW(None, PCWSTR::null())?.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    let size = u32::try_from(size_of_val(&limits)).map_err(io::Error::other)?;
    // SAFETY: `limits` outlives the call and `size` is its size; both handles are open.
    unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size,
        )?;
        AssignProcessToJobObject(raw(&job), HANDLE(child.as_raw_handle()))?;
    }
    // SAFETY: the main thread's handle stays open while `child` lives.
    if unsafe { ResumeThread(HANDLE(child.main_thread_handle().as_raw_handle())) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

fn terminate(job: &OwnedHandle) {
    // SAFETY: the job's handle stays open while `job` is borrowed.
    let _ = unsafe { TerminateJobObject(raw(job), 1) };
}

/// Without `MOVEFILE_REPLACE_EXISTING` an existing target fails the move, and without
/// `MOVEFILE_COPY_ALLOWED` so does another volume.
pub(crate) fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (HSTRING::from(from), HSTRING::from(to));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe { MoveFileExW(&from, &to, MOVEFILE_WRITE_THROUGH) }.map_err(io::Error::from)
}

pub(crate) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (HSTRING::from(from), HSTRING::from(to));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MoveFileExW(
            &from,
            &to,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(io::Error::from)
}

pub(crate) fn short_path(path: &Path) -> PathBuf {
    let long = HSTRING::from(path);
    // SAFETY: with no buffer the call only returns the length, terminator included.
    let needed = unsafe { GetShortPathNameW(&long, None) };
    let Ok(len) = usize::try_from(needed) else {
        return path.to_owned();
    };
    let mut buf = vec![0; len];
    // SAFETY: `buf` holds `len` units and outlives the call.
    let written = unsafe { GetShortPathNameW(&long, Some(&mut buf)) };
    match usize::try_from(written) {
        Ok(n) if n > 0 && n < buf.len() => PathBuf::from(OsString::from_wide(&buf[..n])),
        _ => path.to_owned(),
    }
}

pub(crate) fn mark_downloaded(file: &Path) {
    let mut stream = OsString::from(file);
    stream.push(":Zone.Identifier");
    // FAT volumes have no alternate streams, and the file works without the mark.
    let _ = fs::write(stream, "[ZoneTransfer]\r\nZoneId=3\r\n");
}
