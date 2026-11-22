//! The adapter against yt-dlp runs recorded on Linux.

#![allow(clippy::expect_used, reason = "fixture helpers outside #[test] fns")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use pastegrab_app::ports::{
    FetchRequest, Fetcher, Phase, Progress, ProgressSink, Resolved, Resolver, Source,
};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;
use pastegrab_domain::link::{Link, analyze};
use pastegrab_domain::media::{AudioCodec, FormatId, MediaInfo, VideoCodec};
use pastegrab_domain::plan::{FetchPlan, Machine, plan_fetch};
use pastegrab_domain::profile::{Container, ProfileKind};
use pastegrab_testkit::{ScriptedRunner, TempDir, Transcript};
use pastegrab_ytdlp::{Engine, YtDlp};

const YTDLP: &str = "/opt/yt-dlp";
const ZOO: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
const ZOO_SUMMARY: &[u8] = include_bytes!("fixtures/youtube-zoo.summary.json");
const ZOO_FETCH: &str = include_str!("fixtures/youtube-zoo.fetch.txt");

fn ytdlp(runner: &Arc<ScriptedRunner>, dir: &Path) -> YtDlp {
    let engine = Engine::new(PathBuf::from(YTDLP), dir.join("cache"));
    YtDlp::new(Arc::clone(runner) as _, engine)
}

fn link(text: &str) -> Link {
    analyze(text).pop().expect("a link")
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(Phase, Progress)>>);

impl ProgressSink for Recorder {
    fn report(&self, phase: Phase, progress: Progress) {
        self.0.lock().expect("reports").push((phase, progress));
    }
}

#[test]
fn a_probe_reads_the_summary_and_the_planner_picks_like_youtube_offers() {
    let dir = TempDir::new("ytdlp-probe").expect("a temp folder");
    let runner = Arc::new(ScriptedRunner::default().script(
        YTDLP,
        Transcript::new(&[], 0).writes("summary.json", ZOO_SUMMARY),
    ));
    let job = dir.path().join("jobs").join("1");
    let resolved = ytdlp(&runner, dir.path())
        .resolve(&link(ZOO), &job, false, &Cancel::default())
        .expect("a probe");
    assert_eq!(resolved.info.title, "Me at the zoo");
    assert_eq!(resolved.info.duration, Some(19));
    // 25 formats, of which five are `-drc` twins.
    assert_eq!(resolved.info.formats.len(), 20);
    assert_eq!(
        resolved.source,
        Source::Engine {
            info_json: job.join("probe.info.json"),
            impersonate: false,
        }
    );
    let profile = ProfileKind::Video.profile();
    let plan = plan_fetch(
        &resolved.info,
        &profile,
        Machine { screen_height: 768 },
        None,
    )
    .expect("a plan");
    assert_eq!(plan.selector(), "134+140");

    let call = &runner.calls()[0];
    assert_eq!(call.cwd.as_deref(), Some(job.as_path()));
    let tail: [std::ffi::OsString; 2] = ["--".into(), ZOO.into()];
    assert_eq!(call.args[call.args.len() - 2..], tail);
}

fn probe_recorded(summary: &[u8], url: &str) -> Result<Resolved, Failure> {
    let dir = TempDir::new("ytdlp-recorded").expect("a temp folder");
    let transcript = Transcript::new(&[], 0).writes("summary.json", summary);
    let runner = Arc::new(ScriptedRunner::default().script(YTDLP, transcript));
    let job = dir.path().join("job");
    ytdlp(&runner, dir.path()).resolve(&link(url), &job, false, &Cancel::default())
}

fn video_plan(info: &MediaInfo) -> String {
    let profile = ProfileKind::Video.profile();
    plan_fetch(info, &profile, Machine { screen_height: 768 }, None)
        .expect("a plan")
        .selector()
}

#[test]
fn archive_org_names_no_codecs_and_the_tallest_file_wins() {
    let info = probe_recorded(
        include_bytes!("fixtures/archive-cops.summary.json"),
        "https://archive.org/details/Cops1922",
    )
    .expect("a probe")
    .info;
    assert_eq!(info.title, "Buster Keaton's \"Cops\" (1922)");
    let video = info.formats[1].video.expect("a picture");
    assert_eq!(
        (video.codec, video.height),
        (VideoCodec::Unknown, Some(480))
    );
    assert_eq!(
        info.formats[1].audio.map(|a| a.codec),
        Some(AudioCodec::Unknown)
    );
    assert_eq!(video_plan(&info), "1");
}

#[test]
fn ok_ru_lists_no_heights_and_the_bigger_file_wins() {
    let info = probe_recorded(
        include_bytes!("fixtures/okru.summary.json"),
        "https://ok.ru/video/1484130554189",
    )
    .expect("a probe")
    .info;
    assert_eq!(info.title, "Народная забава");
    assert_eq!(video_plan(&info), "html5-video-high+html5-audio-high");
}

