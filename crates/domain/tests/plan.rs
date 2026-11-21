//! The planner against format lists shaped like the sites it meets, and against ffprobe results.

// Fixture helpers outside #[test] fns fall outside clippy's test exemption.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use pastegrab_domain::media::{
    AudioCodec, AudioStream, Family, FileProbe, Format, MediaInfo, ProbedVideo, Protocol,
    VideoCodec, VideoStream,
};
use pastegrab_domain::plan::{
    AudioAction, Convert, FetchPlan, Machine, PlanError, Section, Trim, VideoAction, VideoEncode,
    plan_convert, plan_fetch,
};
use pastegrab_domain::profile::{Container, ProfileKind};

const MB: u64 = 1_000_000;

fn video(id: &str, codec: &str, height: u32, fps: u16, mb: u64) -> Format {
    Format {
        id: id.into(),
        video: Some(VideoStream {
            codec: VideoCodec::parse(codec).unwrap(),
            height: Some(height),
            fps: Some(fps),
        }),
        audio: None,
        size: Some(mb * MB),
        bitrate: None,
        protocol: Protocol::Dash,
        drm: false,
    }
}

fn audio(id: &str, codec: &str, kbps: u32, mb: u64) -> Format {
    Format {
        id: id.into(),
        video: None,
        audio: Some(AudioStream {
            codec: AudioCodec::parse(codec).unwrap(),
            bitrate: Some(kbps),
        }),
        size: Some(mb * MB),
        bitrate: None,
        protocol: Protocol::Dash,
        drm: false,
    }
}

fn muxed(id: &str, height: u32, fps: u16, audio_kbps: u32, total_kbps: u32) -> Format {
    Format {
        id: id.into(),
        video: Some(VideoStream {
            codec: VideoCodec::H264,
            height: Some(height),
            fps: Some(fps),
        }),
        audio: Some(AudioStream {
            codec: AudioCodec::Aac,
            bitrate: Some(audio_kbps),
        }),
        size: None,
        bitrate: Some(total_kbps),
        protocol: Protocol::Hls,
        drm: false,
    }
}

/// YouTube's format table for a ten-minute video, storyboard included.
fn youtube() -> MediaInfo {
    let storyboard = Format {
        id: "sb0".into(),
        video: None,
        audio: None,
        size: None,
        bitrate: None,
        protocol: Protocol::Unsupported,
        drm: false,
    };
    let mut progressive = video("18", "avc1.42001E", 360, 30, 20);
    progressive.audio = Some(AudioStream {
        codec: AudioCodec::Aac,
        bitrate: Some(96),
    });
    progressive.protocol = Protocol::Http;
    MediaInfo {
        title: "1996 final".into(),
        duration: Some(600),
        is_live: false,
        formats: vec![
            storyboard,
            audio("139", "mp4a.40.5", 48, 3),
            audio("140", "mp4a.40.2", 129, 10),
            audio("249", "opus", 50, 4),
            audio("251", "opus", 135, 10),
            video("160", "avc1.4d400c", 144, 30, 2),
            video("133", "avc1.4d4015", 240, 30, 4),
            video("134", "avc1.4d401e", 360, 30, 8),
            video("135", "avc1.4d401f", 480, 30, 14),
            video("136", "avc1.64001f", 720, 30, 27),
            video("298", "avc1.640020", 720, 60, 40),
            video("137", "avc1.640028", 1080, 30, 50),
            video("299", "avc1.64002a", 1080, 60, 75),
            video("247", "vp9", 720, 30, 20),
            video("302", "vp9", 720, 60, 30),
            video("248", "vp9", 1080, 30, 38),
            video("303", "vp9", 1080, 60, 55),
            video("308", "vp9", 1440, 60, 110),
            video("315", "vp9", 2160, 60, 260),
            video("398", "av01.0.05M.08", 720, 60, 22),
            video("399", "av01.0.08M.08", 1080, 60, 40),
            video("400", "av01.0.12M.08", 1440, 60, 90),
            video("401", "av01.0.12M.08", 2160, 60, 200),
            progressive,
        ],
    }
}

/// An HLS-only site such as olympics.com: muxed variants of a two-hour broadcast.
fn broadcast() -> MediaInfo {
    MediaInfo {
        title: "Atlanta 1996, canoe sprint".into(),
        duration: Some(7200),
        is_live: false,
        formats: vec![
            muxed("hls-360", 360, 25, 64, 800),
            muxed("hls-720", 720, 50, 128, 2500),
            muxed("hls-1080", 1080, 50, 128, 4500),
        ],
    }
}

