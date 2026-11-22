//! The local runner on Unix: the shared contract, then what only real processes show.

#![cfg(unix)]
#![allow(clippy::expect_used, reason = "fixture helpers outside #[test] fns")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, thread};

use pastegrab_app::Cancel;
use pastegrab_app::ports::{Lines, ProcSpec, ProcessRunner};
use pastegrab_os::LocalRunner;
use pastegrab_testkit::contract::{self, Programs};

fn sh(script: &str) -> ProcSpec {
    ProcSpec {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".into(), script.into()],
        ..ProcSpec::default()
    }
}

fn first_line(lines: &mut dyn Lines) -> String {
    let mut buf = Vec::new();
    assert!(lines.next_line(&mut buf).expect("a line"));
    String::from_utf8(buf).expect("ascii")
}

/// Whether `pid` is gone or only waits for its parent to reap it.
fn ended(pid: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => return true,
            Ok(stat)
                if stat
                    .rsplit(')')
                    .next()
                    .is_some_and(|s| s.trim_start().starts_with('Z')) =>
            {
                return true;
            }
            Ok(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
    false
}

#[test]
fn the_local_runner_keeps_the_runner_contract() {
    let programs = Programs {
        talk: sh("echo out; echo err >&2; exit 3"),
        hang: sh("echo ready; exec sleep 60"),
        missing: ProcSpec {
            program: PathBuf::from("/nonexistent/pastegrab-tool"),
            ..ProcSpec::default()
        },
    };
    contract::process_runner(&LocalRunner::new(), &programs);
}

#[test]
fn a_cancel_kills_what_the_child_started() {
    let cancel = Arc::new(Cancel::default());
    let mut lines = LocalRunner::new()
        .spawn(&sh("sleep 60 & echo $!; wait"), &cancel)
        .expect("starts");
    let grandchild = first_line(&mut *lines);
    cancel.cancel();
    let mut buf = Vec::new();
    assert!(!lines.next_line(&mut buf).expect("the read ends"));
    assert!(!lines.wait().expect("reaped").success());
    assert!(ended(&grandchild), "sleep {grandchild} outlived the cancel");
}

#[test]
fn what_the_child_leaves_running_dies_when_it_ends() {
    let cancel = Cancel::default();
    let mut lines = LocalRunner::new()
        .spawn(&sh("sleep 60 >/dev/null 2>&1 & echo $!"), &cancel)
        .expect("starts");
    let leftover = first_line(&mut *lines);
    assert!(lines.wait().expect("reaped").success());
    assert!(ended(&leftover), "sleep {leftover} outlived its parent");
}

#[test]
fn a_cancel_before_the_start_kills_the_child_at_once() {
    let cancel = Cancel::default();
    cancel.cancel();
    let started = Instant::now();
    let lines = LocalRunner::new()
        .spawn(&sh("exec sleep 60"), &cancel)
        .expect("starts");
    assert!(!lines.wait().expect("reaped").success());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[cfg(target_os = "linux")]
#[test]
fn low_priority_children_run_at_nice_10() {
    let spec = ProcSpec {
        low_priority: true,
        ..sh("cut -d' ' -f19 /proc/self/stat")
    };
    let mut lines = LocalRunner::new()
        .spawn(&spec, &Cancel::default())
        .expect("starts");
    assert_eq!(first_line(&mut *lines), "10");
    assert!(lines.wait().expect("reaped").success());
}

#[test]
fn a_long_line_arrives_in_pieces() {
    let mut lines = LocalRunner::new()
        .spawn(
            &sh("head -c 200000 /dev/zero | tr '\\0' x"),
            &Cancel::default(),
        )
        .expect("starts");
    let mut buf = Vec::new();
    let mut total = 0;
    while lines.next_line(&mut buf).expect("a piece") {
        assert!(buf.len() <= 64 * 1024);
        total += buf.len();
    }
    assert_eq!(total, 200_000);
    assert!(lines.wait().expect("reaped").success());
}
