use std::path::{Path, PathBuf};

use pastegrab_i18n::RU;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOLDERID_Videos, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog,
    IFileOpenDialog, IShellItem, KF_FLAG_DEFAULT, SHCreateItemFromParsingName,
    SHGetKnownFolderPath, SIGDN_FILESYSPATH, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MB_ICONERROR, MB_OK, MessageBoxW, SM_CYSCREEN, SW_SHOWNORMAL,
};
use windows::core::{HSTRING, PWSTR, w};

/// The person's Videos folder.
#[must_use]
pub fn videos_folder() -> Option<PathBuf> {
    // SAFETY: the returned buffer is ours to free, which `owned` does.
    let path = unsafe { SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None) }.ok()?;
    Some(PathBuf::from(owned(path)))
}

/// The height of the primary screen in pixels.
#[must_use]
pub fn screen_height() -> u32 {
    // SAFETY: a plain query without pointers.
    u32::try_from(unsafe { GetSystemMetrics(SM_CYSCREEN) }).unwrap_or(768)
}

/// A message box for a problem that leaves no window to show it in.
pub fn alert(text: &str) {
    let (text, title) = (HSTRING::from(text), HSTRING::from(RU.window_title));
    // SAFETY: both strings outlive the call.
    unsafe { MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR) };
}

/// Opens a file or folder the way Explorer would.
pub(super) fn open(path: &Path) {
    let path = HSTRING::from(path);
    // SAFETY: the string outlives the call; Windows owns whatever starts.
    unsafe { ShellExecuteW(None, w!("open"), &path, None, None, SW_SHOWNORMAL) };
}

/// Asks for a folder, starting in `start`; `None` when the person cancels.
pub(super) fn pick_folder(owner: HWND, start: &Path) -> Option<PathBuf> {
    // SAFETY: COM is initialized on this thread before the window exists, and every interface
    // here is released when it drops.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().unwrap_or(FILEOPENDIALOGOPTIONS(0));
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;
        if let Ok(folder) =
            SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start), None)
        {
            let _ = dialog.SetFolder(&folder);
        }
        dialog.Show(Some(owner)).ok()?;
        let path = dialog
            .GetResult()
            .ok()?
            .GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?;
        Some(PathBuf::from(owned(path)))
    }
}

/// Copies a string the shell allocated and frees it.
fn owned(text: PWSTR) -> String {
    // SAFETY: the shell returned a NUL-terminated string that nobody else frees.
    unsafe {
        let copy = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0.cast_const().cast()));
        copy
    }
}
