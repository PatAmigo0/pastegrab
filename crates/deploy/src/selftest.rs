//! Setup's self-test: each tool answers with its version, x264 encodes a second of test pattern,
//! and `QuickJS` computes. Setup's report and `deploy.json` take the results.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::Sidecars;
use crate::machine::Preset;

/// A cold Python start on a slow disk takes tens of seconds; a hung tool shouldn't hold setup.
const LIMIT: Duration = Duration::from_secs(120);

/// What the self-test runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Python with yt-dlp, asked for its version.
    YtDlp,
    /// ffmpeg's version.
    Ffmpeg,
    /// ffprobe's version.
    Ffprobe,
    /// `QuickJS` computing 6 times 7.
    QuickJs,
    /// x264 encoding a second of test pattern at the tuned preset.
    Encode,
}

/// One probe: what the tool answered, or why it failed, and how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The probe.
    pub probe: Probe,
    /// The version, or nothing for the encode and `QuickJS`; the reason on failure.
    pub result: Result<String, String>,
    /// How long it ran.
    pub took: Duration,
}

/// Runs every probe in turn.
#[must_use]
pub fn run(sidecars: &Sidecars, preset: Preset) -> Vec<Check> {
    let ffmpeg = |args: &[&str]| {
        let mut command = Command::new(&sidecars.ffmpeg);
        command.args(args);
        command
    };
    let mut ytdlp = Command::new(&sidecars.ytdlp);
    ytdlp.args(&sidecars.ytdlp_args).arg("--version");
    let mut ffprobe = Command::new(&sidecars.ffprobe);
    ffprobe.args(["-hide_banner", "-version"]);
    let encode = ffmpeg(&[
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=1:size=640x360:rate=25",
        "-c:v",
        "libx264",
        "-preset",
        preset.name(),
        "-f",
        "null",
        "-",
    ]);
    let mut probes = vec![
        (Probe::YtDlp, ytdlp, ""),
        (
            Probe::Ffmpeg,
            ffmpeg(&["-hide_banner", "-version"]),
            "ffmpeg version ",
        ),
        (Probe::Ffprobe, ffprobe, "ffprobe version "),
        (Probe::Encode, encode, ""),
    ];
    if let Some(qjs) = &sidecars.qjs {
        let mut quickjs = Command::new(qjs);
        quickjs.args(["-e", "print(6*7)"]);
        probes.push((Probe::QuickJs, quickjs, ""));
    }
    probes
        .into_iter()
        .map(|(probe, command, prefix)| {
            let started = Instant::now();
            let answer = run_limited(command, LIMIT);
            let result = match (probe, answer) {
                (Probe::Encode, Ok(_)) => Ok(String::new()),
                (Probe::QuickJs, Ok(out)) if out.trim() == "42" => Ok(String::new()),
                (_, Ok(out)) => first_line_after(&out, prefix)
                    .ok_or_else(|| format!("an unexpected answer: {}", out.trim())),
                (_, Err(e)) => Err(e),
            };
            Check {
                probe,
                result,
                took: started.elapsed(),
            }
        })
        .collect()
}

/// The first word after `prefix` on the first line, such as a version.
pub(crate) fn first_line_after(text: &str, prefix: &str) -> Option<String> {
    let line = text.lines().next()?.strip_prefix(prefix)?;
    line.split_whitespace().next().map(str::to_owned)
}

/// Runs `command` without a console window and returns its output, or why it failed: an exit
/// code with its last error line, no start, or no answer within `limit`.
pub(crate) fn run_limited(mut command: Command, limit: Duration) -> Result<String, String> {
    hide_console(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("no answer in {} ms", limit.as_millis()));
        }
        thread::sleep(Duration::from_millis(50));
    };
    let read = |pipe: Option<&mut dyn Read>| {
        let mut text = String::new();
        if let Some(pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    };
    let stdout = read(child.stdout.as_mut().map(|p| p as &mut dyn Read));
    let stderr = read(child.stderr.as_mut().map(|p| p as &mut dyn Read));
    if status.success() {
        return Ok(stdout);
    }
    let last = stderr.lines().rev().find(|line| !line.trim().is_empty());
    Err(match last {
        Some(line) => format!("{status}: {}", line.trim()),
        None => status.to_string(),
    })
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
const fn hide_console(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    use super::{first_line_after, run_limited};

    #[test]
    fn versions_come_from_the_tools_first_lines() {
        let ffmpeg = "ffmpeg version 8.1.1-essentials_build-www.gyan.dev Copyright (c) 2000-2026\nbuilt with gcc";
        assert_eq!(
            first_line_after(ffmpeg, "ffmpeg version "),
            Some("8.1.1-essentials_build-www.gyan.dev".to_owned())
        );
        assert_eq!(
            first_line_after("2026.10.26\n", ""),
            Some("2026.10.26".to_owned())
        );
        assert_eq!(first_line_after("", ""), None);
        assert_eq!(
            first_line_after("Error: no module", "ffmpeg version "),
            None
        );
    }

    #[test]
    fn a_tool_answers_in_time_or_is_stopped() {
        let shell = |script: &str| {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        };
        let answered = run_limited(shell("echo 42"), Duration::from_secs(10)).expect("an answer");
        assert_eq!(answered.trim(), "42");
        let failed = run_limited(shell("echo broken >&2; exit 3"), Duration::from_secs(10));
        assert!(failed.is_err_and(|e| e.contains("broken")));
        let slow = run_limited(shell("sleep 5"), Duration::from_millis(200));
        assert!(slow.is_err_and(|e| e.contains("200 ms")));
        let missing = run_limited(
            Command::new(PathBuf::from("/nonexistent/tool")),
            Duration::from_secs(1),
        );
        assert!(missing.is_err());
    }
}
