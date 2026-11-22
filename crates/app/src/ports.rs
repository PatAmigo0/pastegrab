//! The traits adapters implement. Each has at most three methods, and each has a fake in
//! `testkit` that runs the same contract suite as the real adapter.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use pastegrab_domain::job::{JobId, JobState};
use pastegrab_domain::link::Link;
use pastegrab_domain::media::{FileProbe, MediaInfo};
use pastegrab_domain::plan::{Convert, FetchPlan, PlanError};
use pastegrab_domain::profile::{HeightCap, ProfileKind};

use crate::cancel::Cancel;
use crate::failure::Failure;

/// A child process to start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcSpec {
    /// The executable, by absolute path.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Its working directory.
    pub cwd: Option<PathBuf>,
    /// Variables to set on top of the inherited environment.
    pub env: Vec<(OsString, OsString)>,
    /// Runs below normal priority; conversions use it.
    pub low_priority: bool,
}

/// How a child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    /// The exit code, `None` when a signal ended it.
    pub code: Option<i32>,
}

impl Exit {
    /// Exit code 0.
    #[must_use]
    pub const fn success(self) -> bool {
        matches!(self.code, Some(0))
    }
}

/// The merged stdout and stderr of a running child, line by line.
pub trait Lines: Send {
    /// Reads one line into `buf`, reusing its allocation, without the line ending.
    /// `Ok(false)` at the end of output.
    ///
    /// # Errors
    ///
    /// Pipe errors from the operating system.
    fn next_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool>;

    /// Waits for the child to end.
    ///
    /// # Errors
    ///
    /// Errors from the operating system while waiting.
    fn wait(self: Box<Self>) -> io::Result<Exit>;
}

/// Starts child processes that die with their step.
pub trait ProcessRunner: Send + Sync {
    /// Starts `spec` so that `cancel`, or the end of pastegrab, ends it and every process it
    /// starts.
    ///
    /// # Errors
    ///
    /// `ToolBroken` when the program can't start.
    fn spawn(&self, spec: &ProcSpec, cancel: &Cancel) -> Result<Box<dyn Lines>, Failure>;
}

/// The phases a progress report can belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Bytes coming in.
    Download,
    /// yt-dlp merging or fixing up the download.
    Merge,
    /// ffmpeg converting; `done` and `total` count microseconds of media.
    Convert,
}

/// The latest numbers from a running step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Bytes, or microseconds of media while converting.
    pub done: u64,
    /// The total, when known.
    pub total: Option<u64>,
    /// Bytes per second.
    pub speed: Option<u64>,
    /// Seconds left, as the tool estimates it.
    pub eta: Option<u64>,
}

/// Where a step sends its progress. Implementations must be cheap; they run per line.
pub trait ProgressSink: Send + Sync {
    /// The latest numbers for `phase`.
    fn report(&self, phase: Phase, progress: Progress);
}

/// Where a probed link's bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// yt-dlp's info JSON from the probe, which the fetch loads instead of extracting again.
    Engine {
        /// The `.info.json` file.
        info_json: PathBuf,
        /// Whether the site needed browser impersonation.
        impersonate: bool,
    },
    /// A media file served as is.
    Direct {
        /// Its URL.
        url: String,
    },
}

/// What a resolver learned about a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Title, duration and formats.
    pub info: MediaInfo,
    /// Where the fetch gets the bytes.
    pub source: Source,
}

/// Turns a link into a format list.
pub trait Resolver: Send + Sync {
    /// Whether this resolver handles `link`; offline, no I/O.
    fn accepts(&self, link: &Link) -> bool;

    /// Asks the site what `link` offers, keeping working files in `work_dir`.
    ///
    /// # Errors
    ///
    /// A failure kind from the engine's output, `Unsupported` for a page without media.
    fn resolve(
        &self,
        link: &Link,
        work_dir: &Path,
        impersonate: bool,
        cancel: &Cancel,
    ) -> Result<Resolved, Failure>;
}

/// What a fetch needs.
#[derive(Debug, Clone, Copy)]
pub struct FetchRequest<'a> {
    /// The formats to download.
    pub plan: &'a FetchPlan,
    /// Where they come from.
    pub source: &'a Source,
    /// Where the download lands, on the destination's volume.
    pub work_dir: &'a Path,
}

/// Downloads planned formats.
pub trait Fetcher: Send + Sync {
    /// Whether this fetcher handles `source`.
    fn accepts(&self, source: &Source) -> bool;

