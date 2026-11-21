//! The format planner, in two pure steps.
//!
//! Before the download, [`plan_fetch`] picks yt-dlp's formats so the file needs as little
//! conversion as possible. After it, [`plan_convert`] reads what ffprobe found in the real file and
//! decides per stream: copy, encode or drop. yt-dlp's list never says whether a video is
//! interlaced, and only the file itself does.

use std::cmp::Reverse;
use std::fmt;
use std::time::Duration;

use crate::media::{AudioCodec, AudioStream, FileProbe, Format, MediaInfo, ProbedVideo};
use crate::media::{VideoCodec, VideoStream};
use crate::profile::{AudioGoal, Container, HeightCap, Profile, Rank, VideoGoal};

/// Facts about the PC that change the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Machine {
    /// Height of the primary screen in pixels.
    pub screen_height: u32,
}

/// A part of the source, such as one race out of a long replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Section {
    start: Duration,
    end: Duration,
}

impl Section {
    /// The part from `start` to `end`; `None` unless `end` comes after `start`.
    #[must_use]
    pub fn new(start: Duration, end: Duration) -> Option<Self> {
        (end > start).then_some(Self { start, end })
    }

    /// Where the part starts in the source.
    #[must_use]
    pub const fn start(self) -> Duration {
        self.start
    }

    /// Where the part ends in the source.
    #[must_use]
    pub const fn end(self) -> Duration {
        self.end
    }

    /// How long the part is.
    #[must_use]
    pub fn length(self) -> Duration {
        self.end.saturating_sub(self.start)
    }
}

/// The exact cut the convert step makes, measured in the fetched file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Trim {
    /// Where the kept part starts.
    pub start: Duration,
    /// How long the kept part is.
    pub length: Duration,
}

/// What yt-dlp downloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchPlan {
    /// Exact format ids; yt-dlp gets them joined with `+`.
    pub format_ids: Vec<String>,
    /// The container yt-dlp merges a separate video and audio into.
    pub merge_into: Option<Container>,
    /// The part to download, when only part of the source is wanted.
    pub section: Option<Section>,
    /// Bytes expected on the wire, when every picked format reports a size or a bitrate.
    pub expected_bytes: Option<u64>,
    /// The video will need encoding, which takes minutes on an old PC; the UI warns about it.
    pub encodes_video: bool,
}

impl FetchPlan {
    /// The value for yt-dlp's `-f`, such as `137+140`.
    #[must_use]
    pub fn selector(&self) -> String {
        self.format_ids.join("+")
    }
}

/// Why no fetch plan exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanError {
    /// Nothing is left after dropping DRM, storyboards and formats pastegrab cannot fetch.
    NoUsableFormat,
    /// The profile wants a video and the source has none.
    NoVideo,
    /// The profile wants sound and the source has none.
    NoAudio,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoUsableFormat => "no usable format",
            Self::NoVideo => "the source has no video",
            Self::NoAudio => "the source has no sound",
        })
    }
}

impl std::error::Error for PlanError {}

/// What happens to the video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoAction {
    /// Kept as it is.
    Copy,
    /// Encoded again.
    Encode(VideoEncode),
    /// Left out, or absent from the start.
    Drop,
}

/// How the video gets encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VideoEncode {
    /// The codec to encode into.
    pub codec: VideoCodec,
    /// Deinterlace at field rate: 25 interlaced frames a second become 50 progressive ones.
    pub deinterlace: bool,
    /// A keyframe at least every second, so scrubbing lands on exact frames.
    pub keyframe_every_second: bool,
}

/// What happens to the audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioAction {
    /// Kept as it is.
    Copy,
    /// Encoded into this codec.
    Encode(AudioCodec),
    /// Left out, or absent from the start.
    Drop,
}

/// The convert step, one ffmpeg run over the fetched file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Convert {
    /// The container of the finished file.
    pub container: Container,
    /// What happens to the video.
    pub video: VideoAction,
    /// What happens to the audio.
    pub audio: AudioAction,
    /// The exact cut, when a part was asked for.
    pub trim: Option<Trim>,
}

/// The standard heights a screen-sized cap picks from.
const LADDER: [u32; 6] = [360, 480, 720, 1080, 1440, 2160];

