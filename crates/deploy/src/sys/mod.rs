//! Windows calls: the shortcuts and uninstall entry, the machine probe, and elevated runs.

#![allow(unsafe_code)]

mod elevate;
mod probe;
mod shell;

pub use elevate::run_elevated;
pub use probe::probe;
pub use shell::{Entry, register, unregister};
