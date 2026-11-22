//! The desktop and Start menu shortcuts and the per-user Programs and Features entry.

#![allow(unsafe_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    ShellLink,
};
use windows::core::{GUID, HSTRING, Interface};

const NAME: &str = "pastegrab";
// HKCU\Software isn't redirected for 32-bit programs, so setup and the 64-bit app share this key.
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\pastegrab";

/// What an install shows Windows.
#[derive(Debug, Clone, Copy)]
pub struct Entry<'a> {
    /// The program the shortcuts start, which also uninstalls with `--uninstall`.
    pub exe: &'a Path,
    /// The install folder.
    pub folder: &'a Path,
    /// The version Programs and Features lists.
    pub version: &'a str,
    /// The install's size in KiB.
    pub size_kib: u32,
    /// The shortcut's tooltip.
    pub description: &'a str,
}

/// Puts shortcuts on the desktop and in the Start menu, and adds the uninstall entry.
///
/// # Errors
///
/// COM, the shell or the registry refusing.
pub fn register(entry: &Entry<'_>) -> io::Result<()> {
    let _com = Com::init();
    for folder in [FOLDERID_Desktop, FOLDERID_Programs] {
        shortcut(&known_folder(folder)?.join(format!("{NAME}.lnk")), entry)?;
    }
    let exe = entry.exe.display();
    let key = Key::create(UNINSTALL_KEY)?;
    key.string("DisplayName", NAME)?;
    key.string("DisplayVersion", entry.version)?;
    key.string("DisplayIcon", &exe.to_string())?;
    key.string("InstallLocation", &entry.folder.display().to_string())?;
    key.string("UninstallString", &format!("\"{exe}\" --uninstall"))?;
    key.dword("EstimatedSize", entry.size_kib)?;
    key.dword("NoModify", 1)?;
    key.dword("NoRepair", 1)
}

/// Removes the shortcuts and the uninstall entry; ones already gone are fine.
///
/// # Errors
///
/// The shell or the registry refusing.
pub fn unregister() -> io::Result<()> {
    for folder in [FOLDERID_Desktop, FOLDERID_Programs] {
        match fs::remove_file(known_folder(folder)?.join(format!("{NAME}.lnk"))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    // SAFETY: the key name is NUL-terminated and outlives the call.
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY)) } {
        ERROR_FILE_NOT_FOUND => Ok(()),
        code => check(code),
    }
}

fn shortcut(path: &Path, entry: &Entry<'_>) -> io::Result<()> {
    let workdir = entry.exe.parent().unwrap_or(entry.folder);
    // SAFETY: COM is initialized on this thread, and every string outlives its call.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(entry.exe))?;
        link.SetWorkingDirectory(&HSTRING::from(workdir))?;
        link.SetDescription(&HSTRING::from(entry.description))?;
        link.SetIconLocation(&HSTRING::from(entry.exe), 0)?;
        link.cast::<IPersistFile>()?
            .Save(&HSTRING::from(path), true)?;
    }
    Ok(())
}

fn known_folder(id: GUID) -> io::Result<PathBuf> {
    // SAFETY: the shell allocates the string, which is copied before CoTaskMemFree releases it.
    unsafe {
        let raw = SHGetKnownFolderPath(&raw const id, KF_FLAG_DEFAULT, None)?;
        let path = raw.to_string().map(PathBuf::from);
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        path.map_err(io::Error::other)
    }
}

/// COM on this thread for as long as it lives.
struct Com(bool);

impl Com {
    fn init() -> Self {
        // SAFETY: paired with CoUninitialize in `drop` when it succeeds.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx in `init`.
            unsafe { CoUninitialize() };
        }
    }
}

struct Key(HKEY);

impl Key {
    fn create(path: &str) -> io::Result<Self> {
        let mut key = HKEY::default();
        // SAFETY: `key` is writable and the name outlives the call.
        let code = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &raw mut key,
                None,
            )
        };
        check(code).map(|()| Self(key))
    }

    fn string(&self, name: &str, value: &str) -> io::Result<()> {
        let bytes: Vec<u8> = value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        self.set(name, REG_SZ, &bytes)
    }

    fn dword(&self, name: &str, value: u32) -> io::Result<()> {
        self.set(name, REG_DWORD, &value.to_le_bytes())
    }

    fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> io::Result<()> {
        // SAFETY: the key is open, and the name and data outlive the call.
        check(unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, kind, Some(data)) })
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `create` and is closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn check(code: WIN32_ERROR) -> io::Result<()> {
    match code.0 {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code.cast_signed())),
    }
}
