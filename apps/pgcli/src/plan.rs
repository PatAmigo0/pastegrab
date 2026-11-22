//! `--plan`: one probe per link and the planner's pick, without downloading.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use pastegrab_app::Cancel;
use pastegrab_app::ports::Resolver;
use pastegrab_domain::link::{Intent, analyze};
use pastegrab_domain::media::Format;
use pastegrab_domain::plan::{Machine, plan_fetch};
use pastegrab_domain::profile::ProfileKind;

pub(crate) fn run(
    resolver: &dyn Resolver,
    text: &str,
    profile: Option<ProfileKind>,
    machine: Machine,
    data: &Path,
) -> ExitCode {
    let mut out = io::stdout().lock();
    let mut failed = false;
    for (n, link) in analyze(text).into_iter().enumerate() {
        let _ = writeln!(out, "{}", link.url);
        if !resolver.accepts(&link) {
            failed = true;
            let _ = writeln!(out, "  not accepted: {:?}", link.kind);
            continue;
        }
        let job = data.join("jobs").join(format!("plan-{n}"));
        let answer = resolver.resolve(&link, &job, false, &Cancel::default());
        let _ = fs::remove_dir_all(&job);
        let info = match answer {
            Ok(found) => found.info,
            Err(failure) => {
                failed = true;
                let _ = writeln!(out, "  failed, {:?}: {}", failure.kind, failure.detail);
                continue;
            }
        };
        let _ = writeln!(out, "  {:?}, {:?} s", info.title, info.duration);
        for format in &info.formats {
            let _ = writeln!(out, "  {}", describe(format));
        }
        let profile = profile.unwrap_or(match link.intent {
            Intent::Audio => ProfileKind::Audio,
            Intent::Video => ProfileKind::Video,
        });
        match plan_fetch(&info, &profile.profile(), machine, None) {
            Ok(plan) => {
                let _ = writeln!(
                    out,
                    "  {profile:?}: {} into {:?}, {:?} bytes, encodes video: {}",
                    plan.selector(),
                    plan.merge_into,
                    plan.expected_bytes,
                    plan.encodes_video
                );
            }
            Err(e) => {
                failed = true;
                let _ = writeln!(out, "  {profile:?}: no plan, {e}");
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn describe(f: &Format) -> String {
    let video = f.video.map_or_else(
        || "-".to_owned(),
        |v| format!("{:?} {:?}p {:?}fps", v.codec, v.height, v.fps),
    );
    let audio = f.audio.map_or_else(
        || "-".to_owned(),
        |a| format!("{:?} {:?}k", a.codec, a.bitrate),
    );
    let usable = if f.is_usable() { "" } else { " unusable" };
    format!(
        "{:<14} {video:<28} {audio:<18} {:?} {:?}{usable}",
        f.id, f.size, f.protocol
    )
}