/// An old archive item: the MPEG-2 original and a small H.264 derivative.
fn archive() -> MediaInfo {
    let mut original = video("original", "mpeg2video", 576, 25, 700);
    original.audio = Some(AudioStream {
        codec: AudioCodec::Mp2,
        bitrate: Some(224),
    });
    original.protocol = Protocol::Http;
    let mut derivative = video("mp4", "h264", 480, 25, 120);
    derivative.audio = Some(AudioStream {
        codec: AudioCodec::Aac,
        bitrate: Some(128),
    });
    derivative.protocol = Protocol::Http;
    MediaInfo {
        title: "1980 final".into(),
        duration: Some(900),
        is_live: false,
        formats: vec![original, derivative],
    }
}

fn plan(info: &MediaInfo, kind: ProfileKind, screen_height: u32) -> FetchPlan {
    plan_fetch(info, &kind.profile(), Machine { screen_height }, None).unwrap()
}

fn ids(plan: &FetchPlan) -> Vec<&str> {
    plan.format_ids.iter().map(String::as_str).collect()
}

#[test]
fn video_on_a_768_screen_takes_720p60_h264_with_aac() {
    let plan = plan(&youtube(), ProfileKind::Video, 768);
    assert_eq!(ids(&plan), ["298", "140"]);
    assert_eq!(plan.selector(), "298+140");
    assert_eq!(plan.merge_into, Some(Container::Mp4));
    assert_eq!(plan.expected_bytes, Some(50 * MB));
    assert!(!plan.encodes_video);
}

#[test]
fn video_on_a_1080_screen_takes_1080p60() {
    assert_eq!(
        ids(&plan(&youtube(), ProfileKind::Video, 1080)),
        ["299", "140"]
    );
}

#[test]
fn matching_codec_outranks_height() {
    // 1440p exists only as VP9 and AV1; 1080p H.264 needs no conversion.
    assert_eq!(
        ids(&plan(&youtube(), ProfileKind::Video, 1440)),
        ["299", "140"]
    );
}

#[test]
fn best_quality_prefers_the_codec_cheaper_to_decode() {
    let plan = plan(&youtube(), ProfileKind::BestQuality, 768);
    assert_eq!(ids(&plan), ["315", "251"]);
    assert_eq!(plan.merge_into, Some(Container::Mkv));
}

#[test]
fn small_stops_at_480p() {
    assert_eq!(
        ids(&plan(&youtube(), ProfileKind::Small, 1080)),
        ["135", "140"]
    );
}

#[test]
fn sound_takes_aac_alone() {
    let plan = plan(&youtube(), ProfileKind::Audio, 768);
    assert_eq!(ids(&plan), ["140"]);
    assert_eq!(plan.merge_into, None);
}

#[test]
fn mp3_takes_the_best_source_to_encode_from() {
    let plan = plan(&youtube(), ProfileKind::AudioMp3, 768);
    assert_eq!(ids(&plan), ["251"]);
    assert!(!plan.encodes_video);
}

#[test]
fn analysis_takes_the_highest_frame_rate() {
    assert_eq!(
        ids(&plan(&youtube(), ProfileKind::Analysis, 768)),
        ["299", "140"]
    );
}

#[test]
fn analysis_prefers_60_fps_vp9_over_30_fps_h264() {
    let mut info = youtube();
    info.formats.retain(|f| f.id != "298" && f.id != "299");
    let plan = plan(&info, ProfileKind::Analysis, 768);
    assert_eq!(ids(&plan), ["303", "140"]);
    assert!(plan.encodes_video);
}

#[test]
fn hls_variants_carry_their_own_sound() {
    let plan = plan(&broadcast(), ProfileKind::Video, 768);
    assert_eq!(ids(&plan), ["hls-720"]);
    assert_eq!(plan.merge_into, None);
    assert_eq!(plan.expected_bytes, Some(2_250_000_000));
}

#[test]
fn a_fragment_downloads_its_share_and_gets_encoded() {
    let race = Section::new(Duration::from_secs(4990), Duration::from_secs(5020)).unwrap();
    let plan = plan_fetch(
        &broadcast(),
        &ProfileKind::Analysis.profile(),
        Machine { screen_height: 768 },
        Some(race),
    )
    .unwrap();
    assert_eq!(ids(&plan), ["hls-1080"]);
    assert_eq!(plan.section, Some(race));
    assert_eq!(plan.expected_bytes, Some(16_875_000));
    assert!(plan.encodes_video);
}

