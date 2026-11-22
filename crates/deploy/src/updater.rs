//! App updates: the newest signed release, downloaded, checked and unpacked beside the running
//! version, then staged for the launcher's next start.

use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use pastegrab_update::{Launch, Offer, Refusal, Updater, Version};
use sha2::{Digest, Sha256};

use crate::launch;
use crate::unpack::unpack;

/// The package target this build takes from a release.
#[cfg(target_arch = "x86")]
pub(crate) const TARGET: &str = "x86-windows";
#[cfg(not(target_arch = "x86"))]
pub(crate) const TARGET: &str = "x86_64-windows";

/// Where releases come from.
pub trait Releases {
    /// The newest release's `update.json` and its minisign signature.
    ///
    /// # Errors
    ///
    /// The network's.
    fn manifest(&self) -> io::Result<(Vec<u8>, String)>;

    /// Writes the file at `url` to `dest`, failing once it passes `limit` bytes.
    ///
    /// # Errors
    ///
    /// The network's, the disk's, or a file larger than `limit`.
    fn download(&self, url: &str, dest: &Path, limit: u64) -> io::Result<()>;
}

/// What a check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing newer than the running version.
    UpToDate,
    /// A version staged earlier waits for the next start.
    Waiting(Version),
    /// This version is unpacked and starts next time.
    Staged(Version),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UpToDate => f.write_str("up to date"),
            Self::Waiting(version) => write!(f, "{version} waits for the next start"),
            Self::Staged(version) => write!(f, "staged {version}"),
        }
    }
}

/// Why a check stopped.
#[derive(Debug)]
pub enum UpdateError {
    /// The release or its package couldn't be fetched.
    Network(io::Error),
    /// The manifest can't be trusted or used.
    Refused(Refusal),
    /// The package doesn't match the hash the signed manifest names.
    Hash,
    /// Unpacking or staging failed on this PC.
    Disk(io::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(e) => write!(f, "fetching the release: {e}"),
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Hash => f.write_str("the package doesn't match its signed hash"),
            Self::Disk(e) => write!(f, "installing the release: {e}"),
        }
    }
}

impl Error for UpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Network(e) | Self::Disk(e) => Some(e),
            Self::Refused(refusal) => Some(refusal),
            Self::Hash => None,
        }
    }
}

/// One update check for the install at `root`.
#[derive(Clone, Copy)]
pub struct AppUpdate<'a> {
    /// The install folder.
    pub root: &'a Path,
    /// The version that runs now.
    pub installed: Version,
    /// The release public key, as `minisign -G` writes it.
    pub key: &'a str,
    /// Where releases come from.
    pub releases: &'a dyn Releases,
}

impl AppUpdate<'_> {
    /// Checks the newest release at `now`, in Unix seconds, and stages it when it is newer than
    /// the running version and never rolled back here.
    ///
    /// # Errors
    ///
    /// An `UpdateError` for a release that can't be fetched, trusted or installed; nothing is
    /// staged then.
    pub fn run(&self, now: u64) -> Result<Outcome, UpdateError> {
        let mut state =
            launch::read_state(self.root).unwrap_or_else(|| Launch::new(self.installed));
        if state.current() != self.installed {
            return Ok(Outcome::Waiting(state.current()));
        }
        let (manifest, signature) = self.releases.manifest().map_err(UpdateError::Network)?;
        let updater = Updater {
            key: self.key,
            installed: self.installed,
            target: TARGET,
            skip: state.bad(),
        };
        let Some(offer) = updater
            .read(&manifest, &signature, now)
            .map_err(UpdateError::Refused)?
        else {
            return Ok(Outcome::UpToDate);
        };
        self.install(&offer)?;
        state.stage(offer.version);
        launch::write_state(self.root, &state).map_err(UpdateError::Disk)?;
        Ok(Outcome::Staged(offer.version))
    }

    /// Puts the package's files in `app\<version>\`, whole or not at all.
    fn install(&self, offer: &Offer) -> Result<(), UpdateError> {
        let place = |suffix: &str| -> PathBuf {
            self.root
                .join("app")
                .join(format!("{}{suffix}", offer.version))
        };
        let (download, part) = (place(".download"), place(".part"));
        let result = self.unpack_checked(offer, &download, &part, &place(""));
        let _ = fs::remove_file(&download);
        let _ = fs::remove_dir_all(&part);
        result
    }

    fn unpack_checked(
        &self,
        offer: &Offer,
        download: &Path,
        part: &Path,
        folder: &Path,
    ) -> Result<(), UpdateError> {
        let package = &offer.package;
        self.releases
            .download(&package.url, download, package.size)
            .map_err(UpdateError::Network)?;
        if file_sha256_hex(download).map_err(UpdateError::Disk)? != package.sha256 {
            return Err(UpdateError::Hash);
        }
        let disk = UpdateError::Disk;
        remove_tree(part).map_err(disk)?;
        unpack(File::open(download).map_err(disk)?, part).map_err(disk)?;
        // A folder left by an earlier try that never got staged.
        remove_tree(folder).map_err(disk)?;
        fs::rename(part, folder).map_err(disk)
    }
}

