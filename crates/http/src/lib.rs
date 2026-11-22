//! Direct media files over HTTP(S): a `Resolver` and a `Fetcher` on ureq with rustls and
//! bundled roots. A file only lands once its first bytes match a media signature, whatever
//! `Content-Type` claims. `Downloads` fetches release files for app updates.

mod downloads;
mod magic;
mod split;

pub use downloads::Downloads;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pastegrab_app::ports::{
    FetchRequest, Fetcher, Phase, Progress, ProgressSink, Resolved, Resolver, Source,
};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;
use pastegrab_domain::link::{Link, LinkKind};
use pastegrab_domain::media::{
    AudioCodec, AudioStream, Format, MediaInfo, Protocol, VideoCodec, VideoStream,
};
use ureq::http::Response;
use ureq::{Agent, ResponseExt};

use magic::{Media, SNIFF_LEN};

const BUFFER: usize = 256 * 1024;
const REPORT_EVERY: Duration = Duration::from_millis(250);
/// Files from this size up come in over several connections where the server allows ranges.
const SPLIT_FROM: u64 = 8 * 1024 * 1024;
const CONNECTIONS: usize = 4;

/// Media files served as they are.
#[derive(Debug, Clone)]
pub struct Direct {
    agent: Agent,
    connections: usize,
    split_from: u64,
}

impl Direct {
    /// Fetches files from `split_from` bytes up over `connections` connections.
    #[must_use]
    pub const fn split_above(mut self, connections: usize, split_from: u64) -> Self {
        self.connections = connections;
        self.split_from = split_from;
        self
    }
}

impl Default for Direct {
    fn default() -> Self {
        let agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .max_idle_connections_per_host(CONNECTIONS)
            .user_agent(concat!("pastegrab/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            agent,
            connections: CONNECTIONS,
            split_from: SPLIT_FROM,
        }
    }
}

impl Resolver for Direct {
    fn accepts(&self, link: &Link) -> bool {
        link.kind == LinkKind::DirectMedia
    }

    fn resolve(&self, link: &Link, _: &Path, _: bool, _: &Cancel) -> Result<Resolved, Failure> {
        let url = link.url.as_str();
        let mut response = self
            .agent
            .get(url)
            .header("Range", "bytes=0-1023")
            .call()
            .map_err(request_failure)?;
        let size = full_size(&response);
        let mut head = [0; SNIFF_LEN];
        let read = read_head(&mut response.body_mut().as_reader(), &mut head)?;
        let media = magic::sniff(&head[..read]).ok_or_else(not_media)?;
        Ok(Resolved {
            info: MediaInfo {
                title: title(link.url.as_url().path()),
                duration: None,
                is_live: false,
                formats: vec![format(media, size)],
            },
            source: Source::Direct {
                url: url.to_owned(),
            },
        })
    }
}

impl Fetcher for Direct {
    fn accepts(&self, source: &Source) -> bool {
        matches!(source, Source::Direct { .. })
    }

    fn fetch(
        &self,
        request: FetchRequest<'_>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<PathBuf, Failure> {
        let Source::Direct { url } = request.source else {
            return Err(Failure::internal("not a direct source"));
        };
        let work = request.work_dir;
        fs::create_dir_all(work).map_err(|e| io_failure(&e, "creating the work folder"))?;
        let name = format!("download.{}", extension(url));
        let part = work.join(format!("{name}.part"));
        let mut response = self
            .agent
            .get(url)
            .header("Range", "bytes=0-")
            .call()
            .map_err(request_failure)?;
        let total = full_size(&response);
        let partial = response.status().as_u16() == 206;
        let resolved_url = response.get_uri().to_string();
        let mut body = response.body_mut().as_reader();

        let mut head = [0; SNIFF_LEN];
        let read = read_head(&mut body, &mut head)?;
        magic::sniff(&head[..read]).ok_or_else(not_media)?;
        if let Some(total) = total.filter(|&total| partial && total >= self.split_from) {
            let split = split::Split {
                agent: &self.agent,
                url: &resolved_url,
                file: &part,
                total,
                connections: self.connections,
            };
            split.download(body, &head[..read], progress, cancel)?;
            return finish(work, &name, &part);
        }
        let mut file = File::create(&part).map_err(|e| io_failure(&e, "creating the file"))?;
        if let Some(total) = total {
            file.set_len(total)
                .map_err(|e| io_failure(&e, "reserving space for the file"))?;
        }
        file.write_all(&head[..read])
            .map_err(|e| io_failure(&e, "writing the file"))?;

        let mut buf = vec![0; BUFFER];
        let mut done = u64::try_from(read).unwrap_or(u64::MAX);
        let started = Instant::now();
        let mut reported = started;
        loop {
            if cancel.is_cancelled() {
                return Err(Failure::internal("cancelled"));
            }
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Failure::new(FailureKind::Network, e.to_string())),
            };
            file.write_all(&buf[..n])
                .map_err(|e| io_failure(&e, "writing the file"))?;
            done = done.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            if reported.elapsed() >= REPORT_EVERY {
                reported = Instant::now();
                progress.report(Phase::Download, sample(done, total, started));
            }
        }
        if total.is_some_and(|expected| done < expected) {
            file.set_len(done)
                .map_err(|e| io_failure(&e, "trimming the file"))?;
        }
        progress.report(Phase::Download, sample(done, total, started));
        file.sync_all()
            .map_err(|e| io_failure(&e, "flushing the file"))?;
        drop(file);
        finish(work, &name, &part)
    }
}

