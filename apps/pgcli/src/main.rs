//! pastegrab on the command line, for development and as a downloader of its own.
//!
//! `pgcli [--profile NAME] [--dest DIR] [--screen HEIGHT] [--tools DIR] [--plan] [LINK...]`
//! reads links from its arguments, or from stdin without any, and prints each finished file's
//! path. `--plan` prints what the planner picks and downloads nothing.

mod plan;
mod tools;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use pastegrab_app::ports::ProcessRunner;
use pastegrab_app::queue::{Input, Paste, Queue, Settings};
use pastegrab_app::runtime;
use pastegrab_app::steps::Ports;
use pastegrab_cli::TermView;
use pastegrab_domain::job::{JobState, Permits};
use pastegrab_domain::plan::Machine;
use pastegrab_domain::profile::ProfileKind;
use pastegrab_ffmpeg::Ffmpeg;
use pastegrab_http::Direct;
use pastegrab_os::{LocalFiles, LocalRunner};
use pastegrab_ytdlp::YtDlp;

const USAGE: &str = "usage: pgcli [--profile video|best|small|audio|mp3|analysis] [--dest DIR] \
                     [--screen HEIGHT] [--tools DIR] [--plan] [LINK...]";

fn main() -> ExitCode {
    if let Err(e) = pastegrab_os::harden_dll_search() {
        let _ = writeln!(io::stderr(), "pgcli: DLL search stays as it was: {e}");
    }
    match run() {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(io::stderr(), "pgcli: {message}");
            ExitCode::from(2)
        }
    }
}

struct Options {
    profile: Option<ProfileKind>,
    dest: PathBuf,
    screen_height: u32,
    tools: Option<PathBuf>,
    plan: bool,
    links: Vec<String>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        profile: None,
        dest: env::current_dir().map_err(|e| format!("the current folder: {e}"))?,
        screen_height: 1080,
        tools: None,
        plan: false,
        links: Vec::new(),
    };
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| USAGE.to_owned());
        match arg.to_str() {
            Some("--profile") => options.profile = Some(profile(&value()?)?),
            Some("--dest") => options.dest = PathBuf::from(value()?),
            Some("--screen") => {
                let height = value()?;
                options.screen_height = height
                    .to_str()
                    .and_then(|h| h.parse().ok())
                    .ok_or_else(|| format!("--screen takes a height, not {}", height.display()))?;
            }
            Some("--tools") => options.tools = Some(PathBuf::from(value()?)),
            Some("--plan") => options.plan = true,
            Some("-h" | "--help") => return Err(USAGE.to_owned()),
            Some(link) => options.links.push(link.to_owned()),
            None => return Err(format!("not UTF-8: {}", arg.display())),
        }
    }
    Ok(options)
}

fn profile(name: &OsString) -> Result<ProfileKind, String> {
    Ok(match name.to_str() {
        Some("video") => ProfileKind::Video,
        Some("best") => ProfileKind::BestQuality,
        Some("small") => ProfileKind::Small,
        Some("audio") => ProfileKind::Audio,
        Some("mp3") => ProfileKind::AudioMp3,
        Some("analysis") => ProfileKind::Analysis,
        _ => return Err(format!("no profile {}; {USAGE}", name.display())),
    })
}

fn run() -> Result<ExitCode, String> {
    let options = options()?;
    let text = if options.links.is_empty() {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| format!("reading stdin: {e}"))?;
        text
    } else {
        options.links.join("\n")
    };
    // `canonicalize` gives `\\?\` paths on Windows, which ffmpeg and people misread.
    let dest = std::path::absolute(&options.dest)
        .ok()
        .filter(|dest| dest.is_dir())
        .ok_or_else(|| format!("{} is not a folder", options.dest.display()))?;
    let data = data_dir()?;
    let temp = data.join("tmp");
    fs::create_dir_all(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;

    let temp = pastegrab_os::short_path(&temp);
    let cache = data.join("cache").join("yt-dlp");
    let (engine, tools) = tools::find(options.tools.as_deref(), cache, temp)?;
    let runner: Arc<dyn ProcessRunner> = Arc::new(LocalRunner::new());
    let ytdlp = YtDlp::with_direct(Arc::clone(&runner), engine, Arc::new(Direct::default()));
    let ffmpeg = Ffmpeg::new(runner, tools);
    let machine = Machine {
        screen_height: options.screen_height,
    };
    if options.plan {
        return Ok(plan::run(&ytdlp, &text, options.profile, machine, &data));
    }
    let ports = Ports {
        resolvers: vec![Box::new(Direct::default()), Box::new(ytdlp.clone())],
        fetchers: vec![Box::new(Direct::default()), Box::new(ytdlp)],
        probe: Box::new(ffmpeg.clone()),
        transcoder: Box::new(ffmpeg),
        files: Box::new(LocalFiles),
        settings: None,
    };
    let queue = Queue::new(Settings {
        profile: ProfileKind::Video,
        machine,
        dest,
        data,
        permits: Permits::DEFAULT,
        tutorial: 0,
    });
    let (handle, thread) =
        runtime::start(queue, Arc::new(ports), Arc::new(TermView::default()), true);
    let paste = Paste {
        text,
        profile: options.profile,
        section: None,
        hold: false,
    };
    if !handle.send(Input::Paste(paste)) {
        return Err("the queue stopped before the paste".to_owned());
    }
    drop(handle);
    let states = thread.join().map_err(|_| "the queue thread panicked")?;
    let done = !states.is_empty() && states.iter().all(|(_, state)| *state == JobState::Done);
    Ok(if done {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn data_dir() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| env::home_dir().map(|home| home.join(".local").join("share")))
    };
    Ok(base
        .ok_or("no folder for pastegrab's data")?
        .join("pastegrab"))
}
