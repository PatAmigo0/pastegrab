//! The scheduler against queues in every phase.

use pastegrab_domain::job::{JobId, JobState, Permits, Step, Waiting, schedule};

fn queue(states: &[JobState]) -> Vec<(JobId, JobState)> {
    (0..).map(JobId).zip(states.iter().copied()).collect()
}

#[test]
fn two_probes_start_from_a_fresh_queue() {
    let jobs = queue(&[JobState::Queued; 3]);
    assert_eq!(
        schedule(&jobs, Permits::DEFAULT),
        [(JobId(0), Step::Probe), (JobId(1), Step::Probe)]
    );
}

#[test]
fn running_steps_hold_their_permits() {
    let jobs = queue(&[
        JobState::Probing,
        JobState::Queued,
        JobState::Queued,
        JobState::Fetching,
        JobState::Ready,
        JobState::Ready,
        JobState::Converting,
        JobState::Fetched,
    ]);
    assert_eq!(
        schedule(&jobs, Permits::DEFAULT),
        [(JobId(1), Step::Probe), (JobId(4), Step::Fetch)]
    );
}

#[test]
fn paused_and_finished_jobs_take_nothing() {
    let jobs = queue(&[
        JobState::Paused(Waiting::Queued),
        JobState::Done,
        JobState::Cancelled,
        JobState::Fetched,
        JobState::Fetched,
    ]);
    assert_eq!(
        schedule(&jobs, Permits::DEFAULT),
        [(JobId(3), Step::Convert)]
    );
}