fn remove_tree(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn file_sha256_hex(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

/// The SHA-256 of `data` in lowercase hex, as `update.json` writes it.
#[cfg(test)]
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::fs;
    use std::io::{self, Cursor};
    use std::path::Path;

    use minisign::KeyPair;
    use pastegrab_update::{Launch, Refusal, Version};
    use ruzstd::encoding::{CompressionLevel, compress_to_vec};

    use super::{AppUpdate, Outcome, Releases, UpdateError, sha256_hex};
    use crate::launch::{app_exe, read_state, write_state};

    const NOW: u64 = 1_800_000_000;
    const URL: &str = "https://github.com/PatAmigo0/pastegrab/releases/download/v0.2.0/app.tar.zst";

    fn v(text: &str) -> Version {
        text.parse().expect("a version")
    }

    fn package() -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        let name = app_exe(Path::new(""), v("0.2.0"));
        let name = name.file_name().expect("a name");
        builder
            .append_data(&mut header, name, &b"new"[..])
            .expect("an entry");
        compress_to_vec(
            builder.into_inner().expect("a tar").as_slice(),
            CompressionLevel::Fastest,
        )
    }

    struct Fake {
        keys: KeyPair,
        manifest: String,
        packages: HashMap<String, Vec<u8>>,
        downloads: Cell<u32>,
    }

    impl Fake {
        fn new(version: &str, sha256: &str) -> Self {
            let manifest = format!(
                r#"{{"version": "{version}", "expires": {}, "packages": [{{"target": "{}",
                    "url": "{URL}", "sha256": "{sha256}", "size": 100000}}]}}"#,
                NOW + 60,
                super::TARGET
            );
            Self {
                keys: KeyPair::generate_unencrypted_keypair().expect("a key pair"),
                manifest,
                packages: HashMap::from([(URL.to_owned(), package())]),
                downloads: Cell::new(0),
            }
        }

        fn key(&self) -> String {
            self.keys.pk.to_box().expect("a key box").into_string()
        }
    }

    impl Releases for Fake {
        fn manifest(&self) -> io::Result<(Vec<u8>, String)> {
            let signature =
                minisign::sign(None, &self.keys.sk, Cursor::new(&self.manifest), None, None)
                    .map_err(io::Error::other)?
                    .into_string();
            Ok((self.manifest.clone().into_bytes(), signature))
        }

        fn download(&self, url: &str, dest: &Path, limit: u64) -> io::Result<()> {
            self.downloads.set(self.downloads.get() + 1);
            let bytes = self.packages.get(url).ok_or(io::ErrorKind::NotFound)?;
            if bytes.len() as u64 > limit {
                return Err(io::Error::other("past the size the manifest names"));
            }
            fs::write(dest, bytes)
        }
    }

    fn install(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("pastegrab-updater-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let exe = app_exe(&root, v("0.1.0"));
        fs::create_dir_all(exe.parent().expect("a folder")).expect("a version folder");
        fs::write(&exe, b"old").expect("an exe");
        write_state(&root, &Launch::new(v("0.1.0"))).expect("a state");
        root
    }

    fn run(root: &Path, releases: &Fake, key: &str) -> Result<Outcome, UpdateError> {
        AppUpdate {
            root,
            installed: v("0.1.0"),
            key,
            releases,
        }
        .run(NOW)
    }

    #[test]
    fn a_newer_signed_release_is_unpacked_and_staged_for_the_next_start() {
        let root = install("staged");
        let releases = Fake::new("0.2.0", &sha256_hex(&package()));
        let outcome = run(&root, &releases, &releases.key()).expect("an update");
        assert!(matches!(outcome, Outcome::Staged(version) if version == v("0.2.0")));
        assert_eq!(
            fs::read(app_exe(&root, v("0.2.0"))).expect("the new exe"),
            b"new"
        );
        let mut expected = Launch::new(v("0.1.0"));
        expected.stage(v("0.2.0"));
        assert_eq!(read_state(&root), Some(expected));
        let leftovers: Vec<_> = fs::read_dir(root.join("app"))
            .expect("the app folder")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(
            leftovers.len(),
            2,
            "only the two version folders: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_running_or_a_rolled_back_version_downloads_nothing() {
        let root = install("current");
        let same = Fake::new("0.1.0", &sha256_hex(&package()));
        assert!(matches!(
            run(&root, &same, &same.key()),
            Ok(Outcome::UpToDate)
        ));
        let mut state = Launch::new(v("0.1.0"));
        state.stage(v("0.2.0"));
        for _ in 0..3 {
            let _ = state.start();
        }
        write_state(&root, &state).expect("a state with 0.2.0 rolled back");
        let bad = Fake::new("0.2.0", &sha256_hex(&package()));
        assert!(matches!(
            run(&root, &bad, &bad.key()),
            Ok(Outcome::UpToDate)
        ));
        assert_eq!(same.downloads.get() + bad.downloads.get(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_staged_version_waits_for_its_start() {
        let root = install("waiting");
        let mut state = Launch::new(v("0.1.0"));
        state.stage(v("0.1.5"));
        write_state(&root, &state).expect("a staged state");
        let releases = Fake::new("0.2.0", &sha256_hex(&package()));
        let outcome = run(&root, &releases, &releases.key()).expect("an answer");
        assert!(matches!(outcome, Outcome::Waiting(version) if version == v("0.1.5")));
        assert_eq!(releases.downloads.get(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_wrong_hash_or_a_stranger_signature_installs_nothing() {
        let root = install("refused");
        let tampered = Fake::new("0.2.0", &"00".repeat(32));
        let outcome = run(&root, &tampered, &tampered.key());
        assert!(matches!(outcome, Err(UpdateError::Hash)), "{outcome:?}");
        let stranger = Fake::new("0.2.0", &sha256_hex(&package()));
        let other_key = Fake::new("0.2.0", "").key();
        let outcome = run(&root, &stranger, &other_key);
        assert!(
            matches!(outcome, Err(UpdateError::Refused(Refusal::Signature))),
            "{outcome:?}"
        );
        assert!(!app_exe(&root, v("0.2.0")).exists());
        assert_eq!(read_state(&root), Some(Launch::new(v("0.1.0"))));
        let names: Vec<_> = fs::read_dir(root.join("app"))
            .expect("the app folder")
            .flatten()
            .collect();
        assert_eq!(names.len(), 1, "no download or half-unpacked folder stays");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn outcomes_read_as_plain_words() {
        assert_eq!(Outcome::UpToDate.to_string(), "up to date");
        assert_eq!(
            Outcome::Waiting(v("0.1.1")).to_string(),
            "0.1.1 waits for the next start"
        );
        assert_eq!(Outcome::Staged(v("0.1.1")).to_string(), "staged 0.1.1");
    }
}
