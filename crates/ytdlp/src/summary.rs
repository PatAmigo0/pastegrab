//! The probe's summary JSON, read into the planner's types.

use serde::Deserialize;
use serde_json::{Number, Value};

use pastegrab_domain::media::{
    AudioCodec, AudioStream, Format, MediaInfo, Protocol, VideoCodec, VideoStream,
};

/// `j` escapes everything outside ASCII, which no console code page can damage.
pub(crate) const TEMPLATE: &str = concat!(
    r#"{"id":%(id)j,"title":%(title)j,"duration":%(duration)j,"is_live":%(is_live)j,"#,
    r#""formats":%(formats.:.{format_id,vcodec,acodec,height,fps,tbr,abr,filesize,"#,
    r#"filesize_approx,protocol,has_drm})j}"#,
);

#[derive(Deserialize)]
struct Summary {
    id: Option<String>,
    title: Option<String>,
    duration: Option<Number>,
    is_live: Option<bool>,
    formats: Option<Vec<RawFormat>>,
}

#[derive(Deserialize)]
struct RawFormat {
    format_id: Option<String>,
    vcodec: Option<String>,
    acodec: Option<String>,
    height: Option<Number>,
    fps: Option<Number>,
    tbr: Option<Number>,
    abr: Option<Number>,
    filesize: Option<Number>,
    filesize_approx: Option<Number>,
    protocol: Option<String>,
    // `true`, `false` or `"maybe"`, and yt-dlp downloads the maybes.
    has_drm: Option<Value>,
}

pub(crate) fn parse(text: &[u8]) -> serde_json::Result<MediaInfo> {
    let summary: Summary = serde_json::from_slice(text)?;
    let raw = summary.formats.unwrap_or_default();
    let ids: Vec<&str> = raw.iter().filter_map(|f| f.format_id.as_deref()).collect();
    let formats = raw
        .iter()
        // YouTube's `-drc` twins carry the same sound with compressed dynamics.
        .filter(|f| {
            f.format_id
                .as_deref()
                .and_then(|id| id.strip_suffix("-drc"))
                .is_none_or(|plain| !ids.contains(&plain))
        })
        .filter_map(format)
        .collect();
    let title = summary
        .title
        .or(summary.id)
        .unwrap_or_else(|| "video".to_owned());
    Ok(MediaInfo {
        title,
        duration: summary.duration.as_ref().and_then(whole),
        is_live: summary.is_live.unwrap_or(false),
        formats,
    })
}

fn format(raw: &RawFormat) -> Option<Format> {
    let id = raw.format_id.clone()?;
    let tbr = raw.tbr.as_ref().and_then(whole);
    let abr = raw.abr.as_ref().and_then(whole).filter(|&kbps| kbps > 0);
    // `none` means no stream, and a missing codec is an unknown one.
    let video = match raw.vcodec.as_deref() {
        None => Some(VideoCodec::Unknown),
        Some(name) => VideoCodec::parse(name),
    }
    .map(|codec| VideoStream {
        codec,
        height: raw.height.as_ref().and_then(whole),
        fps: raw
            .fps
            .as_ref()
            .and_then(whole)
            .and_then(|fps| u16::try_from(fps).ok()),
    });
    let audio = match raw.acodec.as_deref() {
        None => Some(AudioCodec::Unknown),
        Some(name) => AudioCodec::parse(name),
    }
    .map(|codec| AudioStream {
        codec,
        bitrate: abr.or(if video.is_none() { tbr } else { None }),
    });
    Some(Format {
        id,
        video,
        audio,
        size: raw
            .filesize
            .as_ref()
            .or(raw.filesize_approx.as_ref())
            .and_then(whole64),
        bitrate: tbr,
        protocol: protocol(raw.protocol.as_deref()),
        drm: raw.has_drm == Some(Value::Bool(true)),
    })
}

fn protocol(name: Option<&str>) -> Protocol {
    match name {
        None | Some("http" | "https") => Protocol::Http,
        Some("m3u8" | "m3u8_native") => Protocol::Hls,
        Some("http_dash_segments" | "http_dash_segments_generator" | "ism" | "f4m") => {
            Protocol::Dash
        }
        Some(_) => Protocol::Unsupported,
    }
}

fn whole(n: &Number) -> Option<u32> {
    whole64(n).and_then(|int| u32::try_from(int).ok())
}

fn whole64(n: &Number) -> Option<u64> {
    const LIMIT: f64 = 18_446_744_073_709_551_616.0;
    if let Some(int) = n.as_u64() {
        return Some(int);
    }
    let rounded = n.as_f64()?.round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the range check before the cast"
    )]
    (0.0..LIMIT).contains(&rounded).then_some(rounded as u64)
}
