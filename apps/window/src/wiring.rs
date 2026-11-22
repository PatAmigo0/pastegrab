use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use pastegrab_app::ports::{ProcessRunner, SettingsStore};
use pastegrab_app::queue::{Preferences, Queue, Settings};
use pastegrab_app::runtime::{self, Handle};
use pastegrab_app::steps::Ports;
use pastegrab_deploy::{cleanup, launch, machine};
use pastegrab_domain::job::Permits;
use pastegrab_domain::plan::Machine;
use pastegrab_domain::profile::{HeightCap, ProfileKind};
use pastegrab_ffmpeg::{Ffmpeg, Tools};
use pastegrab_http::Direct;
use pastegrab_os::{LocalFiles, LocalRunner};
use pastegrab_store::JsonSettings;
use pastegrab_update::Version;
use pastegrab_win32::{Startup, UpdateControl, WinView};
use pastegrab_ytdlp::{Engine, YtDlp};

pub fn run() -> ExitCode {
    let _ = pastegrab_os::harden_dll_search();
    pastegrab_win32::enable_dpi();
    if env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--uninstall")
    {
        return crate::uninstall::run();
    }
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
    let Some(instance) = pastegrab_win32::claim(&text) else {
        return Ok(());
    };
    let data = pastegrab_deploy::data_folder().ok_or("LOCALAPPDATA is unset")?;
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
    // What setup tuned for this PC; a copy without an install takes the defaults.
    let tuning = machine::read(&data).map_or_default(|deployment| deployment.tuning);
    let tools = Tools {
        ffmpeg: sidecars.ffmpeg,
        ffprobe: sidecars.ffprobe,
        x264_preset: tuning.preset.name(),
    };
    let runner: Arc<dyn ProcessRunner> = Arc::new(LocalRunner::new());
    let direct = Direct::default();
    let ytdlp = YtDlp::with_direct(Arc::clone(&runner), engine, Arc::new(direct.clone()));
    let ffmpeg = Ffmpeg::new(runner, tools);
    let settings = JsonSettings::new(data.join("settings.json"));
    let choices = settings.load().unwrap_or_default();
    let ports = Ports {
        resolvers: vec![Box::new(direct.clone()), Box::new(ytdlp.clone())],
        fetchers: vec![Box::new(direct), Box::new(ytdlp)],
        probe: Box::new(ffmpeg.clone()),
        transcoder: Box::new(ffmpeg),
        files: Box::new(LocalFiles),
        settings: Some(Box::new(settings)),
    };
    let dest = resolve_and_sweep_dest(choices.dest.as_deref(), &data)?;
    let tutorial_seen = choices.tutorial.unwrap_or(0);
    let preferences = Preferences {
        clipboard: choices.clipboard.unwrap_or(true),
        close_to_tray: choices.close_to_tray.unwrap_or(false),
        height_cap: choices.max_height.unwrap_or(HeightCap::Screen),
        downloads: choices
            .downloads
            .map_or(tuning.downloads, |n| n.clamp(1, 4)),
    };
    let updater: Arc<dyn UpdateControl> = Arc::new(crate::update::AppUpdater::new(data.clone()));
    let data_for_update = data.clone();
    let queue = Queue::new(Settings {
        profile: choices.profile.unwrap_or(ProfileKind::Video),
        machine: Machine {
            screen_height: pastegrab_win32::screen_height(),
        },
        dest: dest.clone(),
        data,
        permits: Permits {
            fetches: usize::from(preferences.downloads),
            ..Permits::DEFAULT
        },
        tutorial: tutorial_seen,
        clipboard: preferences.clipboard,
        close_to_tray: preferences.close_to_tray,
        height_cap: preferences.height_cap,
        downloads_override: choices.downloads,
    });
    let view = Arc::new(WinView::default());
    let (core, runtime_thread) =
        runtime::start(queue, Arc::new(ports), Arc::clone(&view) as _, false);
    let stopper = Stopper::new(core.clone(), runtime_thread);
    let shutdown_hook = {
        let stopper = stopper.clone();
        move || stopper.stop()
    };
    let shown = move || {
        confirm_version();
        crate::update::start(data_for_update);
    };
    let restart = pastegrab_win32::run(
        core,
        view,
        Startup {
            dest,
            text,
            tutorial_seen,
            preferences,
            updater,
        },
        shown,
        shutdown_hook,
    )?;
    stopper.stop();
    drop(instance);
    if restart {
        crate::update::relaunch()?;
    }
    Ok(())
}

/// Resolves the destination folder (falling back to Videos) and sweeps leftover temp folders.
fn resolve_and_sweep_dest(saved: Option<&Path>, data: &Path) -> Result<PathBuf, String> {
    let videos = pastegrab_win32::videos_folder()
        .or_else(|| env::home_dir().map(|home| home.join("Videos")));
    let dest = saved
        .filter(|dir| dir.is_dir())
        .map(Path::to_path_buf)
        .or_else(|| videos.clone())
        .ok_or("no Videos folder")?;
    let install_root = env::current_exe()
        .ok()
        .and_then(|exe| launch::root_of(&exe).map(Path::to_path_buf));
    let mut sweep_dests = vec![dest.as_path()];
    if let Some(videos) = videos.as_deref() {
        sweep_dests.push(videos);
    }
    cleanup::sweep_startup(&sweep_dests, data, install_root.as_deref());
    Ok(dest)
}

struct Running<T> {
    core: Handle,
    thread: thread::JoinHandle<T>,
}

#[derive(Clone)]
struct Stopper<T: Send + 'static> {
    inner: Arc<Mutex<Option<Running<T>>>>,
}

impl<T: Send + 'static> Stopper<T> {
    fn new(core: Handle, thread: thread::JoinHandle<T>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(Running { core, thread }))),
        }
    }

    /// Cancels active work and waits briefly for workers to clean their working folders.
    fn stop(&self) {
        let Some(Running { core, thread }) = self
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        core.shutdown();
        drop(core);
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let _ = thread::Builder::new()
            .name("shutdown-wait".into())
            .spawn(move || {
                let _ = thread.join();
                let _ = done_tx.send(());
            });
        let _ = done_rx.recv_timeout(Duration::from_secs(2));
    }
}

/// `tools` beside the `app` folder in an install, or beside the exe in a development copy.
fn tools_folder() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let root = launch::root_of(&exe)
        .or_else(|| exe.parent())
        .unwrap_or(Path::new("."));
    let tools = root.join("tools");
    tools
        .is_dir()
        .then_some(tools)
        .ok_or_else(|| format!("no tools folder near {}", exe.display()))
}

/// The window opened, which ends this version's trial in the launcher's `state.json`.
fn confirm_version() {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let (Some(root), Ok(own)) = (
        launch::root_of(&exe),
        env!("CARGO_PKG_VERSION").parse::<Version>(),
    ) else {
        return;
    };
    let Some(mut state) = launch::read_state(root) else {
        return;
    };
    let before = state.clone();
    if state.current() == own {
        state.confirm();
    }
    if state != before {
        let _ = launch::write_state(root, &state);
    }
}
