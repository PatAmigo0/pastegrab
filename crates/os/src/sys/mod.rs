use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

#[cfg(unix)]
pub(crate) use unix::{
    Contained, configure, harden_dll_search, mark_downloaded, move_no_replace, quiet_child_errors,
    replace_file, short_path,
};
#[cfg(windows)]
pub(crate) use win::{
    Contained, configure, harden_dll_search, mark_downloaded, move_no_replace, quiet_child_errors,
    replace_file, short_path,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
