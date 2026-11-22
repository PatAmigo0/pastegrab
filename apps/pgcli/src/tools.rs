//! Where the sidecars live: a `--tools` folder laid out like an install, or `PATH`.

use std::env::{self, consts::EXE_SUFFIX};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use pastegrab_ffmpeg::Tools;
use pastegrab_ytdlp::Engine;

/// `python` with yt-dlp packed in, `qjs` and `ffmpeg`, each in a folder of that name.
pub(crate) fn bundled(
    dir: &Path,
    cache: PathBuf,
    temp: PathBuf,
) -> Result<(Engine, Tools), String> {
    let exe = |folder: &str, name: &str| dir.join(folder).join(format!("{name}{EXE_SUFFIX}"));
    let (python, qjs) = (exe("python", "python"), exe("qjs", "qjs"));
    let tools = Tools {
        ffmpeg: exe("ffmpeg", "ffmpeg"),
        ffprobe: exe("ffmpeg", "ffprobe"),
    };
    for tool in [&python, &qjs, &tools.ffmpeg, &tools.ffprobe] {
        if !tool.is_file() {
            return Err(format!("no {}", tool.display()));
        }
    }
    let mut runtime = OsString::from("quickjs:");
    runtime.push(&qjs);
    let certifi = dir.join("python").join("site-packages").join("certifi");
    let engine = Engine {
        program_args: vec!["-I".into(), "-m".into(), "yt_dlp".into()],
        js_runtime: Some(runtime),
        ffmpeg_dir: Some(dir.join("ffmpeg")),
        ca_file: Some(certifi.join("cacert.pem")).filter(|file| file.is_file()),
        temp_dir: Some(temp),
        ..Engine::new(python, cache)
    };
    Ok((engine, tools))
}

/// yt-dlp, ffmpeg and ffprobe from `PATH`, checking certificates against the system's bundle.
pub(crate) fn from_path(cache: PathBuf, temp: PathBuf) -> Result<(Engine, Tools), String> {
    let ffmpeg = find("ffmpeg")?;
    let system_roots = PathBuf::from("/etc/ssl/certs/ca-certificates.crt");
    let engine = Engine {
        ffmpeg_dir: ffmpeg.parent().map(Path::to_owned),
        ca_file: Some(system_roots).filter(|file| file.is_file()),
        temp_dir: Some(temp),
        ..Engine::new(find("yt-dlp")?, cache)
    };
    let tools = Tools {
        ffmpeg,
        ffprobe: find("ffprobe")?,
    };
    Ok((engine, tools))
}

fn find(name: &str) -> Result<PathBuf, String> {
    let file = format!("{name}{EXE_SUFFIX}");
    env::var_os("PATH")
        .iter()
        .flat_map(env::split_paths)
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("{file} is not on PATH; pass --tools"))
}