    /// Downloads into the request's work folder and returns the finished file.
    ///
    /// # Errors
    ///
    /// A failure kind from the engine, `Network` or `DiskFull`.
    fn fetch(
        &self,
        request: FetchRequest<'_>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<PathBuf, Failure>;
}

/// Reads what a file holds.
pub trait MediaProbe: Send + Sync {
    /// The container and streams of `file`.
    ///
    /// # Errors
    ///
    /// `ToolBroken` when the prober fails, `Internal` for output it can't read.
    fn probe(&self, file: &Path, cancel: &Cancel) -> Result<FileProbe, Failure>;
}

/// Converts files.
pub trait Transcoder: Send + Sync {
    /// Writes `output` from `input` as `convert` says. `duration` in seconds lets progress
    /// report a total.
    ///
    /// # Errors
    ///
    /// `ToolBroken` or `DiskFull`.
    fn transcode(
        &self,
        input: &Path,
        output: &Path,
        convert: &Convert,
        duration: Option<u32>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<(), Failure>;
}

/// The file operations the convert step needs.
pub trait Files: Send + Sync {
    /// Whether anything exists at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// Moves `from` to `to` on the same volume, never replacing an existing `to`, and marks the
    /// result as downloaded where the system has such a mark.
    ///
    /// # Errors
    ///
    /// Errors from the operating system, `AlreadyExists` among them.
    fn place(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Removes a working folder and everything in it; a missing folder is fine.
    ///
    /// # Errors
    ///
    /// Errors from the operating system.
    fn remove_tree(&self, path: &Path) -> io::Result<()>;
}

/// What the person chose and keeps across starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    /// Where finished files go; `None` keeps the Videos folder.
    pub dest: Option<PathBuf>,
    /// The profile video links start with; `None` keeps «Видео».
    pub profile: Option<ProfileKind>,
    /// The highest tutorial version the person finished or skipped.
    pub tutorial: Option<u32>,
    /// Whether focusing the window picks up a link from the clipboard; `None` keeps it on.
    pub clipboard: Option<bool>,
    /// Whether closing the window parks it in the system tray; `None` closes the app.
    pub close_to_tray: Option<bool>,
    /// Override for the «Видео» height cap; `None` fits the screen.
    pub max_height: Option<HeightCap>,
    /// Override for parallel downloads (`1..=4`); `None` follows `deploy.json`.
    pub downloads: Option<u8>,
}

/// Keeps the person's choices.
pub trait SettingsStore: Send + Sync {
    /// The saved choices; `None` before the first save or when the file can't be read.
    fn load(&self) -> Option<Choices>;

    /// Replaces the saved choices, all at once or not at all.
    ///
    /// # Errors
    ///
    /// Errors from the operating system.
    fn save(&self, choices: &Choices) -> io::Result<()>;
}

/// What the person should see change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewUpdate {
    /// A link from the paste became a job.
    Added {
        /// The job.
        id: JobId,
        /// The cleaned link.
        url: String,
        /// The profile it starts with.
        profile: ProfileKind,
    },
    /// The job moved on.
    State {
        /// The job.
        id: JobId,
        /// Its new state.
        state: JobState,
    },
    /// The probe named the video and the planner picked its formats.
    Planned {
        /// The job.
        id: JobId,
        /// The video's title.
        title: String,
        /// The profile the plan follows.
        profile: ProfileKind,
        /// Seconds, when the site says.
        duration: Option<u32>,
        /// The formats and the expected size.
        plan: FetchPlan,
    },
    /// The probe worked, and the profile asks for something the source lacks.
    NoPlan {
        /// The job.
        id: JobId,
        /// The video's title.
        title: String,
        /// The profile that finds nothing.
        profile: ProfileKind,
        /// Seconds, when the site says.
        duration: Option<u32>,
        /// What is missing.
        reason: PlanError,
    },
    /// The file is in place.
    Finished {
        /// The job.
        id: JobId,
        /// Where it landed.
        file: PathBuf,
    },
    /// The job stopped.
    Failed {
        /// The job.
        id: JobId,
        /// Why.
        failure: Failure,
    },
    /// The paste held no link.
    NoLinks,
}

/// The screen, whichever it is.
pub trait View: Send + Sync {
    /// Shows a change.
    fn apply(&self, update: ViewUpdate);

    /// Shows a job's latest progress. Called often; implementations coalesce.
    fn progress(&self, id: JobId, phase: Phase, progress: Progress);
}
