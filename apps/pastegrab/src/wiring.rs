use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use pastegrab_app::ports::{ProcessRunner, SettingsStore};
use pastegrab_app::queue::{Queue, Settings};
use pastegrab_app::runtime;
use pastegrab_app::steps::Ports;
use pastegrab_domain::job::Permits;
use pastegrab_domain::plan::Machine;
use pastegrab_domain::profile::ProfileKind;
use pastegrab_ffmpeg::{Ffmpeg, Tools};
use pastegrab_http::Direct;
use pastegrab_os::{LocalFiles, LocalRunner};
use pastegrab_store::JsonSettings;
use pastegrab_win32::{Startup, WinView};
use pastegrab_ytdlp::{Engine, YtDlp};

pub fn run() -> ExitCode {
    let _ = pastegrab_os::harden_dll_search();
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            pastegrab_win32::alert(&problem);
            ExitCode::FAILURE
        }
    }
}

fn start() -> Result<(), String> {
    let text = env::args().skip(1).collect::<Vec<_>>().join("\n");
    let Some(_instance) = pastegrab_win32::claim(&text) else {
        return Ok(());
    };
    let data = env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("pastegrab"))
        .ok_or("LOCALAPPDATA is unset")?;
    let temp = data.join("tmp");
    fs::create_dir_all(&temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    let temp = pastegrab_os::short_path(&temp);
    let sidecars = pastegrab_deploy::bundled(&tools_folder()?)?;
    let engine = Engine {
        program_args: sidecars.ytdlp_args,
        js_runtime: sidecars.js_runtime,
        ffmpeg_dir: Some(sidecars.ffmpeg_dir),
        ca_file: sidecars.ca_file,
        temp_dir: Some(temp),
        ..Engine::new(sidecars.ytdlp, data.join("cache").join("yt-dlp"))
    };
    let tools = Tools {
        ffmpeg: sidecars.ffmpeg,
        ffprobe: sidecars.ffprobe,
    };
    let runner: Arc<dyn ProcessRunner> = Arc::new(LocalRunner::new());
    let ytdlp = YtDlp::new(Arc::clone(&runner), engine);
    let ffmpeg = Ffmpeg::new(runner, tools);
    let settings = JsonSettings::new(data.join("settings.json"));
    let choices = settings.load().unwrap_or_default();
    let ports = Ports {
        resolvers: vec![Box::new(Direct::default()), Box::new(ytdlp.clone())],
        fetchers: vec![Box::new(Direct::default()), Box::new(ytdlp)],
        probe: Box::new(ffmpeg.clone()),
        transcoder: Box::new(ffmpeg),
        files: Box::new(LocalFiles),
        settings: Some(Box::new(settings)),
    };
    // A chosen folder on a drive that is gone falls back to Videos.
    let dest = choices
        .dest
        .filter(|dir| dir.is_dir())
        .or_else(pastegrab_win32::videos_folder)
        .or_else(|| env::home_dir().map(|home| home.join("Videos")))
        .ok_or("no Videos folder")?;
    let queue = Queue::new(Settings {
        profile: choices.profile.unwrap_or(ProfileKind::Video),
        machine: Machine {
            screen_height: pastegrab_win32::screen_height(),
        },
        dest: dest.clone(),
        data,
        permits: Permits::DEFAULT,
    });
    let view = Arc::new(WinView::default());
    let (core, _) = runtime::start(queue, Arc::new(ports), Arc::clone(&view) as _, false);
    pastegrab_win32::run(core, view, Startup { dest, text })
}

/// `tools` beside the `app` folder in an install, or beside the exe in a development copy.
fn tools_folder() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let here = exe.parent().unwrap_or(Path::new("."));
    [
        here.parent().map(|up| up.join("tools")),
        Some(here.join("tools")),
    ]
    .into_iter()
    .flatten()
    .find(|dir| dir.is_dir())
    .ok_or_else(|| format!("no tools folder near {}", exe.display()))
}
