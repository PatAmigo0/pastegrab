//! `pastegrab-setup.exe`: checks for 64-bit Windows 8 or newer, unpacks the program and every
//! tool it runs into the person's own folder, and adds its shortcuts and uninstall entry.
//! Nothing downloads during the install.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code, reason = "the Windows dialog shows it"))]
mod report;
#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod work;

/// The install folder's files, packed by `tools/release/build-setup.sh`.
static PAYLOAD: &[u8] = include_bytes!(env!("PASTEGRAB_PAYLOAD_FILE"));
/// Microsoft's VC++ redistributable, run only where Windows lacks the UCRT.
static REDIST: &[u8] = include_bytes!(env!("PASTEGRAB_REDIST_FILE"));

fn main() {
    let unpacked: u64 = env!("PASTEGRAB_PAYLOAD_SIZE").parse().unwrap_or(0);
    #[cfg(windows)]
    ui::run(PAYLOAD, REDIST, unpacked);
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let carried = PAYLOAD.len() + REDIST.len();
        let _ = writeln!(
            std::io::stderr(),
            "pastegrab-setup runs on Windows; this build carries {carried} bytes, {unpacked} unpacked"
        );
    }
}
