//! The board in the person's words.

use pastegrab_app::board::Row;
use pastegrab_app::ports::{Phase, Progress};
use pastegrab_domain::job::{FailureKind, JobState};
use pastegrab_domain::plan::PlanError;
use pastegrab_i18n::Strings;

const SEPARATOR: &str = " · ";

/// The state column of a row.
pub(crate) fn state(row: &Row, s: &Strings) -> String {
    let states = &s.states;
    match row.state {
        JobState::Queued => states.queued.into(),
        JobState::Probing => states.probing.into(),
        JobState::Ready => match (
            row.no_plan,
            row.plan.as_ref().and_then(|p| p.expected_bytes),
        ) {
            (Some(reason), _) => no_plan(reason, s).into(),
            (None, Some(bytes)) => format!("{}, {}", states.ready, (s.size)(bytes)),
            (None, None) => states.ready.into(),
        },
        JobState::Fetching | JobState::Fetched | JobState::Converting => running(row, s),
        JobState::Done => states.done.into(),
        JobState::Paused(_) => states.paused.into(),
        JobState::Cancelled => states.cancelled.into(),
        JobState::Failed(kind) => failure(kind, s).into(),
    }
}

fn running(row: &Row, s: &Strings) -> String {
    let states = &s.states;
    let Some((phase, progress)) = row.progress else {
        return if row.state == JobState::Converting {
            states.converting.into()
        } else {
            states.downloading.into()
        };
    };
    let word = match phase {
        Phase::Download => states.downloading,
        Phase::Merge => return states.merging.into(),
        Phase::Convert => states.converting,
    };
    let Some(percent) = percent(progress) else {
        return word.into();
    };
    match progress.speed.filter(|_| phase == Phase::Download) {
        Some(speed) => format!("{word} {percent}%, {}", (s.speed)(speed)),
        None => format!("{word} {percent}%"),
    }
}

fn percent(progress: Progress) -> Option<u64> {
    let total = progress.total.filter(|&total| total > 0)?;
    Some((progress.done.saturating_mul(100) / total).min(100))
}

/// The line under the card's title: site, length and size, or why the profile finds nothing.
pub(crate) fn card_detail(row: &Row, s: &Strings) -> String {
    if let Some(reason) = row.no_plan {
        return no_plan(reason, s).into();
    }
    let mut parts = vec![host(&row.url).to_owned()];
    parts.extend(row.duration.map(s.duration));
    parts.extend(row.plan.as_ref().and_then(|p| p.expected_bytes).map(s.size));
    parts.join(SEPARATOR)
}

/// The card's title: the newest waiting video, and how many more wait with it.
pub(crate) fn card_title(waiting: &[&Row], s: &Strings) -> String {
    match waiting {
        [] => String::new(),
        [row] => row.name().into(),
        [row, rest @ ..] => format!("{}, {}", row.name(), (s.more)(rest.len())),
    }
}

pub(crate) const fn failure(kind: FailureKind, s: &Strings) -> &'static str {
    let f = &s.failures;
    match kind {
        FailureKind::EngineOutdated => f.engine_outdated,
        FailureKind::BotWall => f.bot_wall,
        FailureKind::BotCheck => f.bot_check,
        FailureKind::NeedsLogin => f.needs_login,
        FailureKind::Unavailable => f.unavailable,
        FailureKind::NotStarted => f.not_started,
        FailureKind::Unsupported => f.unsupported,
        FailureKind::Expired => f.expired,
        FailureKind::Network => f.network,
        FailureKind::DiskFull => f.disk_full,
        FailureKind::ToolBroken => f.tool_broken,
        FailureKind::NoFormat => f.no_format,
        FailureKind::Internal => f.internal,
    }
}

const fn no_plan(reason: PlanError, s: &Strings) -> &'static str {
    match reason {
        PlanError::NoUsableFormat => s.no_plan.no_usable_format,
        PlanError::NoVideo => s.no_plan.no_video,
        PlanError::NoAudio => s.no_plan.no_audio,
    }
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use pastegrab_app::board::Row;
    use pastegrab_app::ports::{Phase, Progress};
    use pastegrab_domain::job::{FailureKind, JobId, JobState};
    use pastegrab_domain::media::FormatId;
    use pastegrab_domain::plan::{FetchPlan, PlanError};
    use pastegrab_domain::profile::ProfileKind;
    use pastegrab_i18n::RU;

    use super::{card_detail, card_title, state};

    fn row(state: JobState) -> Row {
        Row {
            id: JobId(1),
            url: "https://www.youtube.com/watch?v=AbC123xyz_0".into(),
            profile: ProfileKind::Video,
            title: Some("Забег на 100 метров".into()),
            duration: Some(9664),
            state,
            plan: Some(FetchPlan {
                format_ids: FormatId::parse("18").into_iter().collect(),
                merge_into: None,
                section: None,
                expected_bytes: Some(1_830_000_000),
                encodes_video: false,
            }),
            no_plan: None,
            progress: None,
            file: None,
            failure: None,
        }
    }

    #[test]
    fn states_read_as_plain_words() {
        assert_eq!(
            state(&row(JobState::Ready), &RU),
            "Готово к загрузке, около 1,8 ГБ"
        );
        let mut fetching = row(JobState::Fetching);
        fetching.progress = Some((
            Phase::Download,
            Progress {
                done: 450,
                total: Some(1000),
                speed: Some(2_150_000),
                eta: None,
            },
        ));
        assert_eq!(state(&fetching, &RU), "Загрузка 45%, 2,1 МБ/с");
        assert_eq!(state(&row(JobState::Converting), &RU), "Конвертация");
        let failed = row(JobState::Failed(FailureKind::Unavailable));
        assert_eq!(
            state(&failed, &RU),
            "Видео удалено или закрыто для вашей страны"
        );
    }

    #[test]
    fn the_card_names_site_length_and_size() {
        let ready = row(JobState::Ready);
        assert_eq!(
            card_detail(&ready, &RU),
            "youtube.com · 2:41:04 · около 1,8 ГБ"
        );
        let mut silent = row(JobState::Ready);
        silent.no_plan = Some(PlanError::NoAudio);
        assert_eq!(card_detail(&silent, &RU), "В этом видео нет звука");
        let other = row(JobState::Ready);
        assert_eq!(
            card_title(&[&ready, &other, &other], &RU),
            "Забег на 100 метров, и ещё 2"
        );
    }
}
