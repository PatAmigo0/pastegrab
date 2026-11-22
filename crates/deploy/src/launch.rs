//! The install's layout as the launcher sees it: `pastegrab.exe` and `state.json` in the install
//! folder, and each version of the window in `app\<version>\`.

use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use pastegrab_update::{Launch, Version};

const APP: &str = "app";
const STATE: &str = "state.json";

/// The window of `version` in the install at `root`.
#[must_use]
pub fn app_exe(root: &Path, version: Version) -> PathBuf {
    root.join(APP)
        .join(version.to_string())
        .join(format!("pastegrab-app{EXE_SUFFIX}"))
}

/// The install folder of an exe in `app\<version>\`; `None` for an exe anywhere else.
#[must_use]
pub fn root_of(exe: &Path) -> Option<&Path> {
    let version_dir = exe.parent()?;
    version_dir.file_name()?.to_str()?.parse::<Version>().ok()?;
    let app = version_dir.parent()?;
    if app.file_name()? != APP {
        return None;
    }
    app.parent()
}

/// What `state.json` says; `None` when it is missing or unreadable.
#[must_use]
pub fn read_state(root: &Path) -> Option<Launch> {
    let text = fs::read(root.join(STATE)).ok()?;
    serde_json::from_slice(&text).ok()
}

/// Replaces `state.json` whole: a crash leaves the old file or the new one.
///
/// # Errors
///
/// Errors from the file system.
pub fn write_state(root: &Path, launch: &Launch) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(launch).map_err(io::Error::other)?;
    let temp = root.join("state.json.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, root.join(STATE))
}

/// The saved state while its version is on disk, otherwise a fresh one for the newest version
/// whose window is there.
#[must_use]
pub fn state(root: &Path) -> Option<Launch> {
    read_state(root)
        .filter(|launch| app_exe(root, launch.current()).is_file())
        .or_else(|| versions(root).pop().map(Launch::new))
}

/// Versions with their window on disk, oldest first.
fn versions(root: &Path) -> Vec<Version> {
    let Ok(entries) = fs::read_dir(root.join(APP)) else {
        return Vec::new();
    };
    let mut found: Vec<Version> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|&version| app_exe(root, version).is_file())
        .collect();
    found.sort_unstable();
    found
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pastegrab_update::{Launch, Version};

    use super::{app_exe, read_state, root_of, state, write_state};

    fn v(text: &str) -> Version {
        text.parse().expect("a version")
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pastegrab-launch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a temp folder");
        dir
    }

    fn install(root: &std::path::Path, version: &str) {
        let exe = app_exe(root, v(version));
        fs::create_dir_all(exe.parent().expect("a folder")).expect("a version folder");
        fs::write(exe, b"exe").expect("an exe");
    }

    #[test]
    fn without_a_state_the_newest_whole_version_runs() {
        let root = temp("newest");
        install(&root, "0.9.0");
        install(&root, "0.10.0");
        fs::create_dir_all(root.join("app").join("0.11.0")).expect("an empty folder");
        fs::create_dir_all(root.join("app").join("old")).expect("a stray folder");
        assert_eq!(state(&root), Some(Launch::new(v("0.10.0"))));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_saved_state_wins_while_its_version_is_on_disk() {
        let root = temp("saved");
        install(&root, "0.1.0");
        install(&root, "0.2.0");
        let mut launch = Launch::new(v("0.1.0"));
        launch.stage(v("0.2.0"));
        write_state(&root, &launch).expect("a save");
        assert_eq!(read_state(&root), Some(launch.clone()));
        assert_eq!(state(&root), Some(launch));
        fs::remove_dir_all(root.join("app").join("0.2.0")).expect("a removal");
        assert_eq!(state(&root), Some(Launch::new(v("0.1.0"))));
        fs::write(root.join("state.json"), "{ half").expect("a broken file");
        assert_eq!(read_state(&root), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_exe_in_a_version_folder_knows_its_install() {
        let root = temp("root");
        let exe = app_exe(&root, v("0.2.0"));
        assert_eq!(root_of(&exe), Some(root.as_path()));
        assert_eq!(root_of(&root.join("pastegrab-app.exe")), None);
        let _ = fs::remove_dir_all(&root);
    }
}