#[test]
fn sound_from_a_muxed_only_site_takes_the_smallest_good_variant() {
    assert_eq!(
        ids(&plan(&broadcast(), ProfileKind::Audio, 768)),
        ["hls-720"]
    );
}

#[test]
fn archive_footage_prefers_h264_unless_quality_is_asked_for() {
    assert_eq!(ids(&plan(&archive(), ProfileKind::Video, 768)), ["mp4"]);
    assert_eq!(
        ids(&plan(&archive(), ProfileKind::BestQuality, 768)),
        ["original"]
    );
}

#[test]
fn drm_and_storyboards_leave_nothing() {
    let mut info = youtube();
    for f in &mut info.formats {
        f.drm = true;
    }
    let machine = Machine { screen_height: 768 };
    let profile = ProfileKind::Video.profile();
    assert_eq!(
        plan_fetch(&info, &profile, machine, None),
        Err(PlanError::NoUsableFormat)
    );
}

#[test]
fn a_sound_only_source_has_no_video_plan() {
    let info = MediaInfo {
        title: "track".into(),
        duration: Some(200),
        is_live: false,
        formats: vec![audio("mp3", "mp3", 128, 3)],
    };
    let machine = Machine { screen_height: 768 };
    assert_eq!(
        plan_fetch(&info, &ProfileKind::Video.profile(), machine, None),
        Err(PlanError::NoVideo)
    );
    assert_eq!(ids(&plan(&info, ProfileKind::Audio, 768)), ["mp3"]);
}

fn source(formats: Vec<Format>) -> MediaInfo {
    MediaInfo {
        title: "clip".into(),
        duration: Some(60),
        is_live: false,
        formats,
    }
}

#[test]
fn without_audio_only_formats_a_muxed_one_keeps_the_sound() {
    let info = source(vec![
        video("dash-720", "avc1.64001f", 720, 30, 20),
        muxed("hls-360", 360, 30, 96, 800),
    ]);
    assert_eq!(ids(&plan(&info, ProfileKind::Video, 768)), ["hls-360"]);
}

#[test]
fn a_silent_source_stays_a_silent_video() {
    let info = source(vec![video("v-480", "avc1.4d401e", 480, 25, 8)]);
    assert_eq!(ids(&plan(&info, ProfileKind::Video, 768)), ["v-480"]);
}

#[test]
fn hls_variants_without_codecs_still_plan() {
    let variant = |id: &str, height: u32, kbps: u32| Format {
        id: id.into(),
        video: Some(VideoStream {
            codec: VideoCodec::Unknown,
            height: Some(height),
            fps: None,
        }),
        audio: Some(AudioStream {
            codec: AudioCodec::Unknown,
            bitrate: None,
        }),
        size: None,
        bitrate: Some(kbps),
        protocol: Protocol::Hls,
        drm: false,
    };
    let info = source(vec![
        variant("hls-360", 360, 800),
        variant("hls-720", 720, 2500),
        variant("hls-1080", 1080, 5000),
    ]);
    let fetch = plan(&info, ProfileKind::Video, 768);
    assert_eq!(ids(&fetch), ["hls-720"]);
    assert_eq!(fetch.merge_into, None);
}

#[test]
fn a_section_must_end_after_it_starts() {
    let at = Duration::from_secs;
    assert_eq!(Section::new(at(40), at(10)), None);
    assert_eq!(Section::new(at(10), at(10)), None);
    assert_eq!(
        Section::new(at(10), at(40)).map(Section::length),
        Some(at(30))
    );
}

fn probe(family: Family, video: Option<(&str, bool)>, audio: Option<&str>) -> FileProbe {
    FileProbe {
        family,
        video: video.map(|(codec, interlaced)| ProbedVideo {
            stream: VideoStream {
                codec: VideoCodec::parse(codec).unwrap(),
                height: Some(720),
                fps: Some(25),
            },
            interlaced,
        }),
        audio: audio.map(|codec| AudioCodec::parse(codec).unwrap()),
    }
}

fn convert(probe: &FileProbe, kind: ProfileKind, trim: Option<Trim>) -> Option<Convert> {
    plan_convert(probe, &kind.profile(), trim)
}

