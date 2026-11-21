//! Job states and the scheduler. Both are pure; threads only carry out their decisions.

/// A job's identity, stable for the life of the queue file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// One piece of work a worker does for a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// Asks the engine what the link offers.
    Probe,
    /// Downloads the planned formats.
    Fetch,
    /// Inspects the file, then remuxes, encodes or only renames it.
    Convert,
}

/// Why a job stopped. Each kind maps to one retry policy and one message in `app`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureKind {
    /// The site changed and the engine needs an update.
    EngineOutdated,
    /// A bot filter refused the plain client.
    BotWall,
    /// The site asks to prove a human is there.
    BotCheck,
    /// Needs an account.
    NeedsLogin,
    /// Removed, private or blocked in this country.
    Unavailable,
    /// A live stream that hasn't started.
    NotStarted,
    /// No video on the page.
    Unsupported,
    /// A download link expired between probe and fetch.
    Expired,
    /// Timeouts, resets, DNS.
    Network,
    /// The destination disk is full.
    DiskFull,
    /// A tool failed to start or crashed.
    ToolBroken,
    /// The source has nothing the profile can use.
    NoFormat,
    /// Anything else.
    Internal,
}

/// A waiting state a paused job returns to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Waiting {
    /// Before the probe.
    Queued,
    /// Probed and planned, before the fetch.
    Ready,
    /// Fetched, before the convert step.
    Fetched,
}

/// Where a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState {
    /// Waiting for a probe.
    Queued,
    /// The engine is reading the link.
    Probing,
    /// Planned, waiting for a fetch.
    Ready,
    /// Downloading.
    Fetching,
    /// Downloaded, waiting for the convert step.
    Fetched,
    /// Inspecting, converting or renaming the file.
    Converting,
    /// The file is in place.
    Done,
    /// Held by the person; resumes where it waited.
    Paused(Waiting),
    /// Stopped by the person.
    Cancelled,
    /// Stopped by a failure.
    Failed(FailureKind),
}

/// What happens to a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobEvent {
    /// A worker starts the step.
    Start(Step),
    /// The step succeeded.
    Finish(Step),
    /// The running step failed.
    Fail(FailureKind),
    /// Hold the job; a running step is stopped and redone on resume.
    Pause,
    /// Continue a paused job.
    Resume,
    /// Stop the job for good.
    Cancel,
    /// Start a stopped or finished job over from the probe.
    Retry,
}

impl JobState {
    /// The state after `event`, or `None` when the event makes no sense here.
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "one arm per transition reads as the state table"
    )]
    pub const fn next(self, event: JobEvent) -> Option<Self> {
        use JobEvent as E;
        use JobState as S;
        let next = match (self, event) {
            (S::Queued, E::Start(Step::Probe)) => S::Probing,
            (S::Probing, E::Finish(Step::Probe)) => S::Ready,
            (S::Ready, E::Start(Step::Fetch)) => S::Fetching,
            (S::Fetching, E::Finish(Step::Fetch)) => S::Fetched,
            (S::Fetched, E::Start(Step::Convert)) => S::Converting,
            (S::Converting, E::Finish(Step::Convert)) => S::Done,
            (S::Probing | S::Fetching | S::Converting, E::Fail(kind)) => S::Failed(kind),
            (S::Queued | S::Probing, E::Pause) => S::Paused(Waiting::Queued),
            (S::Ready | S::Fetching, E::Pause) => S::Paused(Waiting::Ready),
            (S::Fetched | S::Converting, E::Pause) => S::Paused(Waiting::Fetched),
            (S::Paused(Waiting::Queued), E::Resume) => S::Queued,
            (S::Paused(Waiting::Ready), E::Resume) => S::Ready,
            (S::Paused(Waiting::Fetched), E::Resume) => S::Fetched,
            (
                S::Queued
                | S::Probing
                | S::Ready
                | S::Fetching
                | S::Fetched
                | S::Converting
                | S::Paused(_),
                E::Cancel,
            ) => S::Cancelled,
            (S::Done | S::Cancelled | S::Failed(_), E::Retry) => S::Queued,
            _ => return None,
        };
        Some(next)
    }

    /// The step a worker runs in this state, if any.
    #[must_use]
    pub const fn running(self) -> Option<Step> {
        match self {
            Self::Probing => Some(Step::Probe),
            Self::Fetching => Some(Step::Fetch),
            Self::Converting => Some(Step::Convert),
            _ => None,
        }
    }

    /// The step this state waits for, if any.
    #[must_use]
    pub const fn waiting_for(self) -> Option<Step> {
        match self {
            Self::Queued => Some(Step::Probe),
            Self::Ready => Some(Step::Fetch),
            Self::Fetched => Some(Step::Convert),
            _ => None,
        }
    }

    /// Done, cancelled or failed: nothing more happens without a retry.
    #[must_use]
    pub const fn is_over(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled | Self::Failed(_))
    }
}

