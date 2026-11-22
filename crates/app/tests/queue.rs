//! The queue's decisions, driven input by input without threads.

use std::path::PathBuf;

use pastegrab_app::Failure;
use pastegrab_app::ports::{Resolved, Source, ViewUpdate};
use pastegrab_app::queue::{Effect, Input, Outcome, Paste, Queue, Settings, Task};
use pastegrab_domain::job::{FailureKind, JobId, JobState, Permits, Step};
use pastegrab_domain::plan::{Machine, PlanError};
use pastegrab_domain::profile::ProfileKind;
use pastegrab_testkit::media;

fn queue() -> Queue {
    Queue::new(Settings {
        profile: ProfileKind::Video,
        machine: Machine { screen_height: 768 },
        dest: PathBuf::from("/videos"),
        data: PathBuf::from("/data"),
        permits: Permits::DEFAULT,
    })
}

fn paste(text: &str) -> Input {
    Input::Paste(Paste {
        text: text.into(),
        profile: None,
        section: None,
        hold: false,
    })
}

fn resolved(title: &str) -> Resolved {
    Resolved {
        info: media(title),
        source: Source::Engine {
            info_json: PathBuf::from("/data/jobs/1/probe.info.json"),
            impersonate: false,
        },
    }
}

fn started(fx: &[Effect]) -> Vec<(JobId, Step)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Start(id, task) => Some((*id, task.step())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_paste_starts_two_probes_and_queues_the_third() {
    let mut q = queue();
    let fx = q.handle(paste(
        "https://youtu.be/AAAAAAAAAAA https://youtu.be/BBBBBBBBBBB https://youtu.be/CCCCCCCCCCC",
    ));
    assert_eq!(
        started(&fx),
        [(JobId(1), Step::Probe), (JobId(2), Step::Probe)]
    );
    assert_eq!(q.states()[2], (JobId(3), JobState::Queued));
}

#[test]
fn a_job_walks_from_probe_to_done() {
    let mut q = queue();
    q.handle(paste("https://youtu.be/AAAAAAAAAAA"));
    let fx = q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(resolved("Race")))));
    assert!(
        fx.iter().any(
            |e| matches!(e, Effect::Show(ViewUpdate::Planned { title, .. }) if title == "Race")
        )
    );
    let fetch = fx.iter().find_map(|e| match e {
        Effect::Start(_, Task::Fetch(t)) => Some(t.clone()),
        _ => None,
    });
    let fetch = fetch.expect("a fetch starts");
    assert_eq!(fetch.plan.selector(), "136+140");
    assert_eq!(fetch.work_dir, PathBuf::from("/videos/.pastegrab-1"));

    let fx = q.handle(Input::Done(
        JobId(1),
        Outcome::Fetched(Ok(PathBuf::from("/videos/.pastegrab-1/AAAAAAAAAAA.mp4"))),
    ));
    let convert = fx.iter().find_map(|e| match e {
        Effect::Start(_, Task::Convert(t)) => Some(t.clone()),
        _ => None,
    });
    let convert = convert.expect("a conversion starts");
    assert_eq!(convert.title, "Race");
    assert_eq!(convert.fallback, "AAAAAAAAAAA");

    let fx = q.handle(Input::Done(
        JobId(1),
        Outcome::Converted(Ok(PathBuf::from("/videos/Race.mp4"))),
    ));
    assert!(fx.contains(&Effect::Show(ViewUpdate::Finished {
        id: JobId(1),
        file: PathBuf::from("/videos/Race.mp4"),
    })));
    assert!(q.is_idle());
}

#[test]
fn a_bot_wall_retries_once_with_impersonation() {
    let mut q = queue();
    q.handle(paste("https://olympics.com/ru/video/some-race"));
    let wall = Failure::new(FailureKind::BotWall, "HTTP Error 403");
    let fx = q.handle(Input::Done(JobId(1), Outcome::Probed(Err(wall.clone()))));
    let retry = fx.iter().find_map(|e| match e {
        Effect::Start(_, Task::Probe(t)) => Some(t.impersonate),
        _ => None,
    });
    assert_eq!(retry, Some(true));
    let fx = q.handle(Input::Done(JobId(1), Outcome::Probed(Err(wall))));
    assert_eq!(started(&fx), []);
    assert_eq!(q.states()[0].1, JobState::Failed(FailureKind::BotWall));
}

