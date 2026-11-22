//! The person's choices in `settings.json`. A save writes a temporary file, flushes it and
//! renames it over the old one, so a crash leaves the old file or the new one.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use pastegrab_app::ports::{Choices, SettingsStore};
use pastegrab_domain::profile::ProfileKind;
use serde::{Deserialize, Serialize};

/// `settings.json` in pastegrab's data folder.
#[derive(Debug, Clone)]
pub struct JsonSettings {
    path: PathBuf,
}

impl JsonSettings {
    /// Settings kept at `path`.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dest: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
}

impl SettingsStore for JsonSettings {
    fn load(&self) -> Option<Choices> {
        let text = fs::read(&self.path).ok()?;
        // Notepad on Windows 8 saves UTF-8 with a byte order mark.
        let json = text.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&text);
        let saved: Saved = serde_json::from_slice(json).ok()?;
        Some(Choices {
            dest: saved.dest,
            profile: saved.profile.as_deref().and_then(profile),
        })
    }

    fn save(&self, choices: &Choices) -> io::Result<()> {
        let saved = Saved {
            dest: choices.dest.clone(),
            profile: choices.profile.map(|kind| name(kind).to_owned()),
        };
        let json = serde_json::to_vec_pretty(&saved).map_err(io::Error::other)?;
        write_whole(&self.path, &json)
    }
}

fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    // Flushed before the rename, so a crash leaves the old file or the whole new one.
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

const NAMES: [(ProfileKind, &str); 6] = [
    (ProfileKind::Video, "video"),
    (ProfileKind::BestQuality, "best"),
    (ProfileKind::Small, "small"),
    (ProfileKind::Audio, "audio"),
    (ProfileKind::AudioMp3, "mp3"),
    (ProfileKind::Analysis, "analysis"),
];

fn name(kind: ProfileKind) -> &'static str {
    NAMES
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("video", |(_, name)| name)
}

fn profile(name: &str) -> Option<ProfileKind> {
    NAMES
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(kind, _)| *kind)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pastegrab_app::ports::SettingsStore;
    use pastegrab_domain::profile::ProfileKind;
    use pastegrab_testkit::{TempDir, contract};

    use super::JsonSettings;

    #[test]
    fn json_settings_keep_the_settings_contract() {
        let dir = TempDir::new("store").expect("a temp folder");
        let path = dir.path().join("pastegrab").join("settings.json");
        contract::settings(&JsonSettings::new(path.clone()));
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn a_broken_file_reads_as_no_choices() {
        let dir = TempDir::new("store-broken").expect("a temp folder");
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ half").expect("a write");
        assert_eq!(JsonSettings::new(path.clone()).load(), None);
        fs::write(&path, r#"{"profile": "8k"}"#).expect("a write");
        let unknown = JsonSettings::new(path.clone())
            .load()
            .expect("the rest still reads");
        assert_eq!(unknown.profile, None);
        fs::write(&path, "\u{feff}{\"profile\": \"mp3\"}").expect("a write");
        let marked = JsonSettings::new(path)
            .load()
            .expect("a byte order mark is fine");
        assert_eq!(marked.profile, Some(ProfileKind::AudioMp3));
    }
}
