//! What a source offers, as yt-dlp lists it, and what ffprobe finds in a fetched file.

/// Video codecs the planner tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    /// H.264, also called AVC.
    H264,
    /// H.265, also called HEVC.
    Hevc,
    /// VP8.
    Vp8,
    /// VP9.
    Vp9,
    /// AV1.
    Av1,
    /// MPEG-4 Part 2, common in old AVI files.
    Mpeg4,
    /// MPEG-2, as in broadcast and DVD archives.
    Mpeg2,
    /// Anything else.
    Other,
    /// A stream the source lists without naming its codec, as HLS variants without a CODECS
    /// attribute do; ffprobe names it after the fetch.
    Unknown,
}

impl VideoCodec {
    /// Reads an RFC 6381 string such as `avc1.640028` or an ffprobe name such as `h264`.
    /// `none` and an empty string mean there is no video stream.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let base = lower
            .split_once('.')
            .map_or(lower.as_str(), |(base, _)| base);
        let codec = match base {
            "" | "none" => return None,
            "avc1" | "avc3" | "h264" => Self::H264,
            "hev1" | "hvc1" | "hevc" | "h265" => Self::Hevc,
            "vp8" | "vp08" => Self::Vp8,
            "vp9" | "vp09" => Self::Vp9,
            "av01" | "av1" => Self::Av1,
            "mp4v" | "mpeg4" => Self::Mpeg4,
            "mpeg2video" | "mp2v" => Self::Mpeg2,
            _ => Self::Other,
        };
        Some(codec)
    }

    /// How cheap the codec is to decode on an old CPU; higher is cheaper. When a source has to
    /// be converted anyway, the cheaper one wins.
    #[must_use]
    pub const fn decode_ease(self) -> u32 {
        match self {
            Self::H264 | Self::Mpeg4 | Self::Mpeg2 | Self::Vp8 => 3,
            Self::Vp9 | Self::Hevc => 2,
            Self::Av1 => 1,
            Self::Other | Self::Unknown => 0,
        }
    }
}

/// Audio codecs the planner tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    /// AAC.
    Aac,
    /// MP3.
    Mp3,
    /// MPEG-1 Layer II, common in old broadcasts.
    Mp2,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// Dolby Digital.
    Ac3,
    /// Dolby Digital Plus.
    Eac3,
    /// FLAC.
    Flac,
    /// Anything else.
    Other,
    /// A stream the source lists without naming its codec; ffprobe names it after the fetch.
    Unknown,
}

impl AudioCodec {
    /// Reads an RFC 6381 string such as `mp4a.40.2` or an ffprobe name such as `aac`.
    /// `none` and an empty string mean there is no audio stream.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        // MP3 inside MP4 is signalled as an mp4a object type, not by its own name.
        if matches!(lower.as_str(), "mp4a.6b" | "mp4a.69" | "mp4a.40.34") {
            return Some(Self::Mp3);
        }
        let base = lower
            .split_once('.')
            .map_or(lower.as_str(), |(base, _)| base);
        let codec = match base {
            "" | "none" => return None,
            "mp4a" | "aac" => Self::Aac,
            "mp3" => Self::Mp3,
            "mp2" => Self::Mp2,
            "opus" => Self::Opus,
            "vorbis" => Self::Vorbis,
            "ac-3" | "ac3" => Self::Ac3,
            "ec-3" | "eac3" => Self::Eac3,
            "flac" => Self::Flac,
            _ => Self::Other,
        };
        Some(codec)
    }
}

/// How yt-dlp would fetch a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// One HTTP(S) file.
    Http,
    /// HLS segments.
    Hls,
    /// DASH segments.
    Dash,
    /// Anything pastegrab does not fetch, such as storyboard images.
    Unsupported,
}

/// The video stream of a format or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VideoStream {
    /// The codec.
    pub codec: VideoCodec,
    /// Height in pixels, when known.
    pub height: Option<u32>,
    /// Frames per second, rounded: 29.97 counts as 30.
    pub fps: Option<u16>,
}

