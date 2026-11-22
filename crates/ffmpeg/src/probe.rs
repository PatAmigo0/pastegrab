use serde::Deserialize;

use pastegrab_domain::media::{
    AudioCodec, Family, FileProbe, ProbedVideo, VideoCodec, VideoStream,
};

#[derive(Deserialize)]
struct Output {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Format {
    format_name: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    field_order: Option<String>,
    disposition: Option<Disposition>,
}

#[derive(Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

pub(crate) fn parse(json: &[u8]) -> serde_json::Result<FileProbe> {
    let output: Output = serde_json::from_slice(json)?;
    let is = |s: &Stream, kind: &str| s.codec_type.as_deref() == Some(kind);
    let video = output
        .streams
        .iter()
        .filter(|s| is(s, "video"))
        // A cover picture is a video stream to ffprobe.
        .find(|s| s.disposition.as_ref().is_none_or(|d| d.attached_pic == 0))
        .map(|s| ProbedVideo {
            stream: VideoStream {
                codec: s
                    .codec_name
                    .as_deref()
                    .and_then(VideoCodec::parse)
                    .unwrap_or(VideoCodec::Other),
                height: s.height,
                fps: s.avg_frame_rate.as_deref().and_then(frame_rate),
            },
            interlaced: matches!(s.field_order.as_deref(), Some("tt" | "bb" | "tb" | "bt")),
        });
    let audio = output.streams.iter().find(|s| is(s, "audio")).map(|s| {
        s.codec_name
            .as_deref()
            .and_then(AudioCodec::parse)
            .unwrap_or(AudioCodec::Other)
    });
    let names = output
        .format
        .and_then(|f| f.format_name)
        .unwrap_or_default();
    let has = |name: &str| names.split(',').any(|n| n == name);
    let family = if has("mp4") || has("mov") {
        Family::IsoBmff
    } else if has("matroska") || has("webm") {
        Family::Matroska
    } else if has("mp3") {
        Family::Mp3
    } else {
        Family::Other
    };
    Ok(FileProbe {
        family,
        video,
        audio,
    })
}

/// `30000/1001` as 30; `0/0` as unknown.
fn frame_rate(rate: &str) -> Option<u16> {
    let (num, den) = rate.split_once('/')?;
    let (num, den): (u64, u64) = (num.parse().ok()?, den.parse().ok()?);
    let rounded = num.checked_add(den / 2)?.checked_div(den)?;
    u16::try_from(rounded).ok().filter(|&fps| fps > 0)
}

#[cfg(test)]
mod tests {
    use pastegrab_domain::media::{AudioCodec, Family, VideoCodec};

    use super::{frame_rate, parse};

    #[test]
    fn recorded_probes() {
        let mp4 = parse(include_bytes!("../tests/fixtures/h264-aac.json")).expect("json");
        assert_eq!(mp4.family, Family::IsoBmff);
        let video = mp4.video.expect("a video");
        assert_eq!(video.stream.codec, VideoCodec::H264);
        assert_eq!(
            (video.stream.height, video.stream.fps),
            (Some(240), Some(15))
        );
        assert!(!video.interlaced);
        assert_eq!(mp4.audio, Some(AudioCodec::Aac));

        let webm = parse(include_bytes!("../tests/fixtures/vp9-opus.json")).expect("json");
        assert_eq!(webm.family, Family::Matroska);
        assert_eq!(webm.video.map(|v| v.stream.fps), Some(Some(30)));
        assert_eq!(webm.audio, Some(AudioCodec::Opus));

        let mpg = parse(include_bytes!("../tests/fixtures/mpeg2-interlaced.json")).expect("json");
        assert_eq!(mpg.family, Family::Other);
        let video = mpg.video.expect("a video");
        assert_eq!(video.stream.codec, VideoCodec::Mpeg2);
        assert!(video.interlaced);
        assert_eq!(mpg.audio, Some(AudioCodec::Mp2));
    }

    #[test]
    fn a_cover_picture_is_no_video() {
        let json = br#"{"streams":[{"codec_type":"video","codec_name":"mjpeg",
            "disposition":{"attached_pic":1}},{"codec_type":"audio","codec_name":"mp3"}],
            "format":{"format_name":"mp3"}}"#;
        let probe = parse(json).expect("json");
        assert_eq!(probe.video, None);
        assert_eq!(probe.family, Family::Mp3);
    }

    #[test]
    fn frame_rates() {
        assert_eq!(frame_rate("30000/1001"), Some(30));
        assert_eq!(frame_rate("25/1"), Some(25));
        assert_eq!(frame_rate("0/0"), None);
    }
}
