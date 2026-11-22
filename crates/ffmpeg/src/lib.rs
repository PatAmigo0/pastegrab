//! `MediaProbe` over ffprobe and `Transcoder` over ffmpeg, started through the `ProcessRunner`
//! port.

mod args;
mod probe;

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pastegrab_app::ports::{
    Exit, Lines, MediaProbe, Phase, ProcSpec, ProcessRunner, Progress, ProgressSink, Transcoder,
};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;
use pastegrab_domain::media::FileProbe;
use pastegrab_domain::plan::Convert;

const TAIL_LINES: usize = 20;

/// Where ffmpeg and ffprobe live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// The ffmpeg executable.
    pub ffmpeg: PathBuf,
    /// The ffprobe executable.
    pub ffprobe: PathBuf,
}

/// ffprobe and ffmpeg behind the ports; clones share one configuration.
#[derive(Clone)]
pub struct Ffmpeg(Arc<Inner>);

struct Inner {
    runner: Arc<dyn ProcessRunner>,
    tools: Tools,
}

impl Ffmpeg {
    /// Runs `tools` through `runner`.
    #[must_use]
    pub fn new(runner: Arc<dyn ProcessRunner>, tools: Tools) -> Self {
        Self(Arc::new(Inner { runner, tools }))
    }

    fn spawn(
        &self,
        program: &Path,
        args: Vec<std::ffi::OsString>,
        low_priority: bool,
        cancel: &Cancel,
    ) -> Result<Box<dyn Lines>, Failure> {
        let spec = ProcSpec {
            program: program.to_owned(),
            args,
            low_priority,
            ..ProcSpec::default()
        };
        self.0.runner.spawn(&spec, cancel)
    }
}

impl MediaProbe for Ffmpeg {
    fn probe(&self, file: &Path, cancel: &Cancel) -> Result<FileProbe, Failure> {
        let ffprobe = &self.0.tools.ffprobe;
        let mut lines = self.spawn(ffprobe, args::probe(file), false, cancel)?;
        let mut buf = Vec::new();
        let mut json = Vec::new();
        let mut tail = Tail::default();
        while lines.next_line(&mut buf).map_err(pipe_failure)? {
            // Errors can come before the JSON, which starts on a line of its own.
            if json.is_empty() && buf != b"{" {
                tail.push(&buf);
                continue;
            }
            json.extend_from_slice(&buf);
            json.push(b'\n');
        }
        let exit = lines.wait().map_err(pipe_failure)?;
        if !exit.success() {
            return Err(tail.failure(exit, cancel));
        }
        let found =
            probe::parse(&json).map_err(|e| Failure::internal(format!("ffprobe's output: {e}")))?;
        if found.video.is_none() && found.audio.is_none() {
            let name = file.display();
            return Err(Failure::internal(format!("no sound or picture in {name}")));
        }
        Ok(found)
    }
}

impl Transcoder for Ffmpeg {
    fn transcode(
        &self,
        input: &Path,
        output: &Path,
        convert: &Convert,
        duration: Option<u32>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<(), Failure> {
        let total = convert
            .trim
            .map(|trim| trim.length)
            .or_else(|| duration.map(|secs| Duration::from_secs(secs.into())))
            .map(|length| u64::try_from(length.as_micros()).unwrap_or(u64::MAX));
        let ffmpeg = &self.0.tools.ffmpeg;
        let mut lines = self.spawn(ffmpeg, args::convert(input, output, convert), true, cancel)?;
        let mut buf = Vec::with_capacity(128);
        let mut tail = Tail::default();
        while lines.next_line(&mut buf).map_err(pipe_failure)? {
            if let Some(done) = buf.strip_prefix(b"out_time_us=").and_then(number) {
                let report = Progress {
                    done,
                    total,
                    ..Progress::default()
                };
                progress.report(Phase::Convert, report);
            } else if !is_progress_key(&buf) {
                tail.push(&buf);
            }
        }
        let exit = lines.wait().map_err(pipe_failure)?;
        if exit.success() {
            Ok(())
        } else {
            Err(tail.failure(exit, cancel))
        }
    }
}

/// `-progress` writes `key=value` lines such as `frame=120` and `speed=1.5x`.
fn is_progress_key(line: &[u8]) -> bool {
    line.iter().position(|&b| b == b'=').is_some_and(|at| {
        at > 0
            && line[..at]
                .iter()
                .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

fn number(text: &[u8]) -> Option<u64> {
    std::str::from_utf8(text).ok()?.parse().ok()
}

#[derive(Default)]
struct Tail(VecDeque<String>);

impl Tail {
    fn push(&mut self, line: &[u8]) {
        if line.is_empty() {
            return;
        }
        if self.0.len() == TAIL_LINES {
            self.0.pop_front();
        }
        self.0.push_back(String::from_utf8_lossy(line).into_owned());
    }

    fn failure(self, exit: Exit, cancel: &Cancel) -> Failure {
        if cancel.is_cancelled() {
            return Failure::internal("cancelled");
        }
        let lines: Vec<String> = self.0.into();
        let text = lines.join("\n");
        let missing_dll = exit
            .code
            .is_some_and(|code| matches!(code.cast_unsigned(), 0xC000_0135 | 0xC000_0139));
        let kind = if missing_dll {
            FailureKind::ToolBroken
        } else if text.contains("No space left on device")
            || text.contains("There is not enough space on the disk")
        {
            FailureKind::DiskFull
        } else {
            FailureKind::Internal
        };
        let detail = if text.is_empty() {
            format!("exited with {:?}", exit.code)
        } else {
            text
        };
        Failure::new(kind, detail)
    }
}

#[expect(clippy::needless_pass_by_value, reason = "used with map_err")]
fn pipe_failure(e: io::Error) -> Failure {
    Failure::internal(format!("reading ffmpeg's output: {e}"))
}
