//! Files written whole: a temporary file, flushed, then renamed over the old one, so a crash
//! leaves the old file or the new one.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("tmp");
    let mut file = File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}
