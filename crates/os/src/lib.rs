//! Child processes and file moves behind the ports. `unsafe` lives only in `sys`.

mod files;
mod process;
mod sys;

pub use files::LocalFiles;
pub use process::LocalRunner;

/// The 8.3 form of an existing path on Windows, for children that open paths through the ANSI
/// code page. Elsewhere, and on volumes without short names, the path itself.
#[must_use]
pub fn short_path(path: &std::path::Path) -> std::path::PathBuf {
    sys::short_path(path)
}

/// Swaps `from` in for `to`, replacing it, and returns once the rename is on disk.
///
/// # Errors
///
/// Errors from the operating system.
pub fn replace_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    sys::replace_file(from, to)
}

/// Loads DLLs from System32 only, so one planted next to a download never loads.
///
/// # Errors
///
/// Windows refusing the setting.
pub fn harden_dll_search() -> std::io::Result<()> {
    sys::harden_dll_search()
}
