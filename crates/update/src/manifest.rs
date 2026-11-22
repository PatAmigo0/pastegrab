//! `update.json`, the signed list of what a release offers.

use std::fmt;

use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;

use crate::Version;

/// `update.json`.
#[derive(Deserialize)]
struct Manifest {
    version: Version,
    /// Unix seconds; a replayed old manifest stops counting after this.
    expires: u64,
    packages: Vec<Package>,
}

/// One build of a release, for one target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    /// Such as `x86_64-windows`.
    pub target: String,
    /// The tar compressed with zstd, over https.
    pub url: String,
    /// Its SHA-256 in lowercase hex.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
}

impl Package {
    fn is_sound(&self) -> bool {
        let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        self.url.starts_with("https://") && self.sha256.len() == 64 && self.sha256.bytes().all(hex)
    }
}

/// A release to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// Its version.
    pub version: Version,
    /// The build for this PC.
    pub package: Package,
}

/// Why a manifest can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The release key didn't sign these bytes.
    Signature,
    /// Signed, but not a manifest this version reads.
    Malformed,
    /// Signed, but past its expiry.
    Expired,
    /// Signed, but without a build for this PC.
    NoPackage,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Signature => "the release key didn't sign the manifest",
            Self::Malformed => "the manifest is signed but unreadable",
            Self::Expired => "the manifest has expired",
            Self::NoPackage => "the release has no build for this PC",
        })
    }
}

impl std::error::Error for Refusal {}

/// Reads manifests for one install.
#[derive(Debug, Clone, Copy)]
pub struct Updater<'a> {
    /// The release public key, as `minisign -G` writes it.
    pub key: &'a str,
    /// The version that runs now.
    pub installed: Version,
    /// The package target this PC takes.
    pub target: &'a str,
    /// Versions rolled back on this PC.
    pub skip: &'a [Version],
}

impl Updater<'_> {
    /// The release to install: signed by the release key, unexpired at `now` in Unix seconds,
    /// newer than the installed version and never rolled back here.
    ///
    /// # Errors
    ///
    /// A `Refusal` for a manifest that isn't signed, readable, current or meant for this PC.
    pub fn read(
        &self,
        manifest: &[u8],
        signature: &str,
        now: u64,
    ) -> Result<Option<Offer>, Refusal> {
        self.verify(manifest, signature)?;
        let manifest: Manifest =
            serde_json::from_slice(manifest).map_err(|_| Refusal::Malformed)?;
        if !manifest.packages.iter().all(Package::is_sound) {
            return Err(Refusal::Malformed);
        }
        if manifest.expires <= now {
            return Err(Refusal::Expired);
        }
        if manifest.version <= self.installed || self.skip.contains(&manifest.version) {
            return Ok(None);
        }
        let package = manifest
            .packages
            .into_iter()
            .find(|package| package.target == self.target)
            .ok_or(Refusal::NoPackage)?;
        Ok(Some(Offer {
            version: manifest.version,
            package,
        }))
    }

    fn verify(&self, manifest: &[u8], signature: &str) -> Result<(), Refusal> {
        let key = PublicKey::decode(self.key).map_err(|_| Refusal::Signature)?;
        let signature = Signature::decode(signature).map_err(|_| Refusal::Signature)?;
        key.verify(manifest, &signature, false)
            .map_err(|_| Refusal::Signature)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use minisign::KeyPair;

    use super::{Refusal, Updater};
    use crate::Version;

    const NOW: u64 = 1_800_000_000;

    struct Signer(KeyPair);

    impl Signer {
        fn new() -> Self {
            Self(KeyPair::generate_unencrypted_keypair().expect("a key pair"))
        }

        fn public(&self) -> String {
            self.0.pk.to_box().expect("a key box").into_string()
        }

        fn sign(&self, text: &str) -> String {
            minisign::sign(None, &self.0.sk, Cursor::new(text), None, None)
                .expect("a signature")
                .into_string()
        }
    }

    fn manifest(version: &str, expires: u64) -> String {
        format!(
            r#"{{"version": "{version}", "expires": {expires}, "packages": [
                {{"target": "x86_64-windows", "size": 2900000,
                  "url": "https://github.com/PatAmigo0/pastegrab/releases/download/v{version}/app.tar.zst",
                  "sha256": "{}"}}]}}"#,
            "ab".repeat(32)
        )
    }

    fn updater<'a>(key: &'a str, skip: &'a [Version]) -> Updater<'a> {
        Updater {
            key,
            installed: "0.1.0".parse().expect("a version"),
            target: "x86_64-windows",
            skip,
        }
    }

    #[test]
    fn a_newer_signed_release_is_offered_with_its_package() {
        let signer = Signer::new();
        let (key, text) = (signer.public(), manifest("0.2.0", NOW + 60));
        let offer = updater(&key, &[])
            .read(text.as_bytes(), &signer.sign(&text), NOW)
            .expect("a valid manifest")
            .expect("an offer");
        assert_eq!(offer.version.to_string(), "0.2.0");
        assert_eq!(offer.package.size, 2_900_000);
        assert!(offer.package.url.ends_with("/v0.2.0/app.tar.zst"));
    }

    #[test]
    fn the_installed_an_older_or_a_rolled_back_version_is_no_offer() {
        let signer = Signer::new();
        let key = signer.public();
        let bad = ["0.2.0".parse().expect("a version")];
        for (version, skip) in [("0.1.0", &[][..]), ("0.0.9", &[][..]), ("0.2.0", &bad[..])] {
            let text = manifest(version, NOW + 60);
            let read = updater(&key, skip).read(text.as_bytes(), &signer.sign(&text), NOW);
            assert_eq!(read, Ok(None), "{version}");
        }
    }

    #[test]
    fn a_forged_stale_or_unusable_manifest_is_refused() {
        let (signer, stranger) = (Signer::new(), Signer::new());
        let key = signer.public();
        let text = manifest("0.2.0", NOW + 60);
        let tampered = text.replace("2900000", "2900001");
        let read =
            |text: &str, signature: &str| updater(&key, &[]).read(text.as_bytes(), signature, NOW);
        assert_eq!(
            read(&tampered, &signer.sign(&text)),
            Err(Refusal::Signature)
        );
        assert_eq!(read(&text, &stranger.sign(&text)), Err(Refusal::Signature));
        let stale = manifest("0.2.0", NOW - 1);
        assert_eq!(read(&stale, &signer.sign(&stale)), Err(Refusal::Expired));
        let other = text.replace("x86_64-windows", "aarch64-windows");
        assert_eq!(read(&other, &signer.sign(&other)), Err(Refusal::NoPackage));
        let plain = text.replace("https://", "http://");
        assert_eq!(read(&plain, &signer.sign(&plain)), Err(Refusal::Malformed));
        let short = text.replace(&"ab".repeat(32), "abcd");
        assert_eq!(read(&short, &signer.sign(&short)), Err(Refusal::Malformed));
    }
}
