//! Release downloads against a small HTTP server on localhost.

mod common;

use std::fs;
use std::io;

use common::serve;
use pastegrab_http::Downloads;
use pastegrab_testkit::TempDir;

#[test]
fn a_small_file_reads_whole_and_a_larger_one_is_refused() {
    let downloads = Downloads::default();
    let url = serve(b"{\"version\": \"0.2.0\"}".to_vec(), 200, 2);
    assert_eq!(
        downloads.read(&url, 100).expect("the manifest"),
        b"{\"version\": \"0.2.0\"}"
    );
    let refused = downloads.read(&url, 5).expect_err("past the limit");
    assert_eq!(refused.kind(), io::ErrorKind::FileTooLarge);
}

#[test]
fn a_package_streams_to_disk_and_a_failed_one_leaves_nothing() {
    let downloads = Downloads::default();
    let dir = TempDir::new("downloads").expect("a temp folder");
    let dest = dir.path().join("0.2.0.download");
    let url = serve(vec![9; 300_000], 200, 2);
    downloads.save(&url, &dest, 300_000).expect("the package");
    assert_eq!(fs::read(&dest).expect("the file").len(), 300_000);
    fs::remove_file(&dest).expect("a cleanup");
    let refused = downloads
        .save(&url, &dest, 299_999)
        .expect_err("past the limit");
    assert_eq!(refused.kind(), io::ErrorKind::FileTooLarge);
    assert!(!dest.exists(), "no partial file stays");
}

#[test]
fn a_missing_release_file_is_not_found() {
    let url = serve(Vec::new(), 404, 1);
    let missing = Downloads::default().read(&url, 100).expect_err("a 404");
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
}
