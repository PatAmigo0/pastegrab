//! Child processes and file moves behind the ports. `unsafe` lives only in `sys`.

mod files;
mod process;
mod sys;

pub use files::LocalFiles;
pub use process::LocalRunner;

/// Loads DLLs from System32 only, so one planted next to a download never loads.
///
/// # Errors
///
/// Windows refusing the setting.
pub fn harden_dll_search() -> std::io::Result<()> {
    sys::harden_dll_search()
}
