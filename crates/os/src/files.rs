use std::fs;
use std::io;
use std::path::Path;

use pastegrab_app::ports::Files;

use crate::sys;

/// Files on the local disks.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFiles;

impl Files for LocalFiles {
    fn exists(&self, path: &Path) -> bool {
        // A path nobody can check counts as taken, so it never gets overwritten.
        !matches!(fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound)
    }

    fn place(&self, from: &Path, to: &Path) -> io::Result<()> {
        // Marked first, the file never shows up under its final name unmarked.
        sys::mark_downloaded(from);
        sys::move_no_replace(from, to)
    }

    fn remove_tree(&self, path: &Path) -> io::Result<()> {
        match fs::remove_dir_all(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            done => done,
        }
    }
}
