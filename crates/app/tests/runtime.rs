//! The runtime with fake adapters: threads, workers and the queue together.

// Fixture helpers outside #[test] fns fall outside clippy's test exemption.
#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;

use pastegrab_app::Failure;
use pastegrab_app::ports::{Resolved, Source, ViewUpdate};
use pastegrab_app::queue::{Input, Paste, Queue, Settings};
use pastegrab_app::runtime;
use pastegrab_app::steps::Ports;
use pastegrab_domain::job::{FailureKind, JobId, JobState, Permits};
use pastegrab_domain::media::FileProbe;
use pastegrab_domain::plan::Machine;
use pastegrab_domain::profile::ProfileKind;
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
    };
    let queue = Queue::new(Settings {
        profile: ProfileKind::Video,
        machine: Machine { screen_height: 768 },
        dest: PathBuf::from("/videos"),
        data: PathBuf::from("/data"),
        permits: Permits::DEFAULT,
    });
    let view = Arc::new(RecordingView::default());
    let (handle, thread) = runtime::start(queue, Arc::new(ports), view.clone(), true);
    assert!(handle.send(Input::Paste(Paste {
        text: text.into(),
        profile: None,
        section: None,
    })));
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
