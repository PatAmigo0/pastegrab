//! Runs the queue on its own thread and every step on a worker thread of its own.

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use pastegrab_domain::job::{JobId, JobState};

use crate::cancel::Cancel;
use crate::failure::Failure;
use crate::ports::{Files, Phase, Progress, ProgressSink, View};
use crate::queue::{Effect, Input, Outcome, Queue, Task};
use crate::steps::{self, Ports};

/// Messages on the queue thread's channel. Worker results carry the start they answer, which
/// keeps a late result from a cancelled step away from a restarted one.
enum Msg {
    Input(Input),
    Worker(JobId, u64, Outcome),
    Shutdown,
}

struct HandleInner {
    tx: SyncSender<Msg>,
}

impl Drop for HandleInner {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
    }
}

/// Sends input to a running queue.
#[derive(Clone)]
pub struct Handle {
    inner: Arc<HandleInner>,
}

impl Handle {
    /// Queues `input`; `false` once the queue thread has ended.
    #[must_use]
    pub fn send(&self, input: Input) -> bool {
        self.inner.tx.send(Msg::Input(input)).is_ok()
    }

    /// Cancels active work, cleans working folders, and asks the queue thread to exit.
    pub fn shutdown(&self) {
        let _ = self.inner.tx.send(Msg::Shutdown);
    }
}

/// Starts the queue thread. With `until_idle` it ends once nothing runs or waits, and its join
/// handle returns the final states; otherwise it ends when every `Handle` is dropped or
/// [`Handle::shutdown`] is called.
#[must_use]
pub fn start(
    queue: Queue,
    ports: Arc<Ports>,
    view: Arc<dyn View>,
    until_idle: bool,
) -> (Handle, JoinHandle<Vec<(JobId, JobState)>>) {
    let (tx, rx) = sync_channel(64);
    let handle = Handle {
        inner: Arc::new(HandleInner { tx: tx.clone() }),
    };
    let thread = thread::spawn(move || {
        let mut runner = Runner {
            queue,
            ports,
            view,
            tx,
            until_idle,
            running: HashMap::new(),
            stopping: HashSet::new(),
            pending_clean: HashMap::new(),
            starts: 0,
            shutting_down: false,
        };
        while let Ok(msg) = rx.recv() {
            if runner.step(msg) {
                break;
            }
        }
        runner.queue.states()
    });
    (handle, thread)
}

struct Runner {
    queue: Queue,
    ports: Arc<Ports>,
    view: Arc<dyn View>,
    tx: SyncSender<Msg>,
    until_idle: bool,
    running: HashMap<JobId, (u64, Arc<Cancel>)>,
    stopping: HashSet<(JobId, u64)>,
    pending_clean: HashMap<JobId, Vec<PathBuf>>,
    starts: u64,
    shutting_down: bool,
}

impl Runner {
    fn step(&mut self, msg: Msg) -> bool {
        let input = match msg {
            Msg::Shutdown | Msg::Input(Input::Shutdown) => {
                if self.shutting_down {
                    return self.done();
                }
                self.shutting_down = true;
                Input::Shutdown
            }
            Msg::Input(_) if self.shutting_down => return self.done(),
            Msg::Input(input) => input,
            Msg::Worker(id, start, outcome) => {
                if self.stopping.remove(&(id, start)) {
                    let still_busy = self.running.contains_key(&id)
                        || self.stopping.iter().any(|(job, _)| *job == id);
                    if !still_busy && let Some(dirs) = self.pending_clean.remove(&id) {
                        remove_all(&*self.ports.files, &dirs);
                    }
                    return self.done();
                }
                if self
                    .running
                    .get(&id)
                    .is_none_or(|(current, _)| *current != start)
                {
                    return self.done();
                }
                self.running.remove(&id);
                Input::Done(id, outcome)
            }
        };
        for effect in self.queue.handle(input) {
            self.apply(effect);
        }
        self.done()
    }

    fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::Show(update) => self.view.apply(update),
            // An unsaved choice still holds until the window closes.
            Effect::Save(choices) => {
                if let Some(store) = &self.ports.settings {
                    let _ = store.save(&choices);
                }
            }
            Effect::Stop(id) => {
                if let Some((start, cancel)) = self.running.remove(&id) {
                    cancel.cancel();
                    self.stopping.insert((id, start));
                }
            }
            Effect::Clean(id, dirs) => {
                if self.stopping.iter().any(|(job, _)| *job == id) {
                    self.pending_clean.entry(id).or_default().extend(dirs);
                } else {
                    remove_all(&*self.ports.files, &dirs);
                }
            }
            Effect::Start(id, task) => {
                self.pending_clean.remove(&id);
                self.starts += 1;
                let cancel = Arc::new(Cancel::default());
                self.running.insert(id, (self.starts, Arc::clone(&cancel)));
                let worker = Worker {
                    id,
                    start: self.starts,
                    ports: Arc::clone(&self.ports),
                    view: Arc::clone(&self.view),
                    cancel,
                    tx: self.tx.clone(),
                };
                worker.spawn(task);
            }
        }
    }

    fn done(&self) -> bool {
        self.running.is_empty()
            && self.stopping.is_empty()
            && (self.shutting_down || (self.until_idle && self.queue.is_idle()))
    }
}

fn remove_all(files: &dyn Files, dirs: &[PathBuf]) {
    for dir in dirs {
        let _ = files.remove_tree(dir);
    }
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
