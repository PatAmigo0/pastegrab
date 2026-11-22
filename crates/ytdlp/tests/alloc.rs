//! A progress line allocates nothing once the first has gone through.

#![allow(unsafe_code, reason = "a counting allocator")]
#![allow(clippy::expect_used, reason = "fixture helpers outside #[test] fns")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pastegrab_app::Cancel;
use pastegrab_app::ports::{FetchRequest, Fetcher, Phase, Progress, ProgressSink, Source};
use pastegrab_domain::media::FormatId;
use pastegrab_domain::plan::FetchPlan;
use pastegrab_testkit::{ScriptedRunner, TempDir, Transcript};
use pastegrab_ytdlp::{Engine, YtDlp};

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

fn count() {
    let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
}

// SAFETY: every call goes straight to the system allocator with the caller's arguments.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller's contract for `alloc` holds for `System` too.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: `ptr` came from `System` with this layout.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

struct Snapshots(Mutex<Vec<usize>>);

impl ProgressSink for Snapshots {
    fn report(&self, phase: Phase, _: Progress) {
        if phase == Phase::Download {
            let now = ALLOCATIONS.with(Cell::get);
            self.0.lock().expect("snapshots").push(now);
        }
    }
}

#[test]
fn progress_lines_do_not_allocate() {
    let dir = TempDir::new("ytdlp-alloc").expect("a temp folder");
    let work = dir.path().join("work");
    let media = work.join("clip.mp4");
    let lines: Vec<String> = (1..=1000_u64)
        .map(|i| {
            let mut line = String::new();
            write!(
                line,
                "@pg|{}|1000000|NA|{}.25|{}",
                i * 1000,
                250_000 + i,
                1000 - i
            )
            .expect("a line");
            line
        })
        .collect();
    let transcript = Transcript {
        lines,
        ..Transcript::default()
    }
    .writes(&media, "media")
    .writes("result.txt", media.display().to_string());
    let runner = Arc::new(ScriptedRunner::default().script("/opt/yt-dlp", transcript));
    let engine = Engine::new(PathBuf::from("/opt/yt-dlp"), dir.path().join("cache"));
    let plan = FetchPlan {
        format_ids: FormatId::parse("18").into_iter().collect(),
        merge_into: None,
        section: None,
        expected_bytes: Some(1_000_000),
        encodes_video: false,
    };
    let source = Source::Engine {
        info_json: dir.path().join("probe.info.json"),
        impersonate: false,
    };
    let snapshots = Snapshots(Mutex::new(Vec::with_capacity(2000)));
    YtDlp::new(runner, engine)
        .fetch(
            FetchRequest {
                plan: &plan,
                source: &source,
                work_dir: &work,
            },
            &snapshots,
            &Cancel::default(),
        )
        .expect("a fetch");
    let taken = snapshots.0.lock().expect("snapshots");
    assert_eq!(taken.len(), 1000);
    assert_eq!(
        taken[1], taken[999],
        "allocations between the second and the last line"
    );
}
