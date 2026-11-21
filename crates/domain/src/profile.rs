//! Output profiles, the entries in the format list, kept as data.

use crate::media::{AudioCodec, Family, VideoCodec};

/// Output containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Container {
    /// MP4: plays in Windows Media Player with H.264 and AAC inside.
    Mp4,
    /// M4A: MP4 with sound only.
    M4a,
    /// MKV: takes any codec, needs VLC or MPC-HC on Windows 8.
    Mkv,
    /// MP3.
    Mp3,
}

impl Container {
    /// The family ffprobe reports for a file in this container.
    #[must_use]
    pub const fn family(self) -> Family {
        match self {
            Self::Mp4 | Self::M4a => Family::IsoBmff,
            Self::Mkv => Family::Matroska,
            Self::Mp3 => Family::Mp3,
        }
    }

    /// The file name extension, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::M4a => "m4a",
            Self::Mkv => "mkv",
            Self::Mp3 => "mp3",
        }
    }

    /// Whether the container carries this video codec without re-encoding.
    #[must_use]
    pub const fn accepts_video(self, codec: VideoCodec) -> bool {
        match self {
            Self::Mp4 => matches!(
                codec,
                VideoCodec::H264 | VideoCodec::Hevc | VideoCodec::Av1 | VideoCodec::Vp9
            ),
            Self::Mkv => true,
            Self::M4a | Self::Mp3 => false,
        }
    }

    /// Whether the container carries this audio codec without re-encoding.
    #[must_use]
    pub const fn accepts_audio(self, codec: AudioCodec) -> bool {
        match self {
            Self::Mp4 | Self::M4a => matches!(
                codec,
                AudioCodec::Aac
                    | AudioCodec::Mp3
                    | AudioCodec::Opus
                    | AudioCodec::Flac
                    | AudioCodec::Ac3
                    | AudioCodec::Eac3
            ),
            Self::Mkv => true,
            Self::Mp3 => matches!(codec, AudioCodec::Mp3),
        }
    }
}

/// The profiles in the format list, in the order it shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileKind {
    /// «Видео»: MP4 with H.264 and AAC, as tall as the screen.
    Video,
    /// «Видео, лучшее качество»: MKV with the best streams in any codec.
    BestQuality,
    /// «Видео, малый размер»: MP4 with H.264 at 480p at most.
    Small,
    /// «Только звук»: M4A with AAC.
    Audio,
    /// «Только звук, MP3».
    AudioMp3,
    /// «Для анализа»: MP4 with H.264 at the highest frame rate, up to 1080p.
    Analysis,
}

/// How tall a picked video may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeightCap {
    /// The largest standard height that fits the screen.
    Screen,
    /// A fixed limit in pixels.
    AtMost(u32),
    /// No limit.
    Unlimited,
}

/// What wins between two videos that both fit under the cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rank {
    /// The profile's codec first, then height, then frame rate.
    Height,
    /// Frame rate first: conversion keeps the frame rate, and nothing brings lost motion back.
    FrameRate,
}

/// The video half of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VideoGoal {
    /// The codec the file must end in; `None` keeps the source's.
    pub codec: Option<VideoCodec>,
    /// How tall the picked video may be.
    pub cap: HeightCap,
    /// What wins between candidates that fit.
    pub rank: Rank,
    /// Deinterlace at field rate and keep a keyframe at least every second.
    pub analysis: bool,
}

/// The audio half of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioGoal {
    /// The codec the file must end in; `None` keeps the source's.
    pub codec: Option<AudioCodec>,
}

/// One output profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Profile {
    /// Which entry of the format list this is.
    pub kind: ProfileKind,
    /// The container of the finished file.
    pub container: Container,
    /// `None` for sound-only profiles.
    pub video: Option<VideoGoal>,
    /// The audio half.
    pub audio: AudioGoal,
}

impl ProfileKind {
    /// Every profile, in the order the format list shows them.
    pub const ALL: [Self; 6] = [
        Self::Video,
        Self::BestQuality,
        Self::Small,
        Self::Audio,
        Self::AudioMp3,
        Self::Analysis,
    ];

    /// The settings behind the entry.
    #[must_use]
    pub const fn profile(self) -> Profile {
        let (container, video, audio) = match self {
            Self::Video => (
                Container::Mp4,
                Some(video(
                    Some(VideoCodec::H264),
                    HeightCap::Screen,
                    Rank::Height,
                )),
                Some(AudioCodec::Aac),
            ),
            Self::BestQuality => (
                Container::Mkv,
                Some(video(None, HeightCap::Unlimited, Rank::Height)),
                None,
            ),
            Self::Small => (
                Container::Mp4,
                Some(video(
                    Some(VideoCodec::H264),
                    HeightCap::AtMost(480),
                    Rank::Height,
                )),
                Some(AudioCodec::Aac),
            ),
            Self::Audio => (Container::M4a, None, Some(AudioCodec::Aac)),
            Self::AudioMp3 => (Container::Mp3, None, Some(AudioCodec::Mp3)),
            Self::Analysis => (
                Container::Mp4,
                Some(VideoGoal {
                    analysis: true,
                    ..video(
                        Some(VideoCodec::H264),
                        HeightCap::AtMost(1080),
                        Rank::FrameRate,
                    )
                }),
                Some(AudioCodec::Aac),
            ),
        };
        Profile {
            kind: self,
            container,
            video,
            audio: AudioGoal { codec: audio },
        }
    }
}

// A new profile breaks this match until it gets the next number here and its place in `ALL`.
const _: () = {
    const fn position(kind: ProfileKind) -> usize {
        match kind {
            ProfileKind::Video => 0,
            ProfileKind::BestQuality => 1,
            ProfileKind::Small => 2,
            ProfileKind::Audio => 3,
            ProfileKind::AudioMp3 => 4,
            ProfileKind::Analysis => 5,
        }
    }
    let mut i = 0;
    while i < ProfileKind::ALL.len() {
        assert!(position(ProfileKind::ALL[i]) == i);
        i += 1;
    }
};

const fn video(codec: Option<VideoCodec>, cap: HeightCap, rank: Rank) -> VideoGoal {
    VideoGoal {
        codec,
        cap,
        rank,
        analysis: false,
    }
}
