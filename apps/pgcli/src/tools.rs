//! The tools found by `pastegrab-deploy`, set up for the yt-dlp and ffmpeg adapters.

use std::path::{Path, PathBuf};

use pastegrab_deploy::Sidecars;
use pastegrab_ffmpeg::Tools;
use pastegrab_ytdlp::Engine;

/// `--tools DIR` when given, `PATH` otherwise.
pub(crate) fn find(
    dir: Option<&Path>,
    cache: PathBuf,
    temp: PathBuf,
) -> Result<(Engine, Tools), String> {
    let sidecars = match dir {
        Some(dir) => pastegrab_deploy::bundled(dir)?,
        None => pastegrab_deploy::from_path().map_err(|e| format!("{e}; pass --tools"))?,
    };
    Ok(adapters(sidecars, cache, temp))
}

fn adapters(s: Sidecars, cache: PathBuf, temp: PathBuf) -> (Engine, Tools) {
    let engine = Engine {
        program_args: s.ytdlp_args,
        js_runtime: s.js_runtime,
        ffmpeg_dir: Some(s.ffmpeg_dir),
        ca_file: s.ca_file,
        temp_dir: Some(temp),
        ..Engine::new(s.ytdlp, cache)
    };
    let tools = Tools {
        ffmpeg: s.ffmpeg,
        ffprobe: s.ffprobe,
    };
    (engine, tools)
}
