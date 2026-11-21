//! The pure core's hot paths, timed. `cargo bench -p pastegrab-domain` prints the table, and the
//! numbers are the baseline every later optimization has to beat.

use divan::{Bencher, black_box};
use pastegrab_domain::link::analyze;
use pastegrab_domain::media::{
    AudioCodec, AudioStream, Format, MediaInfo, Protocol, VideoCodec, VideoStream,
};
use pastegrab_domain::name::output_name;
use pastegrab_domain::plan::{Machine, plan_fetch};
use pastegrab_domain::profile::ProfileKind;

fn main() {
    divan::main();
}

const PASTE: &str = "Смотри забег: https://youtu.be/AbC123xyz_0?si=Q1w2&t=95 и ещё \
    https://olympics.com/ru/video/slavomir-knazovicky-canoe-sprint-c1-500m-silver-medal-atlanta-1996?uxreference=playlist";

#[divan::bench]
fn analyze_one_paste() -> usize {
    analyze(black_box(PASTE)).len()
}

#[divan::bench]
fn analyze_a_long_chat_paste(b: Bencher) {
    let text = PASTE.repeat(50);
    b.bench(|| analyze(black_box(&text)).len());
}

fn format(id: &str, video: Option<(VideoCodec, u32, u16)>, audio: Option<AudioCodec>) -> Format {
    Format {
        id: id.into(),
        video: video.map(|(codec, height, fps)| VideoStream {
            codec,
            height: Some(height),
            fps: Some(fps),
        }),
        audio: audio.map(|codec| AudioStream {
            codec,
            bitrate: Some(128),
        }),
        size: Some(20_000_000),
        bitrate: None,
        protocol: Protocol::Dash,
        drm: false,
    }
}

/// A YouTube-sized format list: four audio tracks and twenty video formats.
fn youtube() -> MediaInfo {
    let mut formats = vec![
        format("140", None, Some(AudioCodec::Aac)),
        format("139", None, Some(AudioCodec::Aac)),
        format("251", None, Some(AudioCodec::Opus)),
        format("249", None, Some(AudioCodec::Opus)),
    ];
    for (codec, base) in [
        (VideoCodec::H264, 130),
        (VideoCodec::Vp9, 240),
        (VideoCodec::Av1, 390),
    ] {
        for (i, height) in [144, 240, 360, 480, 720, 1080, 1440]
            .into_iter()
            .enumerate()
        {
            let id = (base + i).to_string();
            formats.push(format(&id, Some((codec, height, 30)), None));
        }
    }
    MediaInfo {
        title: "1996 final".into(),
        duration: Some(600),
        is_live: false,
        formats,
    }
}

#[divan::bench]
fn plan_a_youtube_video(b: Bencher) {
    let info = youtube();
    let profile = ProfileKind::Video.profile();
    let machine = Machine { screen_height: 768 };
    b.bench(|| plan_fetch(black_box(&info), &profile, machine, None));
}

#[divan::bench]
fn name_a_russian_title() -> Option<String> {
    output_name(
        black_box("Слава Князовицкий: серебро | Атланта 1996 🏅"),
        "id",
        "mp4",
        200,
        |_| false,
    )
}
