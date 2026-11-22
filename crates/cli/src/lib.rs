//! A `View` over a terminal: finished files on stdout, everything else on stderr.

use std::collections::HashMap;
use std::fmt::Arguments;
use std::io::{self, Write};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use pastegrab_app::ports::{Phase, Progress, View, ViewUpdate};
use pastegrab_domain::job::{JobId, JobState};

const PROGRESS_EVERY: Duration = Duration::from_secs(2);

/// Prints one line per change and at most one progress line per job every two seconds.
#[derive(Debug, Default)]
pub struct TermView {
    shown: Mutex<HashMap<JobId, Instant>>,
}

impl View for TermView {
    fn apply(&self, update: ViewUpdate) {
        match update {
            ViewUpdate::Added { id, url, .. } => note(id, format_args!("{url}")),
            ViewUpdate::State { id, state } => {
                if let Some(word) = state_word(state) {
                    note(id, format_args!("{word}"));
                }
            }
            ViewUpdate::Planned {
                id, title, plan, ..
            } => {
                let size = plan
                    .expected_bytes
                    .map_or(0, |bytes| bytes.div_ceil(1_000_000));
                let formats = plan.selector();
                note(
                    id,
                    format_args!("{title}: formats {formats}, about {size} MB"),
                );
            }
            ViewUpdate::NoPlan {
                id, title, reason, ..
            } => {
                note(id, format_args!("{title}: no plan, {reason}"));
            }
            ViewUpdate::Finished { id, file } => {
                let _ = writeln!(io::stdout().lock(), "{}", file.display());
                note(id, format_args!("done"));
            }
            ViewUpdate::Failed { id, failure } => {
                note(id, format_args!("failed, {:?}", failure.kind));
                let _ = writeln!(io::stderr().lock(), "{}", failure.detail);
            }
            ViewUpdate::NoLinks => {
                let _ = writeln!(io::stderr().lock(), "no links in the text");
            }
        }
    }

    fn progress(&self, id: JobId, phase: Phase, progress: Progress) {
        let now = Instant::now();
        {
            let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
            if shown
                .get(&id)
                .is_some_and(|at| now.duration_since(*at) < PROGRESS_EVERY)
            {
                return;
            }
            shown.insert(id, now);
        }
        let verb = match phase {
            Phase::Download => "downloading",
            Phase::Merge => "merging",
            Phase::Convert => "converting",
        };
        let percent = progress
            .total
            .filter(|&total| total > 0)
            .map_or(0, |total| progress.done.saturating_mul(100) / total);
        match progress.speed {
            Some(speed) => {
                let (whole, tenth) = (speed / 1_000_000, speed / 100_000 % 10);
                note(id, format_args!("{verb} {percent}%, {whole}.{tenth} MB/s"));
            }
            None => note(id, format_args!("{verb} {percent}%")),
        }
    }
}

const fn state_word(state: JobState) -> Option<&'static str> {
    match state {
        JobState::Probing => Some("asking the site"),
        JobState::Paused(_) => Some("paused"),
        JobState::Cancelled => Some("cancelled"),
        _ => None,
    }
}

fn note(id: JobId, text: Arguments<'_>) {
    let _ = writeln!(io::stderr().lock(), "[{}] {text}", id.0);
}