/// Picks the formats yt-dlp downloads for `profile`.
///
/// Among videos under the height cap, a matching codec outranks height and frame rate, so a
/// conversion happens only when the site has no matching codec at all. The analysis profile
/// ranks frame rate first instead. Ties go to the codec that is cheaper to decode, then to the
/// smaller file.
///
/// # Errors
///
/// [`PlanError`] when the source has no usable format, or none with the video or sound the
/// profile needs.
pub fn plan_fetch(
    info: &MediaInfo,
    profile: &Profile,
    machine: Machine,
    section: Option<Section>,
) -> Result<FetchPlan, PlanError> {
    let usable: Vec<&Format> = info.formats.iter().filter(|f| f.is_usable()).collect();
    if usable.is_empty() {
        return Err(PlanError::NoUsableFormat);
    }
    let duration = info.duration;
    let picked = match profile.video {
        Some(goal) => {
            let cap = height_cap(goal.cap, machine);
            let video = pick_video(&usable, goal, cap, duration).ok_or(PlanError::NoVideo)?;
            if video.audio.is_some() {
                vec![video]
            } else if let Some(audio) = pick_audio(&usable, profile.audio, duration, false) {
                vec![video, audio]
            } else {
                // Without an audio-only format a muxed one keeps the sound; a silent source stays silent.
                let muxed: Vec<&Format> = usable
                    .iter()
                    .copied()
                    .filter(|f| f.audio.is_some())
                    .collect();
                vec![pick_video(&muxed, goal, cap, duration).unwrap_or(video)]
            }
        }
        None => vec![pick_audio(&usable, profile.audio, duration, true).ok_or(PlanError::NoAudio)?],
    };
    let expected_bytes = picked
        .iter()
        .try_fold(0_u64, |sum, f| {
            sum.checked_add(f.estimated_bytes(duration)?)
        })
        .map(|bytes| share_of(bytes, duration, section));
    Ok(FetchPlan {
        format_ids: picked.iter().map(|f| f.id.clone()).collect(),
        merge_into: merge_container(&picked, profile.container),
        section,
        expected_bytes,
        encodes_video: profile.video.is_some()
            && (section.is_some() || video_mismatch(&picked, profile)),
    })
}

/// Decides the convert step from what ffprobe found in the fetched file.
///
/// `None` means the file is already right and only needs its final name.
#[must_use]
pub fn plan_convert(probe: &FileProbe, profile: &Profile, trim: Option<Trim>) -> Option<Convert> {
    let container = profile.container;
    let video = match (profile.video, probe.video) {
        (Some(goal), Some(found)) => video_action(goal, found, container, trim.is_some()),
        _ => VideoAction::Drop,
    };
    let audio = probe.audio.map_or(AudioAction::Drop, |found| {
        audio_action(profile.audio, found, container)
    });
    let drops_a_stream = (video == VideoAction::Drop && probe.video.is_some())
        || (audio == AudioAction::Drop && probe.audio.is_some());
    let encodes =
        matches!(video, VideoAction::Encode(_)) || matches!(audio, AudioAction::Encode(_));
    let ready = probe.family == container.family() && !drops_a_stream && !encodes && trim.is_none();
    (!ready).then_some(Convert {
        container,
        video,
        audio,
        trim,
    })
}

fn height_cap(cap: HeightCap, machine: Machine) -> u32 {
    match cap {
        HeightCap::Screen => LADDER
            .iter()
            .copied()
            .rev()
            .find(|&height| height <= machine.screen_height)
            .unwrap_or(LADDER[0]),
        HeightCap::AtMost(height) => height,
        HeightCap::Unlimited => u32::MAX,
    }
}

fn pick_video<'a>(
    formats: &[&'a Format],
    goal: VideoGoal,
    cap: u32,
    duration: Option<u32>,
) -> Option<&'a Format> {
    let candidates: Vec<(&'a Format, VideoStream)> = formats
        .iter()
        .filter_map(|&f| f.video.map(|v| (f, v)))
        .collect();
    let fits = |v: &VideoStream| v.height.is_none_or(|height| height <= cap);
    if candidates.iter().any(|(_, v)| fits(v)) {
        candidates
            .into_iter()
            .filter(|(_, v)| fits(v))
            .max_by_key(|&(f, v)| video_key(f, v, goal, duration))
            .map(|(f, _)| f)
    } else {
        // Every video is taller than the cap; the smallest of them beats no video at all.
        candidates
            .into_iter()
            .min_by_key(|(_, v)| v.height)
            .map(|(f, _)| f)
    }
}

