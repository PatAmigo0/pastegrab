#![allow(unsafe_code)]

use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

use pastegrab_app::ports::Exit;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};

use super::lock;

#[expect(clippy::unnecessary_wraps, reason = "matches the Windows version")]
pub(crate) const fn harden_dll_search() -> io::Result<()> {
    Ok(())
}

pub(crate) const fn quiet_child_errors() {}

pub(crate) fn configure(command: &mut Command, low_priority: bool) {
    command.process_group(0);
    #[cfg(target_os = "linux")]
    {
        let setup = move || -> io::Result<()> {
            // The child dies with the thread that started it, so it can't outlive pastegrab.
            rustix::process::set_parent_process_death_signal(Some(Signal::KILL))?;
            if low_priority {
                rustix::process::setpriority_process(None, 10)?;
            }
            Ok(())
        };
        // SAFETY: `setup` runs between fork and exec and only makes the prctl and setpriority
        // system calls, which neither allocate nor take locks.
        unsafe { command.pre_exec(setup) };
    }
    #[cfg(not(target_os = "linux"))]
    let _ = low_priority;
}

pub(crate) struct Contained {
    child: Child,
    group: Arc<Mutex<Option<Pid>>>,
}

impl Contained {
    #[expect(clippy::unnecessary_wraps, reason = "matches the Windows version")]
    pub(crate) fn new(child: Child) -> io::Result<Self> {
        let group = Arc::new(Mutex::new(Some(Pid::from_child(&child))));
        Ok(Self { child, group })
    }

    pub(crate) fn killer(&self) -> Killer {
        Killer(Arc::clone(&self.group))
    }

    pub(crate) fn wait(mut self) -> io::Result<Exit> {
        let pid = Pid::from_child(&self.child);
        // An exited but unreaped child keeps its group id from being reused until disarm.
        while let Err(rustix::io::Errno::INTR) = waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        ) {}
        disarm(&self.group);
        let status = self.child.wait()?;
        Ok(Exit {
            code: status.code(),
        })
    }
}

impl Drop for Contained {
    fn drop(&mut self) {
        disarm(&self.group);
        let _ = self.child.wait();
    }
}

#[derive(Clone)]
pub(crate) struct Killer(Arc<Mutex<Option<Pid>>>);

impl Killer {
    pub(crate) fn kill(&self) {
        if let Some(pid) = *lock(&self.0) {
            let _ = kill_process_group(pid, Signal::KILL);
        }
    }
}

fn disarm(group: &Mutex<Option<Pid>>) {
    if let Some(pid) = lock(group).take() {
        let _ = kill_process_group(pid, Signal::KILL);
    }
}

/// A hard link fails when `to` exists, which makes it a rename that never replaces.
pub(crate) fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use io::ErrorKind::{AlreadyExists, NotFound};
    match fs::hard_link(from, to) {
        Ok(()) => {
            // A leftover old name goes with the work folder.
            let _ = fs::remove_file(from);
            Ok(())
        }
        Err(e) if matches!(e.kind(), AlreadyExists | NotFound) => Err(e),
        // Volumes without hard links, such as FAT, get a check and a rename that a race can beat.
        Err(_) if to.symlink_metadata().is_ok() => Err(AlreadyExists.into()),
        Err(_) => fs::rename(from, to),
    }
}

/// A rename reaches the disk with its folder.
pub(crate) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    match to.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => fs::File::open(dir)?.sync_all(),
        _ => Ok(()),
    }
}

pub(crate) const fn mark_downloaded(_file: &Path) {}

pub(crate) fn short_path(path: &Path) -> std::path::PathBuf {
    path.to_owned()
}
