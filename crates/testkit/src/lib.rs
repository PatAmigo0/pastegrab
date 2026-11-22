//! Fakes for every port, which record what they were asked and answer from a script, and
//! `contract`, the rules every implementation of a port keeps.

pub mod contract;
mod scripted;
mod temp;

pub use scripted::{ScriptedRunner, Transcript};
pub use temp::TempDir;

use std::collections::{BTreeSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pastegrab_app::ports::{
    FetchRequest, Fetcher, Files, MediaProbe, Phase, Progress, ProgressSink, Resolved, Resolver,
    Source, Transcoder, View, ViewUpdate,
};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::JobId;
use pastegrab_domain::link::Link;
use pastegrab_domain::media::{
    AudioCodec, AudioStream, Family, FileProbe, Format, MediaInfo, ProbedVideo, Protocol,
    VideoCodec, VideoStream,
};
use pastegrab_domain::plan::Convert;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Pops answers in order and repeats the last one.
fn next<T: Clone>(answers: &Mutex<VecDeque<T>>) -> Option<T> {
    let mut answers = lock(answers);
    if answers.len() > 1 {
        answers.pop_front()
    } else {
        answers.front().cloned()
    }
}

/// An in-memory file tree, shared by the fakes that create, move and remove files.
#[derive(Clone, Default)]
pub struct MemFiles(Arc<Mutex<BTreeSet<PathBuf>>>);

impl MemFiles {
    /// Records a file.
    pub fn create(&self, path: &Path) {
        lock(&self.0).insert(path.to_owned());
    }

    /// Every file, sorted.
    #[must_use]
    pub fn all(&self) -> Vec<PathBuf> {
        lock(&self.0).iter().cloned().collect()
    }
}

impl Files for MemFiles {
    fn exists(&self, path: &Path) -> bool {
        lock(&self.0).contains(path)
    }

    fn place(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = lock(&self.0);
        if files.contains(to) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if !files.remove(from) {
            return Err(io::ErrorKind::NotFound.into());
        }
        files.insert(to.to_owned());
        Ok(())
    }

    fn remove_tree(&self, path: &Path) -> io::Result<()> {
        lock(&self.0).retain(|p| !p.starts_with(path));
        Ok(())
    }
}

/// Answers every link from a script of results.
pub struct FakeResolver {
    answers: Mutex<VecDeque<Result<Resolved, Failure>>>,
    /// Each call's URL and whether it asked for impersonation.
    pub calls: Mutex<Vec<(String, bool)>>,
}

impl FakeResolver {
    /// Answers in order, repeating the last answer.
    #[must_use]
    pub fn new(answers: Vec<Result<Resolved, Failure>>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            calls: Mutex::default(),
        }
    }
}

impl Resolver for FakeResolver {
    fn accepts(&self, _: &Link) -> bool {
        true
    }

    fn resolve(
        &self,
        link: &Link,
        _: &Path,
        impersonate: bool,
        _: &Cancel,
    ) -> Result<Resolved, Failure> {
        lock(&self.calls).push((link.url.to_string(), impersonate));
        next(&self.answers).unwrap_or_else(|| Err(Failure::internal("no scripted answer")))
    }
}

/// Creates the download in a `MemFiles` and reports one progress line.
pub struct FakeFetcher {
    files: MemFiles,
    name: String,
    answers: Mutex<VecDeque<Result<(), Failure>>>,
}

impl FakeFetcher {
    /// Downloads to `<work dir>/<name>`, failing first with the scripted failures.
    #[must_use]
    pub fn new(files: MemFiles, name: &str, answers: Vec<Result<(), Failure>>) -> Self {
        let mut answers: VecDeque<_> = answers.into();
        answers.push_back(Ok(()));
        Self {
            files,
            name: name.to_owned(),
            answers: Mutex::new(answers),
        }
    }
}

impl Fetcher for FakeFetcher {
    fn accepts(&self, _: &Source) -> bool {
        true
    }

