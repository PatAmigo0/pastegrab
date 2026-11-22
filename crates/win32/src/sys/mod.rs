//! Everything that calls Win32, and the only place in this crate where `unsafe` is allowed.

#![allow(unsafe_code)]

mod bridge;
mod fade;
mod instance;
mod rows;
mod shell;
mod taskbar;
mod tray;
mod window;

pub use bridge::WinView;
pub use instance::{Instance, claim, window_open};
pub use shell::{alert, ask, inform, screen_height, videos_folder};
pub use window::{Startup, run};
