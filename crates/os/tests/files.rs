//! Local files keep the files contract.

#![allow(clippy::expect_used, reason = "fixture helpers outside #[test] fns")]

use std::fs;

use pastegrab_os::LocalFiles;
use pastegrab_testkit::TempDir;
use pastegrab_testkit::contract;

#[test]
fn local_files_keep_the_files_contract() {
    let dir = TempDir::new("files").expect("a temp folder");
    contract::files(&LocalFiles, dir.path(), &|path| {
        fs::create_dir_all(path.parent().expect("a parent")).expect("folders");
        fs::write(path, b"media").expect("a file");
    });
}
