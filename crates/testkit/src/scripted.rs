use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use pastegrab_app::ports::{Exit, Lines, ProcSpec, ProcessRunner};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;

use crate::lock;

/// One recorded run of a tool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    /// Its output, stdout and stderr merged, one entry per line.
    pub lines: Vec<String>,
    /// Its exit code.
    pub code: i32,
    /// Files it writes before its first line, relative to its working folder or absolute.
    pub files: Vec<(PathBuf, Vec<u8>)>,
    /// After its lines it runs until cancelled, like a stalled download.
    pub hangs: bool,
}

impl Transcript {
    /// A run that prints `lines` and exits with `code`.
    #[must_use]
    pub fn new(lines: &[&str], code: i32) -> Self {
        Self {
            lines: lines.iter().map(|&line| line.to_owned()).collect(),
            code,
            ..Self::default()
        }
    }

    /// Adds a file the run writes.
    #[must_use]
    pub fn writes(mut self, path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.push((path.into(), bytes.into()));
        self
    }

    /// Makes the run wait for a cancel after its lines.
    #[must_use]
    pub const fn hanging(mut self) -> Self {
        self.hangs = true;
        self
    }
}

/// Replays transcripts in order, per program, and records every spawn.
#[derive(Default)]
pub struct ScriptedRunner {
    scripts: Mutex<HashMap<PathBuf, VecDeque<Transcript>>>,
    /// Every spawn, in order.
    pub calls: Mutex<Vec<ProcSpec>>,
}

impl ScriptedRunner {
    /// Queues `transcript` as the next run of `program`.
    #[must_use]
    pub fn script(self, program: impl Into<PathBuf>, transcript: Transcript) -> Self {
        lock(&self.scripts)
            .entry(program.into())
            .or_default()
            .push_back(transcript);
        self
    }

    /// A copy of every spawn so far.
    #[must_use]
    pub fn calls(&self) -> Vec<ProcSpec> {
        lock(&self.calls).clone()
    }
}

impl ProcessRunner for ScriptedRunner {
    fn spawn(&self, spec: &ProcSpec, cancel: &Cancel) -> Result<Box<dyn Lines>, Failure> {
        lock(&self.calls).push(spec.clone());
        let transcript = lock(&self.scripts)
            .get_mut(&spec.program)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| {
                let program = spec.program.display();
                Failure::new(
                    FailureKind::ToolBroken,
                    format!("no transcript for {program}"),
                )
            })?;
        for (path, bytes) in &transcript.files {
            let path = spec
                .cwd
                .as_ref()
                .map_or_else(|| path.clone(), |cwd| cwd.join(path));
            fs::write(&path, bytes)
                .map_err(|e| Failure::internal(format!("scripted file: {e}")))?;
        }
        let (killed, cancelled) = channel::<()>();
        // Dropping the sender wakes a hanging replay.
        let _ = cancel.on_cancel(move || drop(killed));
        Ok(Box::new(Replay {
            lines: transcript.lines.into(),
            code: transcript.code,
            hangs: transcript.hangs,
            cancelled,
            killed: false,
        }))
    }
}

struct Replay {
    lines: VecDeque<String>,
    code: i32,
    hangs: bool,
    cancelled: Receiver<()>,
    killed: bool,
}

impl Lines for Replay {
    fn next_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
        buf.clear();
        if let Some(line) = self.lines.pop_front() {
            buf.extend_from_slice(line.as_bytes());
            return Ok(true);
        }
        if self.hangs && !self.killed {
            let _ = self.cancelled.recv();
            self.killed = true;
        }
        Ok(false)
    }

    fn wait(self: Box<Self>) -> io::Result<Exit> {
        let code = if self.killed { 1 } else { self.code };
        Ok(Exit { code: Some(code) })
    }
}
