//! App updates: a daily background check that stages newer signed releases, plus manual check
//! status and relaunch for the Settings dialog.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pastegrab_deploy::launch;
use pastegrab_deploy::updater::{AppUpdate, Releases};
use pastegrab_http::Downloads;
use pastegrab_update::Version;
use pastegrab_win32::{UpdateControl, UpdateInfo, UpdateState};

/// The newest published release; `latest` skips drafts and pre-releases.
const RELEASES: &str = "https://github.com/PatAmigo0/pastegrab/releases/latest/download/";
const KEY: &str = include_str!("../../../keys/release.pub");
const DAY: Duration = Duration::from_hours(24);
const MANIFEST_LIMIT: u64 = 64 * 1024;
const SIGNATURE_LIMIT: u64 = 4 * 1024;

/// Serializes automatic and manual update checks so they never race on staging files.
static CHECK_LOCK: Mutex<()> = Mutex::new(());

/// Checks for a newer release on its own thread, unless the last check was within a day.
pub fn start(data: PathBuf) {
    let _ = thread::Builder::new().name("update".into()).spawn(move || {
        let _ = check_inner(&data, false);
    });
}

/// Spawns `pastegrab.exe` from the install root after the current window and mutex are gone.
///
/// # Errors
///
/// When running outside an install folder or spawning the launcher fails.
pub fn relaunch() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let root = launch::root_of(&exe).ok_or("not in an install folder")?;
    let launcher = root.join("pastegrab.exe");
    Command::new(&launcher)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", launcher.display()))
}

/// Update status and manual check adapter for the Settings dialog.
pub struct AppUpdater {
    data: PathBuf,
    current: String,
    installed: Option<Version>,
    exe: Option<PathBuf>,
    root: Option<PathBuf>,
}

impl AppUpdater {
    /// Builds an updater pointing at `data` and the running executable's install root.
    #[must_use]
    pub fn new(data: PathBuf) -> Self {
        let current = env!("CARGO_PKG_VERSION").to_owned();
        let installed = current.parse::<Version>().ok();
        let exe = env::current_exe().ok();
        let root = exe
            .as_deref()
            .and_then(launch::root_of)
            .map(Path::to_path_buf);
        Self {
            data,
            current,
            installed,
            exe,
            root,
        }
    }
}

impl UpdateControl for AppUpdater {
    fn status(&self) -> UpdateInfo {
        let last_checked = read_checked(&self.data);
        let current_mtime = self.exe.as_deref().and_then(file_mtime_secs);
        let (Some(root), Some(installed)) = (self.root.as_deref(), self.installed) else {
            return UpdateInfo {
                current: self.current.clone(),
                staged: None,
                last_checked,
                last_downloaded: current_mtime,
                state: UpdateState::NotInstalled,
            };
        };
        if let Some((version, staged_mtime)) = find_staged(root, installed) {
            return UpdateInfo {
                current: self.current.clone(),
                staged: Some(version.to_string()),
                last_checked,
                last_downloaded: staged_mtime.or(current_mtime),
                state: UpdateState::Ready,
            };
        }
        UpdateInfo {
            current: self.current.clone(),
            staged: None,
            last_checked,
            last_downloaded: current_mtime,
            state: UpdateState::UpToDate,
        }
    }

    fn check(&self) -> UpdateInfo {
        let ok = check_inner(&self.data, true);
        let mut info = self.status();
        if !ok && info.state != UpdateState::NotInstalled && info.staged.is_none() {
            info.state = UpdateState::Failed;
        }
        info
    }
}

fn check_inner(data: &Path, force: bool) -> bool {
    let _guard = CHECK_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let stamp = data.join("update-checked");
    let recent = fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < DAY));
    let (Ok(exe), Ok(installed)) = (
        env::current_exe(),
        env!("CARGO_PKG_VERSION").parse::<Version>(),
    ) else {
        return false;
    };
    // A copy outside an install, such as a build folder, never updates itself.
    let Some(root) = launch::root_of(&exe) else {
        return false;
    };
    if !force && recent {
        return true;
    }
    let releases = GitHub {
        downloads: Downloads::default(),
        // The lab points this at a test release; the signature check still applies.
        base: env::var("PASTEGRAB_RELEASES").unwrap_or_else(|_| RELEASES.to_owned()),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let outcome = AppUpdate {
        root,
        installed,
        key: KEY,
        releases: &releases,
    }
    .run(now);
    let line = match &outcome {
        Ok(outcome) => format!("{now} {outcome}\n"),
        Err(e) => format!("{now} {e}\n"),
    };
    // The stamp counts only a check that got an answer, so a failed one runs again next start.
    if outcome.is_ok() {
        let _ = fs::write(&stamp, &line);
    }
    let _ = fs::write(data.join("update-last.txt"), line);
    outcome.is_ok()
}

fn read_checked(data: &Path) -> Option<u64> {
    let stamp = data.join("update-checked");
    if let Ok(text) = fs::read_to_string(&stamp)
        && let Some(secs) = text.split_whitespace().next().and_then(|s| s.parse().ok())
    {
        return Some(secs);
    }
    file_mtime_secs(&stamp)
}

fn find_staged(root: &Path, installed: Version) -> Option<(Version, Option<u64>)> {
    let state = launch::read_state(root)?;
    let candidate = state.current();
    if candidate <= installed {
        return None;
    }
    let exe = launch::app_exe(root, candidate);
    exe.is_file().then(|| (candidate, file_mtime_secs(&exe)))
}

fn file_mtime_secs(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs())
}

struct GitHub {
    downloads: Downloads,
    base: String,
}

impl Releases for GitHub {
    fn manifest(&self) -> io::Result<(Vec<u8>, String)> {
        let file = |name: &str, limit| self.downloads.read(&format!("{}{name}", self.base), limit);
        let manifest = file("update.json", MANIFEST_LIMIT)?;
        let signature = file("update.json.minisig", SIGNATURE_LIMIT)?;
        let signature = String::from_utf8(signature).map_err(io::Error::other)?;
        Ok((manifest, signature))
    }

    fn download(&self, url: &str, dest: &Path, limit: u64) -> io::Result<()> {
        self.downloads.save(url, dest, limit)
    }
}
