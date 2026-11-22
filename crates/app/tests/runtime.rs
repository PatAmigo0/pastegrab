//! The runtime with fake adapters: threads, workers and the queue together.

// Fixture helpers outside #[test] fns fall outside clippy's test exemption.
#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pastegrab_app::ports::{
    FetchRequest, Fetcher, Files, ProgressSink, Resolved, Source, ViewUpdate,
};
use pastegrab_app::queue::{Input, Paste, Queue, Settings};
use pastegrab_app::runtime;
use pastegrab_app::steps::Ports;
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::{FailureKind, JobId, JobState, Permits};
use pastegrab_domain::media::FileProbe;
use pastegrab_domain::plan::Machine;
use pastegrab_domain::profile::{HeightCap, ProfileKind};
use pastegrab_testkit::{
    FakeFetcher, FakeProbe, FakeResolver, FakeTranscoder, MemFiles, RecordingView, media,
    mp4_probe, webm_probe,
};

struct Run {
    states: Vec<(JobId, JobState)>,
    files: MemFiles,
    view: Arc<RecordingView>,
}

fn run(text: &str, probe: FileProbe, fetch_failures: Vec<Result<(), Failure>>) -> Run {
    let files = MemFiles::default();
    let resolved = Resolved {
        info: media("Атланта 1996: финал"),
        source: Source::Engine {
            info_json: PathBuf::from("/data/jobs/1/probe.info.json"),
            impersonate: false,
        },
    };
    let ports = Ports {
        resolvers: vec![Box::new(FakeResolver::new(vec![Ok(resolved)]))],
        fetchers: vec![Box::new(FakeFetcher::new(
            files.clone(),
            "download.mkv",
            fetch_failures,
        ))],
        probe: Box::new(FakeProbe(probe)),
        transcoder: Box::new(FakeTranscoder::new(files.clone())),
        files: Box::new(files.clone()),
        settings: None,
    };
    let queue = Queue::new(Settings {
        profile: ProfileKind::Video,
        machine: Machine { screen_height: 768 },
        dest: PathBuf::from("/videos"),
        data: PathBuf::from("/data"),
        permits: Permits::DEFAULT,
        tutorial: 0,
        clipboard: true,
        close_to_tray: false,
        height_cap: HeightCap::Screen,
        downloads_override: None,
    });
    let view = Arc::new(RecordingView::default());
    let (handle, thread) = runtime::start(queue, Arc::new(ports), view.clone(), true);
    assert!(handle.send(Input::Paste(Paste {
        text: text.into(),
        profile: None,
        section: None,
        hold: false,
    })));
    drop(handle);
    let states = thread.join().expect("the queue thread ends");
    Run {
        states,
        files,
        view,
    }
}

#[test]
fn a_link_ends_as_a_named_file_and_leaves_no_working_files() {
    let run = run("https://youtu.be/AAAAAAAAAAA", mp4_probe(), vec![]);
    assert_eq!(run.states, [(JobId(1), JobState::Done)]);
    assert_eq!(
        run.files.all(),
        [PathBuf::from("/videos/Атланта 1996 - финал.mp4")]
    );
    assert!(
        run.view
            .updates()
            .iter()
            .any(|u| matches!(u, ViewUpdate::Finished { .. }))
    );
    assert!(!run.view.progress.lock().expect("progress").is_empty());
}

#[test]
fn a_file_in_the_wrong_codecs_goes_through_the_transcoder() {
    let run = run("https://youtu.be/AAAAAAAAAAA", webm_probe(), vec![]);
    assert_eq!(run.states, [(JobId(1), JobState::Done)]);
    assert_eq!(
        run.files.all(),
        [PathBuf::from("/videos/Атланта 1996 - финал.mp4")]
    );
}

#[test]
fn a_network_failure_is_retried_and_a_disk_full_one_is_not() {
    let flaky = run(
        "https://youtu.be/AAAAAAAAAAA",
        mp4_probe(),
        vec![Err(Failure::new(FailureKind::Network, "reset"))],
    );
    assert_eq!(flaky.states, [(JobId(1), JobState::Done)]);

    let full = run(
        "https://youtu.be/AAAAAAAAAAA",
        mp4_probe(),
        vec![Err(Failure::new(FailureKind::DiskFull, "no space"))],
    );
    assert_eq!(
        full.states,
        [(JobId(1), JobState::Failed(FailureKind::DiskFull))]
    );
}

struct BlockingFetcher {
    files: MemFiles,
    started: Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
    still_present_on_exit: Arc<std::sync::atomic::AtomicBool>,
}

impl Fetcher for BlockingFetcher {
    fn accepts(&self, _: &Source) -> bool {
        true
    }

