//! What a window shows: a row per job and the card for probed jobs that wait for the download
//! button. It holds no text; the view turns kinds and numbers into words.

use std::path::PathBuf;

use pastegrab_domain::job::{JobId, JobState};
use pastegrab_domain::plan::{FetchPlan, PlanError};
use pastegrab_domain::profile::ProfileKind;

use crate::failure::Failure;
use crate::ports::{Phase, Progress, ViewUpdate};

/// One job as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The job.
    pub id: JobId,
    /// The cleaned link.
    pub url: String,
    /// The profile the job follows.
    pub profile: ProfileKind,
    /// The video's title, once the probe names it.
    pub title: Option<String>,
    /// Where the job is.
    pub state: JobState,
    /// The formats the planner picked.
    pub plan: Option<FetchPlan>,
    /// Why the chosen profile finds nothing in this source.
    pub no_plan: Option<PlanError>,
    /// The latest numbers of the running step.
    pub progress: Option<(Phase, Progress)>,
    /// The finished file.
    pub file: Option<PathBuf>,
    /// Why the job stopped.
    pub failure: Option<Failure>,
}

impl Row {
    /// The title, or the link before the probe.
    #[must_use]
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.url)
    }
}

/// What an update changed, so the view redraws only that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The row at this index.
    Row(usize),
    /// A new row at the end.
    Added,
    /// The paste held no link.
    NoLinks,
    /// An update for a job the board doesn't know.
    Nothing,
}

/// Every job, oldest first.
#[derive(Debug, Default)]
pub struct Board {
    rows: Vec<Row>,
}

impl Board {
    /// Applies one update from the core.
    pub fn apply(&mut self, update: ViewUpdate) -> Change {
        match update {
            ViewUpdate::Added { id, url, profile } => {
                self.rows.push(Row {
                    id,
                    url,
                    profile,
                    title: None,
                    state: JobState::Queued,
                    plan: None,
                    no_plan: None,
                    progress: None,
                    file: None,
                    failure: None,
                });
                Change::Added
            }
            ViewUpdate::NoLinks => Change::NoLinks,
            ViewUpdate::State { id, state } => self.edit(id, |row| {
                row.state = state;
                if state.running().is_none() {
                    row.progress = None;
                }
                if state == JobState::Queued {
                    row.failure = None;
                }
            }),
            ViewUpdate::Planned {
                id,
                title,
                profile,
                plan,
            } => self.edit(id, |row| {
                row.title = Some(title);
                row.profile = profile;
                row.plan = Some(plan);
                row.no_plan = None;
            }),
            ViewUpdate::NoPlan {
                id,
                title,
                profile,
                reason,
            } => self.edit(id, |row| {
                row.title = Some(title);
                row.profile = profile;
                row.plan = None;
                row.no_plan = Some(reason);
            }),
            ViewUpdate::Finished { id, file } => self.edit(id, |row| row.file = Some(file)),
            ViewUpdate::Failed { id, failure } => self.edit(id, |row| row.failure = Some(failure)),
        }
    }

    /// The latest numbers of a job's running step.
    pub fn progress(&mut self, id: JobId, phase: Phase, progress: Progress) -> Change {
        self.edit(id, |row| row.progress = Some((phase, progress)))
    }

    /// Every row, oldest first.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Probed jobs that wait for the download button, newest first.
    pub fn waiting(&self) -> impl Iterator<Item = &Row> {
        self.rows
            .iter()
            .rev()
            .filter(|row| row.state == JobState::Ready)
    }

    fn edit(&mut self, id: JobId, change: impl FnOnce(&mut Row)) -> Change {
        match self
            .rows
            .iter_mut()
            .enumerate()
            .find(|(_, row)| row.id == id)
        {
            Some((index, row)) => {
                change(row);
                Change::Row(index)
            }
            None => Change::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pastegrab_domain::job::{FailureKind, JobId, JobState};
    use pastegrab_domain::plan::PlanError;
    use pastegrab_domain::profile::ProfileKind;

    use super::{Board, Change};
    use crate::failure::Failure;
    use crate::ports::{Phase, Progress, ViewUpdate};

    fn added(board: &mut Board, id: u64) {
        let url = format!("https://youtu.be/{id}");
        let profile = ProfileKind::Video;
        board.apply(ViewUpdate::Added {
            id: JobId(id),
            url,
            profile,
        });
    }

    fn state(board: &mut Board, id: u64, state: JobState) -> Change {
        board.apply(ViewUpdate::State {
            id: JobId(id),
            state,
        })
    }

    #[test]
    fn a_probed_job_waits_on_the_card_until_it_runs() {
        let mut board = Board::default();
        added(&mut board, 1);
        added(&mut board, 2);
        assert_eq!(board.rows()[0].name(), "https://youtu.be/1");
        let change = board.apply(ViewUpdate::NoPlan {
            id: JobId(2),
            title: "Забег".into(),
            profile: ProfileKind::Audio,
            reason: PlanError::NoAudio,
        });
        assert_eq!(board.rows()[1].profile, ProfileKind::Audio);
        assert_eq!(change, Change::Row(1));
        state(&mut board, 2, JobState::Ready);
        let waiting: Vec<&str> = board.waiting().map(super::Row::name).collect();
        assert_eq!(waiting, ["Забег"]);
        state(&mut board, 2, JobState::Fetching);
        assert_eq!(board.waiting().count(), 0);
    }

    #[test]
    fn progress_belongs_to_the_running_step_and_a_retry_starts_clean() {
        let mut board = Board::default();
        added(&mut board, 1);
        state(&mut board, 1, JobState::Fetching);
        let numbers = Progress {
            done: 5,
            total: Some(10),
            ..Progress::default()
        };
        assert_eq!(
            board.progress(JobId(1), Phase::Download, numbers),
            Change::Row(0)
        );
        state(&mut board, 1, JobState::Fetched);
        assert_eq!(board.rows()[0].progress, None);
        board.apply(ViewUpdate::Failed {
            id: JobId(1),
            failure: Failure::new(FailureKind::Network, "reset"),
        });
        state(&mut board, 1, JobState::Queued);
        assert_eq!(board.rows()[0].failure, None);
        let done = PathBuf::from("/videos/Забег.mp4");
        board.apply(ViewUpdate::Finished {
            id: JobId(1),
            file: done.clone(),
        });
        assert_eq!(board.rows()[0].file, Some(done));
        assert_eq!(
            board.progress(JobId(9), Phase::Download, numbers),
            Change::Nothing
        );
    }
}
