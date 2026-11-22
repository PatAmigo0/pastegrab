//! The PC as Windows describes it, for setup's report and its defaults.

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::{env, thread};

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDiskFreeSpaceExW,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery,
    STORAGE_PROPERTY_QUERY, StorageDeviceSeekPenaltyProperty,
};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::SystemInformation::{
    GlobalMemoryStatusEx, MEMORYSTATUSEX, OSVERSIONINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
use windows::core::{HSTRING, w};

use crate::machine::Machine;

const MIB: u64 = 1024 * 1024;

/// The PC, with free space measured on the drive that holds `install`.
#[must_use]
pub fn probe(install: &Path) -> Machine {
    Machine {
        windows: windows_version(),
        cpu: cpu_name(),
        cores: thread::available_parallelism()
            .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX)),
        features: cpu_features(),
        ram_mib: ram_mib(),
        free_mib: free_mib(install),
        hdd: system_disk_spins(),
        screen: screen(),
        ucrt: has_ucrt(),
    }
}

fn windows_version() -> String {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `info` is writable and its size field is set, as RtlGetVersion requires.
    let status = unsafe { RtlGetVersion(&raw mut info) };
    if status.is_err() {
        return String::new();
    }
    format!(
        "{}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    )
}

fn cpu_name() -> String {
    let mut buffer = [0_u16; 256];
    let mut size = u32::try_from(size_of_val(&buffer)).unwrap_or(0);
    // SAFETY: `buffer` holds `size` bytes, and every string outlives the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0"),
            w!("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if read.is_err() {
        return String::new();
    }
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len]).trim().to_owned()
}

fn cpu_features() -> Vec<String> {
    let mut found = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        let checks = [
            ("sse4.1", std::arch::is_x86_feature_detected!("sse4.1")),
            ("sse4.2", std::arch::is_x86_feature_detected!("sse4.2")),
            ("avx", std::arch::is_x86_feature_detected!("avx")),
            ("avx2", std::arch::is_x86_feature_detected!("avx2")),
        ];
        found.extend(
            checks
                .into_iter()
                .filter(|(_, has)| *has)
                .map(|(name, _)| name.to_owned()),
        );
    }
    found
}

fn ram_mib() -> u64 {
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `status` is writable and its length field is set.
    if unsafe { GlobalMemoryStatusEx(&raw mut status) }.is_err() {
        return 0;
    }
    status.ullTotalPhys / MIB
}

fn free_mib(install: &Path) -> u64 {
    // The install folder may not exist yet; its nearest existing parent sits on the same drive.
    let Some(dir) = install.ancestors().find(|dir| dir.is_dir()) else {
        return 0;
    };
    let mut free = 0;
    // SAFETY: the path outlives the call, and `free` is writable.
    let read = unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir), Some(&raw mut free), None, None) };
    if read.is_err() {
        return 0;
    }
    free / MIB
}

fn system_disk_spins() -> Option<bool> {
    let drive = env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let volume = HSTRING::from(format!("\\\\.\\{drive}"));
    // SAFETY: opening the volume for queries only, with no access rights asked; the handle is
    // closed by `OwnedHandle`.
    let handle = unsafe {
        CreateFileW(
            &volume,
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()?;
    // SAFETY: CreateFileW returned an open handle we now own.
    let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut answer = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
    let mut returned = 0;
    // SAFETY: both buffers are sized as passed and outlive the synchronous call.
    let asked = unsafe {
        DeviceIoControl(
            HANDLE(handle.0),
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&raw const query).cast()),
            u32::try_from(size_of::<STORAGE_PROPERTY_QUERY>()).unwrap_or(0),
            Some((&raw mut answer).cast()),
            u32::try_from(size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>()).unwrap_or(0),
            Some(&raw mut returned),
            None,
        )
    };
    drop(owned);
    asked.ok().map(|()| answer.IncursSeekPenalty)
}

fn screen() -> (u32, u32) {
    // SAFETY: plain queries.
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (
        u32::try_from(width).unwrap_or(0),
        u32::try_from(height).unwrap_or(0),
    )
}

/// The 64-bit `ucrtbase.dll`; a 32-bit setup sees the real System32 through Sysnative.
fn has_ucrt() -> bool {
    let windows = PathBuf::from(env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()));
    ["Sysnative", "System32"]
        .iter()
        .any(|dir| windows.join(dir).join("ucrtbase.dll").is_file())
}
