//! Unpacking the payload: a tar compressed with zstd, laid out the way the install folder is.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ruzstd::decoding::StreamingDecoder;

/// Unpacks `payload` next to `dest` and swaps it in, so a failure leaves the old install whole.
/// `read` counts the compressed bytes consumed, for a progress bar.
///
/// # Errors
///
/// A broken payload, a full disk, or an old install whose files are in use.
pub fn install(payload: &[u8], dest: &Path, read: &AtomicU64) -> io::Result<()> {
    let fresh = sibling(dest, "new");
    let old = sibling(dest, "old");
    remove(&fresh)?;
    unpack(payload, &fresh, read)?;
    remove(&old)?;
    if dest.exists() {
        fs::rename(dest, &old)?;
    }
    fs::rename(&fresh, dest)?;
    // A file still open in the old copy keeps it on disk; the next setup removes it.
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

fn unpack(payload: &[u8], dest: &Path, read: &AtomicU64) -> io::Result<()> {
    let counted = Counted {
        rest: payload,
        read,
    };
    let decoder = StreamingDecoder::new(counted).map_err(io::Error::other)?;
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(false);
    archive.set_preserve_mtime(false);
    fs::create_dir_all(dest)?;
    // `unpack` refuses entries that would land outside `dest`.
    archive.unpack(dest)
}

fn sibling(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_owned();
    name.push(".");
    name.push(suffix);
    dest.with_file_name(name)
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

struct Counted<'a> {
    rest: &'a [u8],
    read: &'a AtomicU64,
}

impl Read for Counted<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.rest.read(buf)?;
        self.read
            .fetch_add(u64::try_from(n).unwrap_or(u64::MAX), Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use ruzstd::encoding::{CompressionLevel, compress_to_vec};

    use super::install;

    fn payload(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, *bytes)
                .expect("an entry");
        }
        let tar = builder.into_inner().expect("a tar");
        compress_to_vec(tar.as_slice(), CompressionLevel::Fastest)
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pastegrab-setup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a temp folder");
        dir
    }

    #[test]
    fn a_second_install_replaces_the_first_whole() {
        let root = temp("replace");
        let dest = root.join("pastegrab");
        let first = payload(&[
            ("tools/qjs/qjs.exe", b"one"),
            ("app/old.exe", b"gone later"),
        ]);
        let read = AtomicU64::new(0);
        install(&first, &dest, &read).expect("the first install");
        assert_eq!(read.load(Ordering::Relaxed), first.len() as u64);
        assert_eq!(
            fs::read(dest.join("tools/qjs/qjs.exe")).expect("a file"),
            b"one"
        );

        let second = payload(&[("tools/qjs/qjs.exe", b"two")]);
        install(&second, &dest, &AtomicU64::new(0)).expect("the second install");
        assert_eq!(
            fs::read(dest.join("tools/qjs/qjs.exe")).expect("a file"),
            b"two"
        );
        assert!(!dest.join("app/old.exe").exists());
        assert!(!root.join("pastegrab.new").exists() && !root.join("pastegrab.old").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_payload_leaves_the_install_alone() {
        let root = temp("broken");
        let dest = root.join("pastegrab");
        install(&payload(&[("a.txt", b"kept")]), &dest, &AtomicU64::new(0)).expect("an install");
        let mut broken = payload(&[("a.txt", b"never")]);
        broken.truncate(broken.len() / 2);
        assert!(install(&broken, &dest, &AtomicU64::new(0)).is_err());
        assert_eq!(fs::read(dest.join("a.txt")).expect("a file"), b"kept");
        assert!(Path::new(&root.join("pastegrab")).exists());
        let _ = fs::remove_dir_all(&root);
    }
}
