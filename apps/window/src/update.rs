//! The app update check: once a day, on its own thread after the window opens. A new version
//! lands beside the running one and starts next time.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pastegrab_deploy::launch;
use pastegrab_deploy::updater::{AppUpdate, Releases};
use pastegrab_http::Downloads;
use pastegrab_update::Version;

/// The newest published release; `latest` skips drafts and pre-releases.
const RELEASES: &str = "https://github.com/PatAmigo0/pastegrab/releases/latest/download/";
const KEY: &str = include_str!("../../../keys/release.pub");
const DAY: Duration = Duration::from_hours(24);
const MANIFEST_LIMIT: u64 = 64 * 1024;
const SIGNATURE_LIMIT: u64 = 4 * 1024;

/// Checks for a newer release on its own thread, unless the last check was within a day.
pub fn start(data: PathBuf) {
    let _ = thread::Builder::new()
        .name("update".into())
        .spawn(move || check(&data));
}

fn check(data: &Path) {
    let stamp = data.join("update-checked");
    let recent = fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| at.elapsed().is_ok_and(|age| age < DAY));
    let (Ok(exe), Ok(installed)) = (
        env::current_exe(),
        env!("CARGO_PKG_VERSION").parse::<Version>(),
    ) else {
        return;
    };
    // A copy outside an install, such as a build folder, never updates itself.
    let Some(root) = launch::root_of(&exe) else {
        return;
    };
    if recent {
        return;
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
        Ok(outcome) => format!("{now} {outcome:?}\n"),
        Err(e) => format!("{now} {e}\n"),
    };
    // The stamp counts only a check that got an answer, so a failed one runs again next start.
    if outcome.is_ok() {
        let _ = fs::write(&stamp, &line);
    }
    let _ = fs::write(data.join("update-last.txt"), line);
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