const fn h264(deinterlace: bool, keyframe_every_second: bool) -> VideoAction {
    VideoAction::Encode(VideoEncode {
        codec: VideoCodec::H264,
        deinterlace,
        keyframe_every_second,
    })
}

const HALF_MINUTE: Trim = Trim {
    start: Duration::ZERO,
    length: Duration::from_secs(30),
};

#[test]
fn a_ready_mp4_needs_nothing() {
    let file = probe(Family::IsoBmff, Some(("h264", false)), Some("aac"));
    assert_eq!(convert(&file, ProfileKind::Video, None), None);
}

#[test]
fn a_merged_mkv_is_only_remuxed() {
    let file = probe(Family::Matroska, Some(("h264", false)), Some("aac"));
    let expected = Convert {
        container: Container::Mp4,
        video: VideoAction::Copy,
        audio: AudioAction::Copy,
        trim: None,
    };
    assert_eq!(convert(&file, ProfileKind::Video, None), Some(expected));
}

#[test]
fn vp9_with_opus_is_encoded_for_mp4() {
    let file = probe(Family::Matroska, Some(("vp9", false)), Some("opus"));
    let expected = Convert {
        container: Container::Mp4,
        video: h264(false, false),
        audio: AudioAction::Encode(AudioCodec::Aac),
        trim: None,
    };
    assert_eq!(convert(&file, ProfileKind::Video, None), Some(expected));
}

#[test]
fn interlaced_mpeg2_is_deinterlaced_while_encoding() {
    let file = probe(Family::Other, Some(("mpeg2video", true)), Some("mp2"));
    let plan = convert(&file, ProfileKind::Video, None).unwrap();
    assert_eq!(plan.video, h264(true, false));
    assert_eq!(plan.audio, AudioAction::Encode(AudioCodec::Aac));
}

#[test]
fn analysis_leaves_progressive_h264_alone() {
    let file = probe(Family::IsoBmff, Some(("h264", false)), Some("aac"));
    assert_eq!(convert(&file, ProfileKind::Analysis, None), None);
}

#[test]
fn analysis_deinterlaces_even_h264() {
    let file = probe(Family::IsoBmff, Some(("h264", true)), Some("aac"));
    let plan = convert(&file, ProfileKind::Analysis, None).unwrap();
    assert_eq!(plan.video, h264(true, true));
    assert_eq!(plan.audio, AudioAction::Copy);
}

#[test]
fn a_fragment_is_cut_exactly_and_keeps_its_sound() {
    let file = probe(Family::IsoBmff, Some(("h264", false)), Some("aac"));
    let plan = convert(&file, ProfileKind::Analysis, Some(HALF_MINUTE)).unwrap();
    assert_eq!(plan.video, h264(false, true));
    assert_eq!(plan.audio, AudioAction::Copy);
    assert_eq!(plan.trim, Some(HALF_MINUTE));
}

#[test]
fn sound_from_a_video_file_drops_the_picture() {
    let file = probe(Family::IsoBmff, Some(("h264", false)), Some("aac"));
    let plan = convert(&file, ProfileKind::Audio, None).unwrap();
    assert_eq!(plan.container, Container::M4a);
    assert_eq!(plan.video, VideoAction::Drop);
    assert_eq!(plan.audio, AudioAction::Copy);
}

#[test]
fn a_ready_m4a_needs_nothing() {
    let file = probe(Family::IsoBmff, None, Some("aac"));
    assert_eq!(convert(&file, ProfileKind::Audio, None), None);
}

#[test]
fn mp3_is_encoded_from_aac() {
    let file = probe(Family::IsoBmff, None, Some("aac"));
    let plan = convert(&file, ProfileKind::AudioMp3, None).unwrap();
    assert_eq!(plan.container, Container::Mp3);
    assert_eq!(plan.audio, AudioAction::Encode(AudioCodec::Mp3));
}

#[test]
fn best_quality_keeps_vp9_in_mkv() {
    let file = probe(Family::Matroska, Some(("vp9", false)), Some("opus"));
    assert_eq!(convert(&file, ProfileKind::BestQuality, None), None);
}

#[test]
fn a_best_quality_fragment_encodes_h264_rather_than_av1() {
    let file = probe(
        Family::Matroska,
        Some(("av01.0.08M.08", false)),
        Some("opus"),
    );
    let plan = convert(&file, ProfileKind::BestQuality, Some(HALF_MINUTE)).unwrap();
    assert_eq!(plan.video, h264(false, false));
    assert_eq!(plan.audio, AudioAction::Copy);
}
