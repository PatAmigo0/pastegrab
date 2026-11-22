//! `pastegrab.exe --uninstall`, which Programs and Features runs: the install, the data folder,
//! the shortcuts and the entry go, and downloaded videos stay.

use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{self, ExitCode};

use pastegrab_i18n::RU;

pub fn run() -> ExitCode {
    let text = &RU.uninstall;
    if pastegrab_win32::window_open() {
        pastegrab_win32::inform(text.title, text.close_first);
        return ExitCode::FAILURE;
    }
    if !pastegrab_win32::ask(text.title, text.ask) {
        return ExitCode::SUCCESS;
    }
    match remove() {
        Ok(()) => {
            pastegrab_win32::inform(text.title, text.done);
            ExitCode::SUCCESS
        }
        Err(problem) => {
            pastegrab_win32::alert(&format!("{}\n\n{problem}", text.failed));
            ExitCode::FAILURE
        }
    }
}

fn remove() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let root = pastegrab_deploy::install_folder().ok_or("LOCALAPPDATA is unset")?;
    let installed = exe
        .parent()
        .and_then(Path::parent)
        .is_some_and(|here| same(here, &root));
    if !installed {
        return Err(format!("{} is not in {}", exe.display(), root.display()));
    }
    if let Some(data) = pastegrab_deploy::data_folder() {
        remove_tree(&data)?;
    }
    // A running exe can be renamed but not deleted, and a folder goes only when no process works
    // inside it.
    let temp = env::temp_dir();
    env::set_current_dir(&temp).map_err(|e| at(&temp, &e))?;
    let parked = temp.join(format!("pastegrab-uninstall-{}.exe", process::id()));
    fs::rename(&exe, &parked).map_err(|e| at(&exe, &e))?;
    if let Err(problem) = remove_tree(&root) {
        // Back in place, Programs and Features can start the uninstall again.
        if let Some(app) = exe.parent() {
            let _ = fs::create_dir_all(app);
        }
        let _ = fs::rename(&parked, &exe);
        return Err(problem);
    }
    pastegrab_deploy::unregister().map_err(|e| e.to_string())
}

fn remove_tree(dir: &Path) -> Result<(), String> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(at(dir, &e)),
        _ => Ok(()),
    }
}

fn same(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn at(path: &Path, error: &io::Error) -> String {
    format!("{}: {error}", path.display())
}
