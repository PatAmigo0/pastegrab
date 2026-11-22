//! Contract suites: what every implementation of a port must do, fakes included. Each suite
//! panics on the first broken rule, which fails the test that runs it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    reason = "a broken contract fails the test that runs it"
)]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pastegrab_app::Cancel;
use pastegrab_app::ports::{Choices, Files, Lines, ProcSpec, ProcessRunner, SettingsStore};
use pastegrab_domain::job::FailureKind;
use pastegrab_domain::profile::{HeightCap, ProfileKind};

/// `place` moves without ever replacing, and `remove_tree` takes a folder and everything in it.
/// `create` makes a file, parent folders included.
pub fn files(files: &dyn Files, root: &Path, create: &dyn Fn(&Path)) {
    let (part, done) = (root.join("a.part"), root.join("a.mp4"));
    create(&part);
    files
        .place(&part, &done)
        .expect("a free name takes the file");
    assert!(files.exists(&done), "the file is at its new name");
    assert!(!files.exists(&part), "and gone from the old one");

    let other = root.join("b.part");
    create(&other);
    let refused = files
        .place(&other, &done)
        .expect_err("an existing file is never replaced");
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
    assert!(
        files.exists(&other) && files.exists(&done),
        "both files stay"
    );

    let missing = files.place(&root.join("none.part"), &root.join("none.mp4"));
    assert_eq!(missing.map_err(|e| e.kind()), Err(io::ErrorKind::NotFound));

    let work = root.join("work");
    let deep = work.join("1").join("video.f137.mp4");
    create(&deep);
    files.remove_tree(&work).expect("a work folder goes");
    assert!(!files.exists(&deep));
    files.remove_tree(&work).expect("a missing folder is fine");
}

/// A fresh store has nothing, a save reads back whole, and the next save replaces it.
pub fn settings(store: &dyn SettingsStore) {
    assert_eq!(store.load(), None, "a fresh store has no choices");
    let first = Choices {
        dest: Some(PathBuf::from("D:\\Видео\\Забеги")),
        profile: Some(ProfileKind::Analysis),
        tutorial: Some(1),
        clipboard: Some(false),
        close_to_tray: Some(true),
        max_height: Some(HeightCap::AtMost(1080)),
        downloads: Some(3),
    };
    store.save(&first).expect("a save");
    assert_eq!(store.load(), Some(first));
    let second = Choices {
        dest: None,
        profile: Some(ProfileKind::Audio),
        tutorial: None,
        clipboard: None,
        close_to_tray: None,
        max_height: Some(HeightCap::Unlimited),
        downloads: None,
    };
    store.save(&second).expect("a second save");
    assert_eq!(store.load(), Some(second));
}

/// The programs the runner contract starts.
#[derive(Debug, Clone)]
pub struct Programs {
    /// Prints `out` on stdout, then `err` on stderr, and exits with 3.
    pub talk: ProcSpec,
    /// Prints `ready` and runs until killed.
    pub hang: ProcSpec,
    /// A program that doesn't exist.
    pub missing: ProcSpec,
}

/// Output arrives merged and in order with its exit code, a cancel from another thread ends a
/// running child, and a program that can't start is `ToolBroken`.
pub fn process_runner(runner: &dyn ProcessRunner, programs: &Programs) {
    let cancel = Cancel::default();
    let mut talk = runner.spawn(&programs.talk, &cancel).expect("talk starts");
    assert_eq!(read_all(&mut *talk), ["out", "err"]);
    assert_eq!(talk.wait().expect("talk ends").code, Some(3));

    let cancel = Arc::new(Cancel::default());
    let mut hang = runner.spawn(&programs.hang, &cancel).expect("hang starts");
    let mut line = Vec::new();
    assert!(hang.next_line(&mut line).expect("a line"));
    assert_eq!(line, b"ready");
    let canceller = {
        let cancel = Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel.cancel();
        })
    };
    assert!(
        !hang.next_line(&mut line).expect("the read ends"),
        "a cancel ends the output"
    );
    assert!(!hang.wait().expect("the child is reaped").success());
    canceller.join().expect("the cancel thread ends");

    match runner.spawn(&programs.missing, &Cancel::default()) {
        Ok(_) => panic!("a missing program started"),
        Err(failure) => assert_eq!(failure.kind, FailureKind::ToolBroken),
    }
}

fn read_all(lines: &mut dyn Lines) -> Vec<String> {
    let mut buf = Vec::new();
    let mut all = Vec::new();
    while lines.next_line(&mut buf).expect("a line") {
        all.push(String::from_utf8_lossy(&buf).into_owned());
    }
    all
}
