//! yt-dlp's command lines. Output files are named relative to `-P`: yt-dlp reads those names
//! as templates, where a `%` in an absolute path would become a field.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::Duration;

use pastegrab_domain::link::Link;
use pastegrab_domain::plan::FetchPlan;
use pastegrab_domain::profile::Container;

use crate::Engine;
use crate::progress::{DOWNLOAD_TEMPLATE, POSTPROCESS_TEMPLATE};
use crate::summary::TEMPLATE;

pub(crate) const SUMMARY: &str = "summary.json";
pub(crate) const INFO_JSON: &str = "probe.info.json";
pub(crate) const RESULT: &str = "result.txt";

#[derive(Default)]
struct Args(Vec<OsString>);

impl Args {
    fn flag(&mut self, flag: &str) -> &mut Self {
        self.0.push(flag.into());
        self
    }

    fn pair(&mut self, flag: &str, value: impl AsRef<OsStr>) -> &mut Self {
        self.0.push(flag.into());
        self.0.push(value.as_ref().to_owned());
        self
    }
}

pub(crate) fn fixed(engine: &Engine) -> Vec<OsString> {
    let mut args = Args::default();
    args.flag("--ignore-config")
        .flag("--no-plugin-dirs")
        .flag("--no-remote-components");
    if let Some(runtime) = &engine.js_runtime {
        args.flag("--no-js-runtimes").pair("--js-runtimes", runtime);
    }
    if let Some(dir) = &engine.ffmpeg_dir {
        args.pair("--ffmpeg-location", dir);
    }
    args.pair("--cache-dir", &engine.cache_dir)
        .flag("--quiet")
        .flag("--progress")
        .flag("--newline")
        .pair("--progress-delta", "0.5")
        .pair("--color", "never")
        .pair("--progress-template", DOWNLOAD_TEMPLATE)
        .pair("--progress-template", POSTPROCESS_TEMPLATE)
        .pair("--encoding", "utf-8")
        .flag("--windows-filenames");
    if let Some(ca) = &engine.ca_file {
        args.pair("--downloader-args", tls_args(ca));
    }
    args.pair("-N", "4")
        .pair("--retries", "10")
        .pair("--fragment-retries", "10");
    args.0
}

/// yt-dlp splits this value like a POSIX shell, so the path goes in single quotes.
fn tls_args(ca: &Path) -> String {
    let path = ca.to_string_lossy().replace('\'', r#"'"'"'"#);
    format!("ffmpeg_i:-tls_verify 1 -ca_file '{path}'")
}

pub(crate) fn probe(link: &Link, job: &Path, impersonate: bool) -> Vec<OsString> {
    let mut args = Args::default();
    args.flag("--no-playlist")
        .flag("--no-simulate")
        .flag("--skip-download")
        .flag("--write-info-json")
        .pair("-P", job)
        .pair("-o", "probe")
        .pair("--print-to-file", TEMPLATE)
        .0
        .push(SUMMARY.into());
    if impersonate {
        args.pair("--impersonate", "chrome");
        if link.site.is_none() {
            args.pair("--extractor-args", "generic:impersonate");
        }
    }
    args.flag("--").0.push(link.url.as_str().into());
    args.0
}

pub(crate) fn fetch(
    plan: &FetchPlan,
    info_json: &Path,
    work: &Path,
    impersonate: bool,
) -> Vec<OsString> {
    let mut args = Args::default();
    args.pair("--load-info-json", info_json)
        .pair("-f", plan.selector());
    if let Some(ext) = plan.merge_into.and_then(merge_format) {
        args.pair("--merge-output-format", ext);
    }
    if let Some(section) = plan.section {
        let range = format!("*{}-{}", seconds(section.start()), seconds(section.end()));
        args.pair("--download-sections", range);
    }
    if impersonate {
        args.pair("--impersonate", "chrome");
    }
    args.pair("-P", work)
        .pair("-o", "%(id)s.%(ext)s")
        .pair("--print-to-file", "after_move:filepath")
        .0
        .push(RESULT.into());
    args.0
}

const fn merge_format(container: Container) -> Option<&'static str> {
    match container {
        Container::Mp4 => Some("mp4"),
        Container::Mkv => Some("mkv"),
        Container::M4a | Container::Mp3 => None,
    }
}

