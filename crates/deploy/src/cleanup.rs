//! Removes leftover working files after a crash or sudden power-off.
//!
//! Runs only after the single-instance mutex is held, before the queue starts any job.

use std::fs;
use std::path::Path;

/// Sweeps working folders left behind by a previous crash, kill or power-off.
pub fn sweep_startup(dests: &[&Path], data: &Path, root: Option<&Path>) {
    for (index, dest) in dests.iter().enumerate() {
        if dests[..index].contains(dest) {
            continue;
        }
        sweep_dest(dest);
    }
    sweep_contents(&data.join("jobs"));
    sweep_contents(&data.join("tmp"));
    let _ = fs::remove_file(data.join("settings.tmp"));
    if let Some(root) = root {
        sweep_staging(&root.join("app"));
        let _ = fs::remove_file(root.join("state.tmp"));
    }
}

/// Removes `.pastegrab-<digits>` working folders inside a destination directory.
pub fn sweep_dest(dest: &Path) {
    let Ok(entries) = fs::read_dir(dest) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_job_folder(name) {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn is_job_folder(name: &str) -> bool {
    name.strip_prefix(".pastegrab-")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

fn sweep_contents(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        remove_entry(&entry);
    }
}

fn sweep_staging(app_dir: &Path) {
    let Ok(entries) = fs::read_dir(app_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("download") || ext.eq_ignore_ascii_case("part")
        }) {
            remove_entry(&entry);
        }
    }
}

fn remove_entry(entry: &fs::DirEntry) {
    let Ok(kind) = entry.file_type() else {
        return;
    };
    let path = entry.path();
    if kind.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{is_job_folder, sweep_startup};

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pastegrab-cleanup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn job_folder_matcher_is_strict() {
        assert!(is_job_folder(".pastegrab-1"));
        assert!(is_job_folder(".pastegrab-42"));
        assert!(!is_job_folder(".pastegrab-"));
        assert!(!is_job_folder(".pastegrab-notes"));
        assert!(!is_job_folder(".git"));
        assert!(!is_job_folder("pastegrab-1"));
    }

    #[test]
    fn startup_sweep_cleans_leftovers_and_keeps_user_and_install_files() {
        let base = temp("sweep");
        let dest = base.join("videos");
        let data = base.join("data");
        let root = base.join("install");

        fs::create_dir_all(dest.join(".pastegrab-1")).expect("job 1");
        fs::write(dest.join(".pastegrab-1").join("video.part"), b"partial").expect("part");
        fs::create_dir_all(dest.join(".pastegrab-23")).expect("job 23");
        fs::create_dir_all(dest.join(".pastegrab-keep")).expect("other dotdir");
        fs::write(dest.join("Race.mp4"), b"video").expect("video");

        fs::create_dir_all(data.join("jobs").join("1")).expect("probe 1");
        fs::write(data.join("jobs").join("1").join("summary.json"), b"{}").expect("summary");
        fs::create_dir_all(data.join("tmp").join("sub")).expect("tmp sub");
        fs::write(data.join("tmp").join("junk.tmp"), b"x").expect("tmp file");
        fs::write(data.join("settings.json"), b"{}").expect("settings");
        fs::write(data.join("settings.tmp"), b"partial").expect("settings tmp");
        fs::create_dir_all(data.join("cache")).expect("cache");
        fs::write(data.join("cache").join("keep"), b"c").expect("cache file");

        fs::create_dir_all(root.join("app").join("0.2.0")).expect("installed");
        fs::write(
            root.join("app").join("0.2.0").join("pastegrab-app.exe"),
            b"exe",
        )
        .expect("exe");
        fs::write(root.join("app").join("0.3.0.download"), b"partial").expect("dl");
        fs::create_dir_all(root.join("app").join("0.3.0.part")).expect("part dir");
        fs::write(root.join("app").join("0.3.0.part").join("x"), b"x").expect("part file");
        fs::write(root.join("state.json"), b"{}").expect("state");
        fs::write(root.join("state.tmp"), b"partial").expect("state tmp");

        sweep_startup(&[&dest, &dest], &data, Some(&root));

        assert!(!dest.join(".pastegrab-1").exists());
        assert!(!dest.join(".pastegrab-23").exists());
        assert!(dest.join(".pastegrab-keep").exists());
        assert!(dest.join("Race.mp4").exists());

        assert!(data.join("jobs").exists());
        assert!(!data.join("jobs").join("1").exists());
        assert!(data.join("tmp").exists());
        assert!(!data.join("tmp").join("sub").exists());
        assert!(!data.join("tmp").join("junk.tmp").exists());
        assert!(data.join("settings.json").exists());
        assert!(!data.join("settings.tmp").exists());
        assert!(data.join("cache").join("keep").exists());

        assert!(
            root.join("app")
                .join("0.2.0")
                .join("pastegrab-app.exe")
                .exists()
        );
        assert!(!root.join("app").join("0.3.0.download").exists());
        assert!(!root.join("app").join("0.3.0.part").exists());
        assert!(root.join("state.json").exists());
        assert!(!root.join("state.tmp").exists());

        let _ = fs::remove_dir_all(&base);
    }
}