    fn fetch(
        &self,
        request: FetchRequest<'_>,
        _: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<PathBuf, Failure> {
        let part = request.work_dir.join("part.bin");
        self.files.create(&part);
        if let Some(tx) = self.started.lock().expect("started").take() {
            let _ = tx.send(());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cancel.is_cancelled() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.still_present_on_exit.store(
            self.files.exists(&part),
            std::sync::atomic::Ordering::SeqCst,
        );
        Err(Failure::new(FailureKind::Network, "cancelled"))
    }
}

#[test]
fn cancelling_a_running_fetch_cleans_working_folders_after_worker_exits() {
    let files = MemFiles::default();
    let probe_file = PathBuf::from("/data/jobs/1/probe.info.json");
    files.create(&probe_file);
    let resolved = Resolved {
        info: media("Атланта 1996: финал"),
        source: Source::Engine {
            info_json: probe_file,
            impersonate: false,
        },
    };
    let (started_tx, started_rx) = sync_channel(1);
    let still_present_on_exit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ports = Ports {
        resolvers: vec![Box::new(FakeResolver::new(vec![Ok(resolved)]))],
        fetchers: vec![Box::new(BlockingFetcher {
            files: files.clone(),
            started: Mutex::new(Some(started_tx)),
            still_present_on_exit: Arc::clone(&still_present_on_exit),
        })],
        probe: Box::new(FakeProbe(mp4_probe())),
        transcoder: Box::new(FakeTranscoder::new(files.clone())),
        files: Box::new(files.clone()),
        settings: None,
    };
    let queue = Queue::new(Settings {
        profile: ProfileKind::Video,
        machine: Machine { screen_height: 768 },
        dest: PathBuf::from("/videos"),
        data: PathBuf::from("/data"),
        permits: Permits::DEFAULT,
        tutorial: 0,
        clipboard: true,
        close_to_tray: false,
        height_cap: HeightCap::Screen,
        downloads_override: None,
    });
    let view = Arc::new(RecordingView::default());
    let (handle, thread) = runtime::start(queue, Arc::new(ports), view, true);
    assert!(handle.send(Input::Paste(Paste {
        text: "https://youtu.be/AAAAAAAAAAA".into(),
        profile: None,
        section: None,
        hold: false,
    })));
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fetch started");
    assert!(handle.send(Input::Cancel(JobId(1))));
    let states = thread.join().expect("the queue thread ends");
    assert_eq!(states, [(JobId(1), JobState::Cancelled)]);
    assert!(still_present_on_exit.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(files.all(), Vec::<PathBuf>::new());
}

#[test]
fn cancelling_a_held_job_removes_its_probe_folder_right_away() {
    let files = MemFiles::default();
    let probe_file = PathBuf::from("/data/jobs/1/probe.info.json");
    files.create(&probe_file);
    let resolved = Resolved {
        info: media("Атланта 1996: финал"),
        source: Source::Engine {
            info_json: probe_file,
            impersonate: false,
        },
    };
    let ports = Ports {
        resolvers: vec![Box::new(FakeResolver::new(vec![Ok(resolved)]))],
        fetchers: vec![Box::new(FakeFetcher::new(
            files.clone(),
            "download.mkv",
            vec![],
        ))],
        probe: Box::new(FakeProbe(mp4_probe())),
        transcoder: Box::new(FakeTranscoder::new(files.clone())),
        files: Box::new(files.clone()),
        settings: None,
    };
    let queue = Queue::new(Settings {
        profile: ProfileKind::Video,
        machine: Machine { screen_height: 768 },
        dest: PathBuf::from("/videos"),
        data: PathBuf::from("/data"),
        permits: Permits::DEFAULT,
        tutorial: 0,
        clipboard: true,
        close_to_tray: false,
        height_cap: HeightCap::Screen,
        downloads_override: None,
    });
    let view = Arc::new(RecordingView::default());
    let (handle, thread) = runtime::start(queue, Arc::new(ports), view.clone(), false);
    assert!(handle.send(Input::Paste(Paste {
        text: "https://youtu.be/AAAAAAAAAAA".into(),
        profile: None,
        section: None,
        hold: true,
    })));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.updates().iter().any(|u| {
        matches!(
            u,
            ViewUpdate::State {
                state: JobState::Ready,
                ..
            }
        )
    }) {
        assert!(Instant::now() < deadline, "probe reached Ready");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(handle.send(Input::Cancel(JobId(1))));
    drop(handle);
    let states = thread.join().expect("the queue thread ends");
    assert_eq!(states, [(JobId(1), JobState::Cancelled)]);
    assert_eq!(files.all(), Vec::<PathBuf>::new());
}

#[test]
fn shutdown_stops_running_fetch_and_cleans_before_exit() {
    let files = MemFiles::default();
    let probe_file = PathBuf::from("/data/jobs/1/probe.info.json");
    files.create(&probe_file);
    let resolved = Resolved {
        info: media("Атланта 1996: финал"),
        source: Source::Engine {
            info_json: probe_file,
            impersonate: false,
        },
    };
    let (started_tx, started_rx) = sync_channel(1);
    let still_present_on_exit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ports = Ports {
        resolvers: vec![Box::new(FakeResolver::new(vec![Ok(resolved)]))],
        fetchers: vec![Box::new(BlockingFetcher {
            files: files.clone(),
            started: Mutex::new(Some(started_tx)),
            still_present_on_exit: Arc::clone(&still_present_on_exit),
        })],
        probe: Box::new(FakeProbe(mp4_probe())),
        transcoder: Box::new(FakeTranscoder::new(files.clone())),
        files: Box::new(files.clone()),
        settings: None,
    };
    let queue = Queue::new(Settings {
        profile: ProfileKind::Video,
        machine: Machine { screen_height: 768 },
        dest: PathBuf::from("/videos"),
        data: PathBuf::from("/data"),
        permits: Permits::DEFAULT,
        tutorial: 0,
        clipboard: true,
        close_to_tray: false,
        height_cap: HeightCap::Screen,
        downloads_override: None,
    });
    let view = Arc::new(RecordingView::default());
    let (handle, thread) = runtime::start(queue, Arc::new(ports), view, false);
    assert!(handle.send(Input::Paste(Paste {
        text: "https://youtu.be/AAAAAAAAAAA".into(),
        profile: None,
        section: None,
        hold: false,
    })));
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fetch started");
    handle.shutdown();
    let states = thread.join().expect("the queue thread ends");
    assert_eq!(states, [(JobId(1), JobState::Cancelled)]);
    assert!(still_present_on_exit.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(files.all(), Vec::<PathBuf>::new());
}