fn seconds(at: Duration) -> String {
    match at.subsec_millis() {
        0 => at.as_secs().to_string(),
        ms => format!("{}.{ms:03}", at.as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use pastegrab_domain::link::analyze;
    use pastegrab_domain::media::FormatId;
    use pastegrab_domain::plan::{FetchPlan, Section};
    use pastegrab_domain::profile::Container;

    use super::{fetch, fixed, probe};
    use crate::Engine;

    fn text(args: &[std::ffi::OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn fixed_flags_with_every_tool_named() {
        let engine = Engine {
            js_runtime: Some("quickjs:C:\\tools\\qjs\\2026-06-04\\qjs.exe".into()),
            ffmpeg_dir: Some(PathBuf::from("C:\\tools\\ffmpeg\\8.1.1")),
            ca_file: Some(PathBuf::from("C:\\Users\\O'Neil\\cacert.pem")),
            ..Engine::new(
                PathBuf::from("python.exe"),
                PathBuf::from("C:\\data\\cache"),
            )
        };
        let args = text(&fixed(&engine));
        let at = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .map(|i| args[i + 1].as_str())
        };
        assert_eq!(
            at("--js-runtimes"),
            Some("quickjs:C:\\tools\\qjs\\2026-06-04\\qjs.exe")
        );
        assert_eq!(
            at("--downloader-args"),
            Some(r#"ffmpeg_i:-tls_verify 1 -ca_file 'C:\Users\O'"'"'Neil\cacert.pem'"#)
        );
        assert_eq!(at("--encoding"), Some("utf-8"));
        assert!(args.contains(&"--no-js-runtimes".to_owned()));
    }

    #[test]
    fn the_probe_ends_with_the_url_after_a_separator() {
        let link = analyze("https://www.youtube.com/watch?v=AbC123xyz_0&list=PLq9z")
            .pop()
            .expect("a link");
        let args = text(&probe(&link, Path::new("/data/jobs/1"), false));
        assert_eq!(
            args[args.len() - 2..],
            [
                "--",
                "https://www.youtube.com/watch?v=AbC123xyz_0&list=PLq9z"
            ]
        );
        assert!(args.contains(&"--no-playlist".to_owned()));
        assert!(!args.contains(&"--impersonate".to_owned()));

        let unknown = analyze("https://example.org/news/some-article")
            .pop()
            .expect("a link");
        let args = text(&probe(&unknown, Path::new("/data/jobs/2"), true));
        assert!(args.contains(&"generic:impersonate".to_owned()));
    }

    #[test]
    fn the_fetch_names_formats_container_and_section() {
        let plan = FetchPlan {
            format_ids: ["137", "140"]
                .iter()
                .filter_map(|id| FormatId::parse(id))
                .collect(),
            merge_into: Some(Container::Mp4),
            section: Section::new(Duration::from_secs(5010), Duration::from_millis(5_040_500)),
            expected_bytes: None,
            encodes_video: true,
        };
        let args = text(&fetch(
            &plan,
            Path::new("/data/jobs/1/probe.info.json"),
            Path::new("/videos/.pastegrab/1"),
            false,
        ));
        assert_eq!(
            args,
            [
                "--load-info-json",
                "/data/jobs/1/probe.info.json",
                "-f",
                "137+140",
                "--merge-output-format",
                "mp4",
                "--download-sections",
                "*5010-5040.500",
                "-P",
                "/videos/.pastegrab/1",
                "-o",
                "%(id)s.%(ext)s",
                "--print-to-file",
                "after_move:filepath",
                "result.txt",
            ]
        );
    }
}
