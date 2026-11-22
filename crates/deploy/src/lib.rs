//! Where pastegrab and its tools sit: an install's folders, or `PATH` on a development machine,
//! and on Windows the shortcuts and the Programs and Features entry that point at an install.

#[cfg(windows)]
mod sys;

#[cfg(windows)]
pub use sys::{Entry, register, unregister};

use std::env::{self, consts::EXE_SUFFIX};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\Programs\pastegrab`, where setup installs.
#[must_use]
pub fn install_folder() -> Option<PathBuf> {
    local_app_data().map(|dir| dir.join("Programs").join("pastegrab"))
}

/// `%LOCALAPPDATA%\pastegrab`: settings, caches, probe output and the sidecars' temp files.
#[must_use]
pub fn data_folder() -> Option<PathBuf> {
    local_app_data().map(|dir| dir.join("pastegrab"))
}

fn local_app_data() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// The tools a download needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecars {
    /// The program that runs yt-dlp.
    pub ytdlp: PathBuf,
    /// Arguments before yt-dlp's own.
    pub ytdlp_args: Vec<OsString>,
    /// yt-dlp's `--js-runtimes` value when a runtime ships with pastegrab.
    pub js_runtime: Option<OsString>,
    /// The folder with ffmpeg and ffprobe.
    pub ffmpeg_dir: PathBuf,
    /// ffmpeg.
    pub ffmpeg: PathBuf,
    /// ffprobe.
    pub ffprobe: PathBuf,
    /// The CA bundle ffmpeg checks certificates against.
    pub ca_file: Option<PathBuf>,
}

/// The tools in an install's `tools` folder: Python with yt-dlp packed in, `QuickJS` and ffmpeg,
/// each in a folder of its name.
///
/// # Errors
///
/// The first tool that is missing.
pub fn bundled(dir: &Path) -> Result<Sidecars, String> {
    let exe = |folder: &str, name: &str| dir.join(folder).join(format!("{name}{EXE_SUFFIX}"));
    let (python, qjs) = (exe("python", "python"), exe("qjs", "qjs"));
    let (ffmpeg, ffprobe) = (exe("ffmpeg", "ffmpeg"), exe("ffmpeg", "ffprobe"));
    for tool in [&python, &qjs, &ffmpeg, &ffprobe] {
        if !tool.is_file() {
            return Err(format!("no {}", tool.display()));
        }
    }
    let mut runtime = OsString::from("quickjs:");
    runtime.push(&qjs);
    let certifi = dir.join("python").join("site-packages").join("certifi");
    Ok(Sidecars {
        ytdlp: python,
        ytdlp_args: vec!["-I".into(), "-m".into(), "yt_dlp".into()],
        js_runtime: Some(runtime),
        ffmpeg_dir: dir.join("ffmpeg"),
        ffmpeg,
        ffprobe,
        ca_file: Some(certifi.join("cacert.pem")).filter(|file| file.is_file()),
    })
}

/// yt-dlp, ffmpeg and ffprobe from `PATH`, checking certificates against the system's bundle.
///
/// # Errors
///
/// The first tool `PATH` lacks.
pub fn from_path() -> Result<Sidecars, String> {
    let ffmpeg = find("ffmpeg")?;
    let system_roots = PathBuf::from("/etc/ssl/certs/ca-certificates.crt");
    Ok(Sidecars {
        ytdlp: find("yt-dlp")?,
        ytdlp_args: Vec::new(),
        js_runtime: None,
        ffmpeg_dir: ffmpeg.parent().map_or_default(Path::to_owned),
        ffprobe: find("ffprobe")?,
        ffmpeg,
        ca_file: Some(system_roots).filter(|file| file.is_file()),
    })
}

fn find(name: &str) -> Result<PathBuf, String> {
    let file = format!("{name}{EXE_SUFFIX}");
    env::var_os("PATH")
        .iter()
        .flat_map(env::split_paths)
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("{file} is not on PATH"))
}
