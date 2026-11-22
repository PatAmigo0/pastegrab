//! `pastegrab-setup.exe`: checks for 64-bit Windows 8 or newer, then unpacks the program and
//! every tool it runs into the person's own folder. Nothing downloads during the install.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code, reason = "the Windows dialog calls it"))]
mod install;
#[cfg(windows)]
mod ui;

/// The install folder's files, packed by `tools/release/build-setup.sh`.
static PAYLOAD: &[u8] = include_bytes!(env!("PASTEGRAB_PAYLOAD_FILE"));

fn main() {
    let unpacked: u64 = env!("PASTEGRAB_PAYLOAD_SIZE").parse().unwrap_or(0);
    #[cfg(windows)]
    ui::run(PAYLOAD, unpacked);
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let carried = PAYLOAD.len();
        let _ = writeln!(
            std::io::stderr(),
            "pastegrab-setup runs on Windows; this build carries {carried} bytes, {unpacked} unpacked"
        );
    }
}
