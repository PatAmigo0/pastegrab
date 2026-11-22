//! `pastegrab.exe`, what the shortcuts start. It runs the version `state.json` names from
//! `app\<version>\`, gives a new version two starts to open its window, and rolls it back after.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::env;
use std::process::{Command, ExitCode};

use pastegrab_deploy::launch;
use pastegrab_update::Start;

#[cfg(windows)]
mod sys;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            report(&problem);
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let root = exe.parent().ok_or("pastegrab.exe has no folder")?;
    let mut state = launch::state(root)
        .ok_or_else(|| format!("no version in {}", root.join("app").display()))?;
    // A start that only hands links to the open window proves nothing about the new version.
    let start = if window_open() {
        Start::Run(state.current())
    } else {
        let start = state.start();
        // Unsaved, the trial count is lost; the start itself still goes ahead.
        let _ = launch::write_state(root, &state);
        start
    };
    let version = match start {
        Start::Run(version) | Start::RolledBack { run: version, .. } => version,
    };
    let app = launch::app_exe(root, version);
    let mut command = Command::new(&app);
    command.args(env::args_os().skip(1));
    if let Some(dir) = app.parent() {
        command.current_dir(dir);
    }
    command
        .spawn()
        .map(drop)
        .map_err(|e| format!("{}: {e}", app.display()))
}

#[cfg(windows)]
fn window_open() -> bool {
    sys::window_open()
}

#[cfg(not(windows))]
const fn window_open() -> bool {
    false
}

#[cfg(windows)]
fn report(problem: &str) {
    sys::alert(problem);
}

#[cfg(not(windows))]
fn report(problem: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "pastegrab: {problem}");
}