    fn fetch(
        &self,
        request: FetchRequest<'_>,
        progress: &dyn ProgressSink,
        _: &Cancel,
    ) -> Result<PathBuf, Failure> {
        let answer = lock(&self.answers).pop_front().unwrap_or(Ok(()));
        answer?;
        progress.report(
            Phase::Download,
            Progress {
                done: 1,
                total: Some(1),
                ..Progress::default()
            },
        );
        let file = request.work_dir.join(&self.name);
        self.files.create(&file);
        Ok(file)
    }
}

/// Reports the same probe for every file.
pub struct FakeProbe(pub FileProbe);

impl MediaProbe for FakeProbe {
    fn probe(&self, _: &Path, _: &Cancel) -> Result<FileProbe, Failure> {
        Ok(self.0)
    }
}

/// Creates the output in a `MemFiles` and records the conversion it was asked for.
pub struct FakeTranscoder {
    files: MemFiles,
    /// Every conversion, in order.
    pub calls: Mutex<Vec<Convert>>,
}

impl FakeTranscoder {
    /// A transcoder that writes into `files`.
    #[must_use]
    pub fn new(files: MemFiles) -> Self {
        Self {
            files,
            calls: Mutex::default(),
        }
    }
}

impl Transcoder for FakeTranscoder {
    fn transcode(
        &self,
        _: &Path,
        output: &Path,
        convert: &Convert,
        _: Option<u32>,
        _: &dyn ProgressSink,
        _: &Cancel,
    ) -> Result<(), Failure> {
        lock(&self.calls).push(*convert);
        self.files.create(output);
        Ok(())
    }
}

/// Keeps every update and counts progress reports.
#[derive(Default)]
pub struct RecordingView {
    /// Updates in order.
    pub updates: Mutex<Vec<ViewUpdate>>,
    /// Progress reports per job.
    pub progress: Mutex<Vec<(JobId, Phase)>>,
}

impl RecordingView {
    /// A copy of the updates so far.
    #[must_use]
    pub fn updates(&self) -> Vec<ViewUpdate> {
        lock(&self.updates).clone()
    }
}

impl View for RecordingView {
    fn apply(&self, update: ViewUpdate) {
        lock(&self.updates).push(update);
    }

    fn progress(&self, id: JobId, phase: Phase, _: Progress) {
        lock(&self.progress).push((id, phase));
    }
}

/// A source with a 720p H.264 video and an AAC track, the shape the default profile picks.
#[must_use]
pub fn media(title: &str) -> MediaInfo {
    let format = |id: &str, video: Option<VideoStream>, audio: Option<AudioStream>| Format {
        id: id.into(),
        video,
        audio,
        size: Some(10_000_000),
        bitrate: None,
        protocol: Protocol::Dash,
        drm: false,
    };
    MediaInfo {
        title: title.into(),
        duration: Some(60),
        is_live: false,
        formats: vec![
            format(
                "136",
                Some(VideoStream {
                    codec: VideoCodec::H264,
                    height: Some(720),
                    fps: Some(30),
                }),
                None,
            ),
            format(
                "140",
                None,
                Some(AudioStream {
                    codec: AudioCodec::Aac,
                    bitrate: Some(128),
                }),
            ),
        ],
    }
}

/// What ffprobe reports for an MP4 with H.264 and AAC, which the default profile keeps as is.
#[must_use]
pub const fn mp4_probe() -> FileProbe {
    FileProbe {
        family: Family::IsoBmff,
        video: Some(ProbedVideo {
            stream: VideoStream {
                codec: VideoCodec::H264,
                height: Some(720),
                fps: Some(30),
            },
            interlaced: false,
        }),
        audio: Some(AudioCodec::Aac),
    }
}

/// What ffprobe reports for a `WebM` file with VP9 and Opus, which the default profile converts.
#[must_use]
pub const fn webm_probe() -> FileProbe {
    FileProbe {
        family: Family::Matroska,
        video: Some(ProbedVideo {
            stream: VideoStream {
                codec: VideoCodec::Vp9,
                height: Some(720),
                fps: Some(30),
            },
            interlaced: false,
        }),
        audio: Some(AudioCodec::Opus),
    }
}
