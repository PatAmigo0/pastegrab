//! Everything that calls Win32, and the only place in this crate where `unsafe` is allowed.

#![allow(unsafe_code)]

mod bridge;
mod instance;
mod shell;
mod window;

pub use bridge::WinView;
pub use instance::{Instance, claim};
pub use shell::{alert, screen_height, videos_folder};
pub use window::{Startup, run};