fn video_key(
    f: &Format,
    v: VideoStream,
    goal: VideoGoal,
    duration: Option<u32>,
) -> (u32, u32, u32, u32, Reverse<u64>) {
    let codec = u32::from(goal.codec.is_none_or(|c| c == v.codec));
    let height = v.height.unwrap_or(0);
    let fps = u32::from(v.fps.unwrap_or(0));
    let ease = v.codec.decode_ease();
    let size = Reverse(f.estimated_bytes(duration).unwrap_or(u64::MAX));
    match goal.rank {
        Rank::Height => (codec, height, fps, ease, size),
        Rank::FrameRate => (fps, codec, height, ease, size),
    }
}

/// The best audio-only format, or with `muxed_too`, the best format with sound of any kind.
fn pick_audio<'a>(
    formats: &[&'a Format],
    goal: AudioGoal,
    duration: Option<u32>,
    muxed_too: bool,
) -> Option<&'a Format> {
    let best = |audio_only: bool| {
        formats
            .iter()
            .filter(|f| !audio_only || f.video.is_none())
            .filter_map(|&f| f.audio.map(|a| (f, a)))
            .max_by_key(|&(f, a)| audio_key(f, a, goal, duration))
            .map(|(f, _)| f)
    };
    best(true).or_else(|| if muxed_too { best(false) } else { None })
}

fn audio_key(
    f: &Format,
    a: AudioStream,
    goal: AudioGoal,
    duration: Option<u32>,
) -> (u32, u32, Reverse<u64>) {
    let codec = u32::from(goal.codec.is_none_or(|c| c == a.codec));
    let bitrate = a.bitrate.unwrap_or(0);
    let size = Reverse(f.estimated_bytes(duration).unwrap_or(u64::MAX));
    (codec, bitrate, size)
}

/// yt-dlp merges into the profile's container when it takes both codecs, and into MKV otherwise.
fn merge_container(picked: &[&Format], target: Container) -> Option<Container> {
    let [video, audio] = picked else {
        return None;
    };
    let fits = video.video.is_some_and(|v| target.accepts_video(v.codec))
        && audio.audio.is_some_and(|a| target.accepts_audio(a.codec));
    Some(if fits { target } else { Container::Mkv })
}

fn video_mismatch(picked: &[&Format], profile: &Profile) -> bool {
    let Some(wanted) = profile.video.and_then(|goal| goal.codec) else {
        return false;
    };
    picked
        .iter()
        .filter_map(|f| f.video)
        .any(|v| v.codec != wanted)
}

/// The share of `bytes` a section takes, assuming an even bitrate.
fn share_of(bytes: u64, duration: Option<u32>, section: Option<Section>) -> u64 {
    match (duration, section) {
        (Some(total), Some(section)) if total > 0 => {
            let part = section.length().as_secs().min(u64::from(total));
            let share = u128::from(bytes) * u128::from(part) / u128::from(total);
            u64::try_from(share).unwrap_or(u64::MAX)
        }
        _ => bytes,
    }
}

fn video_action(
    goal: VideoGoal,
    found: ProbedVideo,
    container: Container,
    cutting: bool,
) -> VideoAction {
    let source = found.stream.codec;
    let keeps_codec = goal.codec.is_none_or(|c| c == source) && container.accepts_video(source);
    let must_encode = !keeps_codec || cutting || (found.interlaced && goal.analysis);
    if !must_encode {
        return VideoAction::Copy;
    }
    // Any other target would mean libvpx or libaom encoding on an old CPU.
    let codec = goal
        .codec
        .filter(|&c| container.accepts_video(c))
        .unwrap_or(VideoCodec::H264);
    VideoAction::Encode(VideoEncode {
        codec,
        deinterlace: found.interlaced,
        keyframe_every_second: goal.analysis,
    })
}

fn audio_action(goal: AudioGoal, found: AudioCodec, container: Container) -> AudioAction {
    if goal.codec.is_none_or(|c| c == found) && container.accepts_audio(found) {
        return AudioAction::Copy;
    }
    let codec = goal
        .codec
        .filter(|&c| container.accepts_audio(c))
        .unwrap_or(AudioCodec::Aac);
    AudioAction::Encode(codec)
}