/// The audio stream of a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioStream {
    /// The codec.
    pub codec: AudioCodec,
    /// Kilobits per second, when known.
    pub bitrate: Option<u32>,
}

/// One entry in yt-dlp's format list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    /// yt-dlp's `format_id`, such as `137`.
    pub id: String,
    /// The video stream, if the format has one.
    pub video: Option<VideoStream>,
    /// The audio stream, if the format has one.
    pub audio: Option<AudioStream>,
    /// Bytes, exact or estimated by yt-dlp.
    pub size: Option<u64>,
    /// Total kilobits per second, used for estimates when the size is unknown.
    pub bitrate: Option<u32>,
    /// How yt-dlp would fetch it.
    pub protocol: Protocol,
    /// DRM-protected; pastegrab never touches these.
    pub drm: bool,
}

impl Format {
    /// A usable format is DRM-free, fetchable and carries video or sound.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        !self.drm
            && self.protocol != Protocol::Unsupported
            && (self.video.is_some() || self.audio.is_some())
    }

    /// Bytes on the wire: the reported size, or the bitrate times the duration.
    #[must_use]
    pub fn estimated_bytes(&self, duration_secs: Option<u32>) -> Option<u64> {
        self.size.or_else(|| {
            let kbps = u64::from(self.bitrate?);
            kbps.checked_mul(125)?
                .checked_mul(u64::from(duration_secs?))
        })
    }
}

/// What yt-dlp reports about one video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    /// The title as the site shows it.
    pub title: String,
    /// Length in seconds, when known.
    pub duration: Option<u32>,
    /// A live stream rather than a recording.
    pub is_live: bool,
    /// Every format the site offers.
    pub formats: Vec<Format>,
}

/// Container families as ffprobe reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// MP4, M4A and MOV.
    IsoBmff,
    /// MKV and `WebM`.
    Matroska,
    /// A bare MP3 stream.
    Mp3,
    /// FLV, MPEG-TS, AVI, MPEG-PS, Ogg and everything else.
    Other,
}

/// The video stream ffprobe found in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProbedVideo {
    /// Codec, height and frame rate.
    pub stream: VideoStream,
    /// ffprobe's `field_order` is `tt`, `bb`, `tb` or `bt`.
    pub interlaced: bool,
}

/// What ffprobe found in a fetched or local file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileProbe {
    /// The container family.
    pub family: Family,
    /// The first video stream, if any.
    pub video: Option<ProbedVideo>,
    /// The codec of the first audio stream, if any.
    pub audio: Option<AudioCodec>,
}

#[cfg(test)]
mod tests {
    use super::{AudioCodec, VideoCodec};

    #[test]
    fn video_codec_names() {
        assert_eq!(VideoCodec::parse("avc1.640028"), Some(VideoCodec::H264));
        assert_eq!(VideoCodec::parse("vp09.00.40.08"), Some(VideoCodec::Vp9));
        assert_eq!(VideoCodec::parse("av01.0.08M.08"), Some(VideoCodec::Av1));
        assert_eq!(VideoCodec::parse("mpeg2video"), Some(VideoCodec::Mpeg2));
        assert_eq!(VideoCodec::parse("none"), None);
        assert_eq!(VideoCodec::parse("theora"), Some(VideoCodec::Other));
    }

    #[test]
    fn audio_codec_names() {
        assert_eq!(AudioCodec::parse("mp4a.40.2"), Some(AudioCodec::Aac));
        assert_eq!(AudioCodec::parse("mp4a.6B"), Some(AudioCodec::Mp3));
        assert_eq!(AudioCodec::parse("opus"), Some(AudioCodec::Opus));
        assert_eq!(AudioCodec::parse("ec-3"), Some(AudioCodec::Eac3));
        assert_eq!(AudioCodec::parse("none"), None);
    }
}
