use std::io::{self, BufRead, BufReader, PipeReader, Read};
use std::process::{Command, Stdio};

use pastegrab_app::ports::{Exit, Lines, ProcSpec, ProcessRunner};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;

use crate::sys::{self, Contained};

/// A longer line arrives in pieces, so a runaway line can't eat memory.
const MAX_LINE: u64 = 64 * 1024;

/// Starts children that a cancel, or the end of pastegrab, takes down with all they start.
#[derive(Debug)]
pub struct LocalRunner(());

impl LocalRunner {
    /// On Windows this also keeps a child's missing DLL from opening a system dialog.
    #[must_use]
    pub fn new() -> Self {
        sys::quiet_child_errors();
        Self(())
    }
}

impl Default for LocalRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessRunner for LocalRunner {
    fn spawn(&self, spec: &ProcSpec, cancel: &Cancel) -> Result<Box<dyn Lines>, Failure> {
        let pipe_failure = |e: io::Error| Failure::internal(format!("output pipe: {e}"));
        let (reader, writer) = io::pipe().map_err(pipe_failure)?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(writer.try_clone().map_err(pipe_failure)?)
            .stderr(writer);
        if let Some(dir) = &spec.cwd {
            command.current_dir(dir);
        }
        sys::configure(&mut command, spec.low_priority);
        let broken = |e: io::Error| {
            let program = spec.program.display();
            Failure::new(FailureKind::ToolBroken, format!("starting {program}: {e}"))
        };
        let child = command.spawn().map_err(broken)?;
        // The command keeps copies of the pipe's write end, and EOF needs every copy closed.
        drop(command);
        let child = Contained::new(child).map_err(broken)?;
        let killer = child.killer();
        let _ = cancel.on_cancel(move || killer.kill());
        Ok(Box::new(ChildLines {
            reader: BufReader::new(reader),
            child,
        }))
    }
}

struct ChildLines {
    reader: BufReader<PipeReader>,
    child: Contained,
}

impl Lines for ChildLines {
    fn next_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
        buf.clear();
        if (&mut self.reader).take(MAX_LINE).read_until(b'\n', buf)? == 0 {
            return Ok(false);
        }
        while let Some(b'\n' | b'\r') = buf.last() {
            buf.pop();
        }
        Ok(true)
    }

    fn wait(self: Box<Self>) -> io::Result<Exit> {
        let Self { mut reader, child } = *self;
        // A child blocked on a full pipe never exits.
        io::copy(&mut reader, &mut io::sink())?;
        drop(reader);
        child.wait()
    }
}
