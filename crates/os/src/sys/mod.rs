use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

#[cfg(unix)]
pub(crate) use unix::{
    Contained, configure, harden_dll_search, mark_downloaded, move_no_replace, quiet_child_errors,
};
#[cfg(windows)]
pub(crate) use win::{
    Contained, configure, harden_dll_search, mark_downloaded, move_no_replace, quiet_child_errors,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