/// How many steps of each kind may run at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permits {
    /// Probes at once.
    pub probes: usize,
    /// Fetches at once.
    pub fetches: usize,
    /// Conversions at once; more than one fights over an old CPU.
    pub converts: usize,
}

impl Permits {
    /// Two probes, two fetches, one conversion.
    pub const DEFAULT: Self = Self {
        probes: 2,
        fetches: 2,
        converts: 1,
    };

    const fn limit(self, step: Step) -> usize {
        match step {
            Step::Probe => self.probes,
            Step::Fetch => self.fetches,
            Step::Convert => self.converts,
        }
    }
}

/// The steps to start now, oldest job first, within what `permits` leaves free.
/// `jobs` is in queue order; only jobs the caller marks as allowed to advance are offered.
#[must_use]
pub fn schedule(jobs: &[(JobId, JobState)], permits: Permits) -> Vec<(JobId, Step)> {
    let busy = |step: Step| {
        jobs.iter()
            .filter(|(_, s)| s.running() == Some(step))
            .count()
    };
    let mut free = [Step::Probe, Step::Fetch, Step::Convert]
        .map(|step| permits.limit(step).saturating_sub(busy(step)));
    let mut start = Vec::new();
    for &(id, state) in jobs {
        let Some(step) = state.waiting_for() else {
            continue;
        };
        let slot = &mut free[step as usize];
        if *slot > 0 {
            *slot -= 1;
            start.push((id, step));
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use super::{FailureKind, JobEvent, JobState, Step, Waiting};

    const STATES: [JobState; 12] = [
        JobState::Queued,
        JobState::Probing,
        JobState::Ready,
        JobState::Fetching,
        JobState::Fetched,
        JobState::Converting,
        JobState::Done,
        JobState::Paused(Waiting::Queued),
        JobState::Paused(Waiting::Ready),
        JobState::Paused(Waiting::Fetched),
        JobState::Cancelled,
        JobState::Failed(FailureKind::Network),
    ];

    const EVENTS: [JobEvent; 12] = [
        JobEvent::Start(Step::Probe),
        JobEvent::Start(Step::Fetch),
        JobEvent::Start(Step::Convert),
        JobEvent::Finish(Step::Probe),
        JobEvent::Finish(Step::Fetch),
        JobEvent::Finish(Step::Convert),
        JobEvent::Fail(FailureKind::Network),
        JobEvent::Pause,
        JobEvent::Resume,
        JobEvent::Cancel,
        JobEvent::Retry,
        JobEvent::Fail(FailureKind::DiskFull),
    ];

    /// Every state against every event, written out once, so a change to `next` shows here.
    #[test]
    fn transition_table() {
        use JobState as S;
        let q = S::Paused(Waiting::Queued);
        let r = S::Paused(Waiting::Ready);
        let f = S::Paused(Waiting::Fetched);
        let net = S::Failed(FailureKind::Network);
        let disk = S::Failed(FailureKind::DiskFull);
        let x = None;
        #[rustfmt::skip]
        let table: [[Option<JobState>; 12]; 12] = [
            // start p, start f, start c, fin p, fin f, fin c, fail net, pause, resume, cancel, retry, fail disk
            [Some(S::Probing), x, x, x, x, x, x, Some(q), x, Some(S::Cancelled), x, x],
            [x, x, x, Some(S::Ready), x, x, Some(net), Some(q), x, Some(S::Cancelled), x, Some(disk)],
            [x, Some(S::Fetching), x, x, x, x, x, Some(r), x, Some(S::Cancelled), x, x],
            [x, x, x, x, Some(S::Fetched), x, Some(net), Some(r), x, Some(S::Cancelled), x, Some(disk)],
            [x, x, Some(S::Converting), x, x, x, x, Some(f), x, Some(S::Cancelled), x, x],
            [x, x, x, x, x, Some(S::Done), Some(net), Some(f), x, Some(S::Cancelled), x, Some(disk)],
            [x, x, x, x, x, x, x, x, x, x, Some(S::Queued), x],
            [x, x, x, x, x, x, x, x, Some(S::Queued), Some(S::Cancelled), x, x],
            [x, x, x, x, x, x, x, x, Some(S::Ready), Some(S::Cancelled), x, x],
            [x, x, x, x, x, x, x, x, Some(S::Fetched), Some(S::Cancelled), x, x],
            [x, x, x, x, x, x, x, x, x, x, Some(S::Queued), x],
            [x, x, x, x, x, x, x, x, x, x, Some(S::Queued), x],
        ];
        for (state, row) in STATES.iter().zip(table) {
            for (event, want) in EVENTS.iter().zip(row) {
                assert_eq!(state.next(*event), want, "{state:?} + {event:?}");
            }
        }
    }
}
