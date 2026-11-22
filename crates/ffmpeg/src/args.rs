use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::Duration;

use pastegrab_domain::media::AudioCodec;
use pastegrab_domain::plan::{AudioAction, Convert, Trim, VideoAction};
use pastegrab_domain::profile::Container;

const ENTRIES: &str = "format=format_name:stream=codec_type,codec_name,height,avg_frame_rate,\
                       field_order:stream_disposition=attached_pic";

pub(crate) fn probe(file: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-v", "error", "-print_format", "json"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.extend([
        "-show_entries".into(),
        ENTRIES.into(),
        "-i".into(),
        file_url(file),
    ]);
    args
}

pub(crate) fn convert(
    input: &Path,
    output: &Path,
    convert: &Convert,
    preset: &str,
) -> Vec<OsString> {
    let mut args = Args::default();
    args.flags(&["-hide_banner", "-nostdin", "-nostats", "-y"])
        .pair("-loglevel", "error")
        .pair("-progress", "pipe:1")
        .pair("-stats_period", "0.5");
    if let Some(Trim { start, .. }) = convert.trim {
        args.pair("-ss", seconds(start));
    }
    args.pair("-i", file_url(input));
    if let Some(Trim { length, .. }) = convert.trim {
        args.pair("-t", seconds(length));
    }
    match convert.video {
        VideoAction::Drop => args.flags(&["-vn"]),
        VideoAction::Copy => args.pair("-map", "0:V:0").pair("-c:v", "copy"),
        VideoAction::Encode(encode) => {
            args.pair("-map", "0:V:0")
                .pair("-c:v", "libx264")
                .pair("-preset", preset)
                .flags(&["-crf", "23", "-pix_fmt", "yuv420p"]);
            if encode.deinterlace {
                args.pair("-vf", "bwdif=mode=send_field");
            }
            if encode.keyframe_every_second {
                args.pair("-force_key_frames", "expr:gte(t,n_forced*1)");
            }
            &mut args
        }
    };
    match convert.audio {
        AudioAction::Drop => args.flags(&["-an"]),
        AudioAction::Copy => args.pair("-map", "0:a:0").pair("-c:a", "copy"),
        AudioAction::Encode(AudioCodec::Mp3) => args
            .pair("-map", "0:a:0")
            .pair("-c:a", "libmp3lame")
            .pair("-q:a", "2"),
        AudioAction::Encode(_) => args
            .pair("-map", "0:a:0")
            .pair("-c:a", "aac")
            .pair("-b:a", "160k"),
    };
    args.pair("-f", muxer(convert.container))
        .0
        .push(file_url(output));
    args.0
}

#[derive(Default)]
struct Args(Vec<OsString>);

impl Args {
    fn flags(&mut self, flags: &[&str]) -> &mut Self {
        self.0.extend(flags.iter().map(OsString::from));
        self
    }

    fn pair(&mut self, flag: &str, value: impl AsRef<OsStr>) -> &mut Self {
        self.0.push(flag.into());
        self.0.push(value.as_ref().to_owned());
        self
    }
}

/// A name starting with `-` or holding a `:` would otherwise read as an option or a protocol.
fn file_url(path: &Path) -> OsString {
    let mut url = OsString::from("file:");
    url.push(path);
    url
}

const fn muxer(container: Container) -> &'static str {
    match container {
        Container::Mp4 => "mp4",
        Container::M4a => "ipod",
        Container::Mkv => "matroska",
        Container::Mp3 => "mp3",
    }
}

fn seconds(at: Duration) -> String {
    format!("{}.{:03}", at.as_secs(), at.subsec_millis())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use pastegrab_domain::media::{AudioCodec, VideoCodec};
    use pastegrab_domain::plan::{AudioAction, Convert, Trim, VideoAction, VideoEncode};
    use pastegrab_domain::profile::Container;

    use super::convert;

    fn line(c: &Convert) -> String {
        line_at(c, "veryfast")
    }

    fn line_at(c: &Convert, preset: &str) -> String {
        let args = convert(Path::new("/w/in.mkv"), Path::new("/w/out.mp4"), c, preset);
        let text: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        text.join(" ")
    }

    const HEAD: &str = "-hide_banner -nostdin -nostats -y -loglevel error -progress pipe:1 \
                        -stats_period 0.5";

    #[test]
    fn a_remux_copies_both_streams() {
        let remux = Convert {
            container: Container::Mp4,
            video: VideoAction::Copy,
            audio: AudioAction::Copy,
            trim: None,
        };
        assert_eq!(
            line(&remux),
            format!(
                "{HEAD} -i file:/w/in.mkv -map 0:V:0 -c:v copy -map 0:a:0 -c:a copy -f mp4 \
                 file:/w/out.mp4"
            )
        );
    }

    #[test]
    fn an_analysis_cut_deinterlaces_with_a_keyframe_every_second() {
        let analysis = Convert {
            container: Container::Mp4,
            video: VideoAction::Encode(VideoEncode {
                codec: VideoCodec::H264,
                deinterlace: true,
                keyframe_every_second: true,
            }),
            audio: AudioAction::Encode(AudioCodec::Aac),
            trim: Some(Trim {
                start: Duration::from_millis(4_990_250),
                length: Duration::from_secs(30),
            }),
        };
        assert_eq!(
            line(&analysis),
            format!(
                "{HEAD} -ss 4990.250 -i file:/w/in.mkv -t 30.000 -map 0:V:0 -c:v libx264 \
                 -preset veryfast -crf 23 -pix_fmt yuv420p -vf bwdif=mode=send_field \
                 -force_key_frames expr:gte(t,n_forced*1) -map 0:a:0 -c:a aac -b:a 160k -f mp4 \
                 file:/w/out.mp4"
            )
        );
    }

    #[test]
    fn an_encode_takes_the_preset_setup_picked() {
        let encode = Convert {
            container: Container::Mp4,
            video: VideoAction::Encode(VideoEncode {
                codec: VideoCodec::H264,
                deinterlace: false,
                keyframe_every_second: false,
            }),
            audio: AudioAction::Copy,
            trim: None,
        };
        assert!(line_at(&encode, "superfast").contains(" -preset superfast -crf 23 "));
    }

    #[test]
    fn mp3_drops_the_picture() {
        let mp3 = Convert {
            container: Container::Mp3,
            video: VideoAction::Drop,
            audio: AudioAction::Encode(AudioCodec::Mp3),
            trim: None,
        };
        assert_eq!(
            line(&mp3),
            format!(
                "{HEAD} -i file:/w/in.mkv -vn -map 0:a:0 -c:a libmp3lame -q:a 2 -f mp3 \
                 file:/w/out.mp4"
            )
        );
    }
}
