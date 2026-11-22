//! pastegrab's window over Win32. `text` turns the board into the person's words and builds
//! everywhere; the window itself exists only on Windows.

#[cfg_attr(not(windows), allow(dead_code, reason = "the window uses it"))]
mod cell;
#[cfg_attr(not(windows), allow(dead_code, reason = "the window uses it"))]
mod motion;
#[cfg_attr(not(windows), allow(dead_code, reason = "the window uses it"))]
mod text;

#[cfg(windows)]
mod sys;

#[cfg(windows)]
pub use sys::{
    Instance, Startup, WinView, alert, ask, claim, enable_dpi, inform, run, screen_height,
    videos_folder, window_open,
};
