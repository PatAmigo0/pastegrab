//! A `Resolver` and a `Fetcher` over yt-dlp. The probe saves yt-dlp's info JSON and the fetch
//! loads it back, so each video costs one site extraction.

mod args;
mod failure;
mod progress;
mod summary;

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pastegrab_app::ports::{
    Exit, FetchRequest, Fetcher, Phase, ProcSpec, ProcessRunner, Progress, ProgressSink, Resolved,
    Resolver, Source,
};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::{FailureKind, Step};
use pastegrab_domain::link::{Link, LinkKind};

use progress::{Event, Tally};

const TAIL_LINES: usize = 20;

/// Where yt-dlp and the tools it calls live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    /// The bundled `python.exe`, or a `yt-dlp` executable.
    pub program: PathBuf,
    /// Arguments before yt-dlp's own, such as `-I` and the yt-dlp folder.
    pub program_args: Vec<OsString>,
    /// `--js-runtimes`, such as `quickjs:<path>`; `None` keeps yt-dlp's default.
    pub js_runtime: Option<OsString>,
    /// The folder with ffmpeg and ffprobe; `None` searches `PATH`.
    pub ffmpeg_dir: Option<PathBuf>,
    /// The CA bundle ffmpeg checks certificates against.
    pub ca_file: Option<PathBuf>,
    /// yt-dlp's cache.
    pub cache_dir: PathBuf,
    /// The temporary folder for yt-dlp and what it starts.
    pub temp_dir: Option<PathBuf>,
}

impl Engine {
    /// `program` with no extra arguments or tools, caching in `cache_dir`.
    #[must_use]
    pub const fn new(program: PathBuf, cache_dir: PathBuf) -> Self {
        Self {
            program,
            program_args: Vec::new(),
            js_runtime: None,
            ffmpeg_dir: None,
            ca_file: None,
            cache_dir,
            temp_dir: None,
        }
    }

    fn env(&self) -> Vec<(OsString, OsString)> {
        let Some(dir) = &self.temp_dir else {
            return Vec::new();
        };
        ["TEMP", "TMP", "TMPDIR"]
            .into_iter()
            .map(|name| (name.into(), dir.as_os_str().to_owned()))
            .collect()
    }
}

/// yt-dlp as a resolver and a fetcher; clones share one configuration.
#[derive(Clone)]
pub struct YtDlp(Arc<Inner>);

struct Inner {
    runner: Arc<dyn ProcessRunner>,
    engine: Engine,
    fixed: Vec<OsString>,
}

impl YtDlp {
    /// Runs `engine` through `runner`.
    #[must_use]
    pub fn new(runner: Arc<dyn ProcessRunner>, engine: Engine) -> Self {
        let fixed = args::fixed(&engine);
        Self(Arc::new(Inner {
            runner,
            engine,
            fixed,
        }))
    }

    fn run(
        &self,
        step_args: Vec<OsString>,
        cwd: &Path,
        cancel: &Cancel,
        mut on_progress: impl FnMut(Event),
    ) -> Result<Run, Failure> {
        let Inner { engine, fixed, .. } = &*self.0;
        let mut args =
            Vec::with_capacity(engine.program_args.len() + fixed.len() + step_args.len());
        args.extend(engine.program_args.iter().cloned());
        args.extend(fixed.iter().cloned());
        args.extend(step_args);
        let spec = ProcSpec {
            program: engine.program.clone(),
            args,
            cwd: Some(cwd.to_owned()),
            env: engine.env(),
            low_priority: false,
        };
        let mut lines = self.0.runner.spawn(&spec, cancel)?;
        let mut buf = Vec::with_capacity(256);
        let mut tail = VecDeque::with_capacity(TAIL_LINES);
        while lines.next_line(&mut buf).map_err(pipe_failure)? {
            match progress::parse(&buf) {
                Some(event) => on_progress(event),
                None if buf.is_empty() => {}
                None => {
                    if tail.len() == TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(String::from_utf8_lossy(&buf).into_owned());
                }
            }
        }
        let exit = lines.wait().map_err(pipe_failure)?;
        Ok(Run {
            exit,
            tail: tail.into(),
        })
    }
}