#[test]
fn several_videos_on_one_page_wait_for_item_selection() {
    let failure = probe_recorded(
        include_bytes!("fixtures/archive-several.summary.json"),
        "https://archive.org/details/Election_Ads",
    )
    .expect_err("several videos");
    assert_eq!(failure.kind, FailureKind::Unsupported);
    assert_eq!(failure.detail, "10 videos on one page");
}

#[test]
fn a_fetch_counts_both_parts_as_one_download_and_names_the_merged_file() {
    let dir = TempDir::new("ytdlp-fetch").expect("a temp folder");
    let job = dir.path().join("jobs").join("1");
    fs::create_dir_all(&job).expect("the job folder");
    let info_json = job.join("probe.info.json");
    fs::write(&info_json, "{}").expect("an info JSON");
    let work = dir.path().join("videos").join(".pastegrab").join("1");
    let media = work.join("jNQXAC9IVRw.mp4");
    let transcript = Transcript {
        lines: ZOO_FETCH.lines().map(str::to_owned).collect(),
        ..Transcript::default()
    }
    .writes(&media, "media")
    .writes("result.txt", format!("{}\n", media.display()));
    let runner = Arc::new(ScriptedRunner::default().script(YTDLP, transcript));
    let plan = FetchPlan {
        format_ids: ["134", "140"]
            .into_iter()
            .filter_map(FormatId::parse)
            .collect(),
        merge_into: Some(Container::Mp4),
        section: None,
        expected_bytes: Some(631_667),
        encodes_video: false,
    };
    let source = Source::Engine {
        info_json: info_json.clone(),
        impersonate: false,
    };
    let request = FetchRequest {
        plan: &plan,
        source: &source,
        work_dir: &work,
    };
    let recorder = Recorder::default();
    let file = ytdlp(&runner, dir.path())
        .fetch(request, &recorder, &Cancel::default())
        .expect("a fetch");
    assert_eq!(file, media);
    assert!(!info_json.exists(), "the info JSON can hold cookies");

    let reports = recorder.0.lock().expect("reports");
    let downloads: Vec<u64> = reports
        .iter()
        .filter(|(phase, _)| *phase == Phase::Download)
        .map(|(_, p)| p.done)
        .collect();
    assert!(downloads.is_sorted(), "{downloads:?}");
    assert_eq!(downloads.last(), Some(&631_667));
    assert!(reports.iter().any(|(phase, _)| *phase == Phase::Merge));
    let call = &runner.calls()[0];
    assert!(call.args.contains(&"134+140".into()));
}

#[test]
fn a_refusal_carries_its_kind_and_yt_dlps_words() {
    let dir = TempDir::new("ytdlp-refusal").expect("a temp folder");
    let line = "ERROR: [youtube] BaW_jenozKc: This video is unavailable";
    let runner = Arc::new(ScriptedRunner::default().script(YTDLP, Transcript::new(&[line], 1)));
    let failure = ytdlp(&runner, dir.path())
        .resolve(
            &link("https://youtu.be/BaW_jenozKc"),
            &dir.path().join("job"),
            false,
            &Cancel::default(),
        )
        .expect_err("a refusal");
    assert_eq!(failure.kind, FailureKind::Unavailable);
    assert_eq!(failure.detail, line);
}

#[test]
fn a_cancel_ends_a_stalled_probe() {
    let dir = TempDir::new("ytdlp-cancel").expect("a temp folder");
    let runner =
        Arc::new(ScriptedRunner::default().script(YTDLP, Transcript::new(&[], 0).hanging()));
    let cancel = Arc::new(Cancel::default());
    let canceller = {
        let cancel = Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel.cancel();
        })
    };
    let ended =
        ytdlp(&runner, dir.path()).resolve(&link(ZOO), &dir.path().join("job"), false, &cancel);
    assert!(ended.is_err());
    canceller.join().expect("the cancel thread ends");
}

#[test]
fn playlists_and_impersonation() {
    let dir = TempDir::new("ytdlp-accepts").expect("a temp folder");
    let runner = Arc::new(ScriptedRunner::default().script(
        YTDLP,
        Transcript::new(&[], 0).writes("summary.json", ZOO_SUMMARY),
    ));
    let ytdlp = ytdlp(&runner, dir.path());
    assert!(!Resolver::accepts(
        &ytdlp,
        &link("https://www.youtube.com/playlist?list=PLq9zAbCdEf")
    ));
    assert!(Resolver::accepts(&ytdlp, &link(ZOO)));

    let resolved = ytdlp
        .resolve(
            &link(ZOO),
            &dir.path().join("job"),
            true,
            &Cancel::default(),
        )
        .expect("a probe");
    assert!(matches!(
        resolved.source,
        Source::Engine {
            impersonate: true,
            ..
        }
    ));
    let args = &runner.calls()[0].args;
    assert!(args.contains(&"--impersonate".into()));
    assert!(
        !args.contains(&"generic:impersonate".into()),
        "YouTube has its own extractor"
    );
}
