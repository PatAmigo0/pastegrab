//! The operating system behind the ports: child processes that die with their step, and file
//! moves that never replace a file. Every `unsafe` block lives in `sys`.

mod files;
mod process;
mod sys;

pub use files::LocalFiles;
pub use process::LocalRunner;

/// Restricts DLL loading to System32 for pastegrab and every library it loads, so a DLL planted
/// next to a download never loads. Call it first in `main`; elsewhere it does nothing.
///
/// # Errors
///
/// The system's error when Windows refuses the setting.
pub fn harden_dll_search() -> std::io::Result<()> {
    sys::harden_dll_search()
}