struct Run {
    exit: Exit,
    tail: Vec<String>,
}

impl Run {
    fn check(self, step: Step, cancel: &Cancel) -> Result<(), Failure> {
        if self.exit.success() {
            return Ok(());
        }
        if cancel.is_cancelled() {
            return Err(Failure::internal("cancelled"));
        }
        let kind = failure::classify(&self.tail, self.exit.code, step);
        let detail = if self.tail.is_empty() {
            format!("yt-dlp exited with {:?}", self.exit.code)
        } else {
            self.tail.join("\n")
        };
        Err(Failure::new(kind, detail))
    }
}

impl Resolver for YtDlp {
    fn accepts(&self, link: &Link) -> bool {
        // Playlists wait for item selection.
        !matches!(link.kind, LinkKind::Playlist | LinkKind::Channel)
    }

    fn resolve(
        &self,
        link: &Link,
        work_dir: &Path,
        impersonate: bool,
        cancel: &Cancel,
    ) -> Result<Resolved, Failure> {
        fs::create_dir_all(work_dir).map_err(|e| io_failure(&e, "creating the probe folder"))?;
        let summary = work_dir.join(args::SUMMARY);
        remove_stale(&summary)?;
        let probe = args::probe(link, work_dir, impersonate);
        self.run(probe, work_dir, cancel, |_| {})?
            .check(Step::Probe, cancel)?;
        let text = fs::read(&summary).map_err(|e| io_failure(&e, "reading the summary"))?;
        let info =
            summary::parse(&text).map_err(|e| Failure::internal(format!("the summary: {e}")))?;
        Ok(Resolved {
            info,
            source: Source::Engine {
                info_json: work_dir.join(args::INFO_JSON),
                impersonate,
            },
        })
    }
}

impl Fetcher for YtDlp {
    fn accepts(&self, source: &Source) -> bool {
        matches!(source, Source::Engine { .. })
    }

    fn fetch(
        &self,
        request: FetchRequest<'_>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<PathBuf, Failure> {
        let Source::Engine {
            info_json,
            impersonate,
        } = request.source
        else {
            return Err(Failure::internal("not a yt-dlp source"));
        };
        let work = request.work_dir;
        fs::create_dir_all(work).map_err(|e| io_failure(&e, "creating the work folder"))?;
        let result = work.join(args::RESULT);
        remove_stale(&result)?;
        let expected = request.plan.expected_bytes;
        let mut tally = Tally::default();
        let fetch = args::fetch(request.plan, info_json, work, *impersonate);
        self.run(fetch, work, cancel, |event| match event {
            Event::Download(sample) => {
                progress.report(Phase::Download, tally.add(sample, expected));
            }
            Event::Merge => progress.report(Phase::Merge, Progress::default()),
            Event::Other => {}
        })?
        .check(Step::Fetch, cancel)?;
        let named =
            fs::read_to_string(&result).map_err(|e| io_failure(&e, "reading the result"))?;
        let file = named
            .lines()
            .rfind(|line| !line.is_empty())
            .map(PathBuf::from)
            .filter(|file| file.is_file())
            .ok_or_else(|| Failure::internal(format!("yt-dlp named no file: {named:?}")))?;
        // With browser login the info JSON carries cookies in every format's headers.
        let _ = fs::remove_file(info_json);
        let _ = fs::remove_file(&result);
        Ok(file)
    }
}

fn remove_stale(path: &Path) -> Result<(), Failure> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_failure(&e, "clearing old output")),
        _ => Ok(()),
    }
}

fn io_failure(e: &io::Error, doing: &str) -> Failure {
    let kind = if e.kind() == io::ErrorKind::StorageFull {
        FailureKind::DiskFull
    } else {
        FailureKind::Internal
    };
    Failure::new(kind, format!("{doing}: {e}"))
}

#[expect(clippy::needless_pass_by_value, reason = "used with map_err")]
fn pipe_failure(e: io::Error) -> Failure {
    Failure::internal(format!("reading yt-dlp's output: {e}"))
}
