//! The direct resolver and fetcher against a small HTTP server on localhost.

#![allow(clippy::expect_used, reason = "fixture helpers outside #[test] fns")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread;

use pastegrab_app::Cancel;
use pastegrab_app::ports::{
    FetchRequest, Fetcher, Phase, Progress, ProgressSink, Resolver, Source,
};
use pastegrab_domain::job::FailureKind;
use pastegrab_domain::link::{Link, analyze};
use pastegrab_domain::media::FormatId;
use pastegrab_domain::plan::FetchPlan;
use pastegrab_http::Direct;
use pastegrab_testkit::TempDir;

/// Serves `body` with `status` to `requests` connections, honouring a `Range` header.
fn serve(body: Vec<u8>, status: u16, requests: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address");
    thread::spawn(move || {
        for stream in listener.incoming().take(requests) {
            let mut stream = stream.expect("a connection");
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut range = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("a header line");
                if line.trim().is_empty() {
                    break;
                }
                if let Some(spec) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    let (start, end) = spec.trim().split_once('-').expect("a range");
                    range = Some((
                        start.parse::<usize>().expect("a start"),
                        end.parse::<usize>().expect("an end"),
                    ));
                }
            }
            let head = match (status, range) {
                (200, Some((start, end))) => {
                    let end = end.min(body.len() - 1);
                    let length = end + 1 - start;
                    let total = body.len();
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {length}\r\n\
                         Content-Range: bytes {start}-{end}/{total}\r\nConnection: close\r\n\r\n"
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(body[start..=end].iter().copied())
                    .collect::<Vec<u8>>()
                }
                (200, None) => format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes()
                .into_iter()
                .chain(body.iter().copied())
                .collect(),
                _ => format!(
                    "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .into_bytes(),
            };
            let _ = stream.write_all(&head);
        }
    });
    format!("http://{address}")
}

fn mp4(len: usize) -> Vec<u8> {
    let mut body = b"\0\0\0\x18ftypmp42".to_vec();
    body.resize(len, 7);
    body
}

fn link(url: &str) -> Link {
    analyze(url).pop().expect("a link")
}

#[derive(Default)]
struct Last(Mutex<Option<Progress>>);

impl ProgressSink for Last {
    fn report(&self, phase: Phase, progress: Progress) {
        assert_eq!(phase, Phase::Download);
        *self.0.lock().expect("the last report") = Some(progress);
    }
}

fn fetch(
    url: &str,
    work: &std::path::Path,
) -> (Result<std::path::PathBuf, pastegrab_app::Failure>, Last) {
    let plan = FetchPlan {
        format_ids: FormatId::parse("direct").into_iter().collect(),
        merge_into: None,
        section: None,
        expected_bytes: None,
        encodes_video: false,
    };
    let source = Source::Direct {
        url: url.to_owned(),
    };
    let last = Last::default();
    let request = FetchRequest {
        plan: &plan,
        source: &source,
        work_dir: work,
    };
    let result = Direct::default().fetch(request, &last, &Cancel::default());
    (result, last)
}

#[test]
fn a_direct_link_resolves_with_its_size_and_name() {
    let base = serve(mp4(300_000), 200, 1);
    let url = format!("{base}/media/%D0%B7%D0%B0%D0%B1%D0%B5%D0%B3.mp4");
    let found = Direct::default()
        .resolve(
            &link(&url),
            std::path::Path::new("/unused"),
            false,
            &Cancel::default(),
        )
        .expect("a resolve");
    assert_eq!(found.info.title, "забег");
    assert_eq!(found.info.formats[0].size, Some(300_000));
    assert!(found.info.formats[0].video.is_some());
    assert_eq!(found.source, Source::Direct { url });
}

#[test]
fn a_fetch_streams_the_file_whole() {
    let body = mp4(700_000);
    let base = serve(body.clone(), 200, 1);
    let dir = TempDir::new("http-fetch").expect("a temp folder");
    let (file, last) = fetch(&format!("{base}/clip.mp4"), dir.path());
    let file = file.expect("a file");
    assert_eq!(file, dir.path().join("download.mp4"));
    assert_eq!(std::fs::read(&file).expect("the file"), body);
    let last = last.0.lock().expect("reports").expect("a report");
    assert_eq!((last.done, last.total), (700_000, Some(700_000)));
}

#[test]
fn a_page_that_is_no_media_never_lands() {
    let page = b"<!DOCTYPE html><html><body>nothing here</body></html>".repeat(10);
    let base = serve(page, 200, 2);
    let url = format!("{base}/video.mp4");
    let refused = Direct::default()
        .resolve(
            &link(&url),
            std::path::Path::new("/unused"),
            false,
            &Cancel::default(),
        )
        .expect_err("not media");
    assert_eq!(refused.kind, FailureKind::Unsupported);
    let dir = TempDir::new("http-page").expect("a temp folder");
    let (file, _) = fetch(&url, dir.path());
    assert_eq!(file.expect_err("not media").kind, FailureKind::Unsupported);
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("the folder").count(),
        0
    );
}

#[test]
fn a_missing_file_is_unavailable() {
    let base = serve(Vec::new(), 404, 1);
    let failure = Direct::default()
        .resolve(
            &link(&format!("{base}/gone.mp4")),
            std::path::Path::new("/unused"),
            false,
            &Cancel::default(),
        )
        .expect_err("a 404");
    assert_eq!(failure.kind, FailureKind::Unavailable);
}