fn finish(work: &Path, name: &str, part: &Path) -> Result<PathBuf, Failure> {
    let finished = work.join(name);
    fs::rename(part, &finished).map_err(|e| io_failure(&e, "naming the file"))?;
    Ok(finished)
}

pub(crate) fn sample(done: u64, total: Option<u64>, started: Instant) -> Progress {
    let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let speed = done.saturating_mul(1000).checked_div(millis);
    let eta = match (total, speed) {
        (Some(total), Some(speed)) => total.saturating_sub(done).checked_div(speed),
        _ => None,
    };
    Progress {
        done,
        total,
        speed,
        eta,
    }
}

fn read_head(body: &mut impl Read, head: &mut [u8]) -> Result<usize, Failure> {
    let mut filled = 0;
    while filled < head.len() {
        match body.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Failure::new(FailureKind::Network, e.to_string())),
        }
    }
    Ok(filled)
}

/// The whole file's size, from `Content-Range` on a partial answer or `Content-Length`.
fn full_size<B>(response: &Response<B>) -> Option<u64> {
    let header = |name: &str| response.headers().get(name)?.to_str().ok();
    match response.status().as_u16() {
        206 => header("content-range")?.rsplit_once('/')?.1.parse().ok(),
        _ => header("content-length")?.parse().ok(),
    }
}

fn format(media: Media, size: Option<u64>) -> Format {
    let codec = match media {
        Media::Mp3 => AudioCodec::Mp3,
        Media::Aac => AudioCodec::Aac,
        Media::Flac => AudioCodec::Flac,
        _ => AudioCodec::Unknown,
    };
    Format {
        id: "direct".into(),
        video: (!media.sound_only()).then_some(VideoStream {
            codec: VideoCodec::Unknown,
            height: None,
            fps: None,
        }),
        audio: Some(AudioStream {
            codec,
            bitrate: None,
        }),
        size,
        bitrate: None,
        protocol: Protocol::Http,
        drm: false,
    }
}

/// The file name from the URL path, decoded, without its extension.
fn title(path: &str) -> String {
    let last = path.rsplit('/').next().unwrap_or_default();
    let name = decode(last);
    let stem = name
        .rsplit_once('.')
        .map_or(name.as_str(), |(stem, _)| stem);
    if stem.trim().is_empty() {
        "video".to_owned()
    } else {
        stem.to_owned()
    }
}

fn extension(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let last = path.rsplit('/').next().unwrap_or_default();
    last.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| {
            (1..=5).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "bin".to_owned())
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let (b'%', Some(decoded)) = (byte, hex) {
            out.push(decoded);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn not_media() -> Failure {
    Failure::new(FailureKind::Unsupported, "the link holds no audio or video")
}

#[expect(clippy::needless_pass_by_value, reason = "used with map_err")]
pub(crate) fn request_failure(e: ureq::Error) -> Failure {
    let kind = match &e {
        ureq::Error::StatusCode(401 | 403 | 404 | 410) => FailureKind::Unavailable,
        ureq::Error::StatusCode(429) => FailureKind::BotCheck,
        ureq::Error::StatusCode(500..=599)
        | ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Io(_) => FailureKind::Network,
        _ => FailureKind::Internal,
    };
    Failure::new(kind, e.to_string())
}

pub(crate) fn io_failure(e: &io::Error, doing: &str) -> Failure {
    let kind = if e.kind() == io::ErrorKind::StorageFull {
        FailureKind::DiskFull
    } else {
        FailureKind::Internal
    };
    Failure::new(kind, format!("{doing}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{extension, title};

    #[test]
    fn names_from_the_url() {
        assert_eq!(title("/media/clip.mp4"), "clip");
        assert_eq!(
            title("/a/%D0%B7%D0%B0%D0%B1%D0%B5%D0%B3%201936.mpg"),
            "забег 1936"
        );
        assert_eq!(title("/"), "video");
        assert_eq!(extension("https://example.org/media/clip.MP4?x=1"), "mp4");
        assert_eq!(extension("https://example.org/stream"), "bin");
        assert_eq!(extension("https://example.org/a.b/c.toolong"), "bin");
    }
}