#[test]
fn a_cancel_stops_the_worker_and_its_late_result_is_ignored() {
    let mut q = queue();
    q.handle(paste("https://youtu.be/AAAAAAAAAAA"));
    let fx = q.handle(Input::Cancel(JobId(1)));
    assert_eq!(fx[0], Effect::Stop(JobId(1)));
    let fx = q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(resolved("Race")))));
    assert_eq!(fx, []);
    assert_eq!(q.states()[0].1, JobState::Cancelled);
}

#[test]
fn a_profile_that_finds_nothing_fails_the_job() {
    let mut q = queue();
    q.handle(Input::Paste(Paste {
        text: "https://youtu.be/AAAAAAAAAAA".into(),
        profile: Some(ProfileKind::Video),
        section: None,
        hold: false,
    }));
    let mut sound_only = resolved("Track");
    sound_only.info.formats.retain(|f| f.video.is_none());
    q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(sound_only))));
    assert_eq!(q.states()[0].1, JobState::Failed(FailureKind::NoFormat));
}

#[test]
fn text_without_links_says_so_and_duplicates_are_skipped() {
    let mut q = queue();
    assert_eq!(
        q.handle(paste("просто текст")),
        [Effect::Show(ViewUpdate::NoLinks)]
    );
    q.handle(paste("https://youtu.be/AAAAAAAAAAA"));
    q.handle(paste("https://www.youtube.com/watch?v=AAAAAAAAAAA"));
    assert_eq!(q.states().len(), 1);
}

fn held(text: &str) -> Input {
    Input::Paste(Paste {
        text: text.into(),
        profile: None,
        section: None,
        hold: true,
    })
}

fn planned(fx: &[Effect]) -> Option<String> {
    fx.iter().find_map(|e| match e {
        Effect::Show(ViewUpdate::Planned { plan, .. }) => Some(plan.selector()),
        _ => None,
    })
}

#[test]
fn a_held_job_waits_after_its_probe_until_start() {
    let mut q = queue();
    q.handle(held("https://youtu.be/AAAAAAAAAAA"));
    let fx = q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(resolved("Race")))));
    assert_eq!(started(&fx), []);
    assert_eq!(q.states()[0].1, JobState::Ready);
    assert!(q.is_idle());
    let fx = q.handle(Input::Start(JobId(1)));
    assert_eq!(started(&fx), [(JobId(1), Step::Fetch)]);
}

#[test]
fn another_profile_plans_again_from_the_same_probe() {
    let mut q = queue();
    q.handle(held("https://youtu.be/AAAAAAAAAAA"));
    q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(resolved("Race")))));
    let fx = q.handle(Input::Profile(JobId(1), ProfileKind::Audio));
    assert_eq!(started(&fx), [], "no second probe");
    assert_eq!(planned(&fx).as_deref(), Some("140"));
}

#[test]
fn a_profile_the_source_cannot_meet_keeps_a_held_job_waiting() {
    let mut q = queue();
    q.handle(held("https://youtu.be/AAAAAAAAAAA"));
    let mut silent = resolved("Race");
    silent.info.formats.retain(|f| f.audio.is_none());
    q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(silent))));
    let fx = q.handle(Input::Profile(JobId(1), ProfileKind::Audio));
    assert!(fx.iter().any(|e| matches!(
        e,
        Effect::Show(ViewUpdate::NoPlan {
            reason: PlanError::NoAudio,
            ..
        })
    )));
    let fx = q.handle(Input::Start(JobId(1)));
    assert_eq!(started(&fx), [], "nothing to download yet");
    let fx = q.handle(Input::Profile(JobId(1), ProfileKind::Video));
    assert_eq!(planned(&fx).as_deref(), Some("136"));
    assert_eq!(started(&fx), [(JobId(1), Step::Fetch)]);
}

#[test]
fn another_folder_applies_to_fetches_that_start_after_it() {
    let mut q = queue();
    q.handle(paste("https://youtu.be/AAAAAAAAAAA"));
    q.handle(Input::Dest(PathBuf::from("/elsewhere")));
    let fx = q.handle(Input::Done(JobId(1), Outcome::Probed(Ok(resolved("Race")))));
    let work = fx.iter().find_map(|e| match e {
        Effect::Start(_, Task::Fetch(t)) => Some(t.work_dir.clone()),
        _ => None,
    });
    assert_eq!(work, Some(PathBuf::from("/elsewhere/.pastegrab-1")));
}
