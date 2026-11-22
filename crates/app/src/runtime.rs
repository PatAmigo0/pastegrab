//! Runs the queue on its own thread and every step on a worker thread of its own.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use pastegrab_domain::job::{JobId, JobState};

use crate::cancel::Cancel;
use crate::failure::Failure;
use crate::ports::{Phase, Progress, ProgressSink, View};
use crate::queue::{Effect, Input, Outcome, Queue, Task};
use crate::steps::{self, Ports};

/// Messages on the queue thread's channel. Worker results carry the start they answer, which
/// keeps a late result from a cancelled step away from a restarted one.
enum Msg {
    Input(Input),
    Worker(JobId, u64, Outcome),
}

/// Sends input to a running queue.
#[derive(Clone)]
pub struct Handle {
    tx: SyncSender<Msg>,
}

impl Handle {
    /// Queues `input`; `false` once the queue thread has ended.
    #[must_use]
    pub fn send(&self, input: Input) -> bool {
        self.tx.send(Msg::Input(input)).is_ok()
    }
}

/// Starts the queue thread. With `until_idle` it ends once nothing runs or waits, and its join
/// handle returns the final states; otherwise it ends when every `Handle` is dropped.
#[must_use]
pub fn start(
    mut queue: Queue,
    ports: Arc<Ports>,
    view: Arc<dyn View>,
    until_idle: bool,
) -> (Handle, JoinHandle<Vec<(JobId, JobState)>>) {
    let (tx, rx) = sync_channel(64);
    let handle = Handle { tx: tx.clone() };
    let thread = thread::spawn(move || {
        let mut running: HashMap<JobId, (u64, Arc<Cancel>)> = HashMap::new();
        let mut starts = 0_u64;
        while let Ok(msg) = rx.recv() {
            let input = match msg {
                Msg::Input(input) => input,
                Msg::Worker(id, start, outcome) => {
                    if running
                        .get(&id)
                        .is_none_or(|(current, _)| *current != start)
                    {
                        continue;
                    }
                    running.remove(&id);
                    Input::Done(id, outcome)
                }
            };
            for effect in queue.handle(input) {
                match effect {
                    Effect::Show(update) => view.apply(update),
                    // An unsaved choice still holds until the window closes.
                    Effect::Save(choices) => {
                        if let Some(store) = &ports.settings {
                            let _ = store.save(&choices);
                        }
                    }
                    Effect::Stop(id) => {
                        if let Some((_, cancel)) = running.remove(&id) {
                            cancel.cancel();
                        }
                    }
                    Effect::Start(id, task) => {
                        starts += 1;
                        let cancel = Arc::new(Cancel::default());
                        running.insert(id, (starts, Arc::clone(&cancel)));
                        let worker = Worker {
                            id,
                            start: starts,
                            ports: Arc::clone(&ports),
                            view: Arc::clone(&view),
                            cancel,
                            tx: tx.clone(),
                        };
                        worker.spawn(task);
                    }
                }
            }
            if until_idle && running.is_empty() && queue.is_idle() {
                break;
            }
        }
        queue.states()
    });
    (handle, thread)
}

struct Worker {
    id: JobId,
    start: u64,
    ports: Arc<Ports>,
    view: Arc<dyn View>,
    cancel: Arc<Cancel>,
    tx: SyncSender<Msg>,
}

impl Worker {
    fn spawn(self, task: Task) {
        let step = task.step();
        let (id, start, tx) = (self.id, self.start, self.tx.clone());
        let spawned = thread::Builder::new()
            .name(format!("job-{}-{step:?}", self.id.0))
            .spawn(move || {
                let outcome =
                    catch_unwind(AssertUnwindSafe(|| self.run(&task))).unwrap_or_else(|_| {
                        Outcome::failed(step, Failure::internal("the step panicked"))
                    });
                let _ = self.tx.send(Msg::Worker(self.id, self.start, outcome));
            });
        if let Err(e) = spawned {
            let failed = Outcome::failed(step, Failure::internal(format!("no worker thread: {e}")));
            let _ = tx.try_send(Msg::Worker(id, start, failed));
        }
    }

    fn run(&self, task: &Task) -> Outcome {
        let sink = JobProgress {
            id: self.id,
            view: &*self.view,
        };
        match task {
            Task::Probe(t) => Outcome::Probed(steps::probe(&self.ports, t, &self.cancel)),
            Task::Fetch(t) => Outcome::Fetched(steps::fetch(&self.ports, t, &sink, &self.cancel)),
            Task::Convert(t) => {
                Outcome::Converted(steps::convert(&self.ports, t, &sink, &self.cancel))
            }
        }
    }
}

struct JobProgress<'a> {
    id: JobId,
    view: &'a dyn View,
}

impl ProgressSink for JobProgress<'_> {
    fn report(&self, phase: Phase, progress: Progress) {
        self.view.progress(self.id, phase, progress);
    }
}
