//! pastegrab's window. The composition root: it finds the tools, wires the adapters to the
//! core and hands the core to the view.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod uninstall;
#[cfg(windows)]
mod update;
#[cfg(windows)]
mod wiring;

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    {
        wiring::run()
    }
    #[cfg(not(windows))]
    {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "pastegrab's window runs on Windows; try pgcli"
        );
        std::process::ExitCode::FAILURE
    }
}
