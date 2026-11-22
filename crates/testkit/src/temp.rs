//! Temporary folders that remove themselves.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::{fs, io, process};

/// A fresh folder under the system's temporary folder, removed with everything in it on drop.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// Creates `pastegrab-<label>-<pid>-<n>`.
    ///
    /// # Errors
    ///
    /// The system's error when the folder can't be created.
    pub fn new(label: &str) -> io::Result<Self> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pastegrab-{label}-{}-{n}", process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    /// The folder.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
