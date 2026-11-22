//! The job queue: inputs in, effects out. It owns every job and never blocks, sleeps or touches
//! the world, which keeps its tests free of threads.

use std::path::PathBuf;

use pastegrab_domain::job::{
    FailureKind, JobEvent, JobId, JobState, Permits, Step, Waiting, schedule,
};
use pastegrab_domain::link::{Intent, Link, analyze};
use pastegrab_domain::plan::{FetchPlan, Machine, Section, plan_fetch};
use pastegrab_domain::profile::ProfileKind;

use crate::failure::Failure;
use crate::ports::{Choices, Resolved, Source, ViewUpdate};

/// How often a retryable failure sends a job back to the probe.
const MAX_RETRIES: u8 = 2;

/// The person's choices and the machine's limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The profile for video links.
    pub profile: ProfileKind,
    /// The screen the height cap fits.
    pub machine: Machine,
    /// Where finished files go.
    pub dest: PathBuf,
    /// pastegrab's own folder: probe output, caches, logs.
    pub data: PathBuf,
    /// Steps at once.
    pub permits: Permits,
    /// The highest tutorial version already seen.
    pub tutorial: u32,
}

/// A paste and the choices made with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paste {
    /// The pasted text.
    pub text: String,
    /// A profile for every link in it; music links otherwise default to sound only.
    pub profile: Option<ProfileKind>,
    /// Only this part of each video.
    pub section: Option<Section>,
    /// Each job waits after its probe until `Input::Start`.
    pub hold: bool,
}

/// Everything that reaches the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Text from the clipboard, a drop or the command line.
    Paste(Paste),
    /// Stop a job for good.
    Cancel(JobId),
    /// Hold a job.
    Pause(JobId),
    /// Continue a held job.
    Resume(JobId),
    /// Start a stopped job over.
    Retry(JobId),
    /// Let a held job download.
    Start(JobId),
    /// Another profile; a probed job gets a new plan from the same probe.
    Profile(JobId, ProfileKind),
    /// Another destination folder for jobs that haven't started their download.
    Dest(PathBuf),
    /// The profile later video links start with.
    DefaultProfile(ProfileKind),
    /// The person finished or skipped this tutorial version.
    TutorialSeen(u32),
    /// A worker finished a step.
    Done(JobId, Outcome),
    /// The program is closing; cancel active jobs and clean working folders.
    Shutdown,
}

/// What a worker reports for its step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The probe's answer.
    Probed(Result<Resolved, Failure>),
    /// The downloaded file.
    Fetched(Result<PathBuf, Failure>),
    /// The file in its final place.
    Converted(Result<PathBuf, Failure>),
}

impl Outcome {
    /// The failure outcome for `step`, for a worker that panicked or couldn't start.
    #[must_use]
    pub fn failed(step: Step, failure: Failure) -> Self {
        match step {
            Step::Probe => Self::Probed(Err(failure)),
            Step::Fetch => Self::Fetched(Err(failure)),
            Step::Convert => Self::Converted(Err(failure)),
        }
    }

    const fn step(&self) -> Step {
        match self {
            Self::Probed(_) => Step::Probe,
            Self::Fetched(_) => Step::Fetch,
            Self::Converted(_) => Step::Convert,
        }
    }
}

/// What the runtime does for the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Run a step on a worker.
    Start(JobId, Task),
    /// Cancel the job's running step.
    Stop(JobId),
    /// Remove working folders for a cancelled job once its worker has exited.
    Clean(JobId, Vec<PathBuf>),
    /// Tell the view.
    Show(ViewUpdate),
    /// Keep the person's choices for the next start.
    Save(Choices),
}

/// A step with everything its worker needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// Ask the site.
    Probe(ProbeTask),
    /// Download.
    Fetch(FetchTask),
    /// Inspect, convert if needed, and move into place.
    Convert(ConvertTask),
}

impl Task {
    /// The step this task carries out.
    #[must_use]
    pub const fn step(&self) -> Step {
        match self {
            Self::Probe(_) => Step::Probe,
            Self::Fetch(_) => Step::Fetch,
            Self::Convert(_) => Step::Convert,
        }
    }
}

/// What a probe needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTask {
    /// The link.
    pub link: Link,
    /// The job's own folder for the engine's output.
    pub work_dir: PathBuf,
    /// Impersonate a browser, after a bot filter refused the plain client.
    pub impersonate: bool,
}

/// What a fetch needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchTask {
    /// The formats.
    pub plan: FetchPlan,
    /// Where they come from.
    pub source: Source,
    /// A folder on the destination's volume.
    pub work_dir: PathBuf,
}

/// What the convert step needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertTask {
    /// The downloaded file.
    pub file: PathBuf,
    /// The profile it has to match.
    pub profile: ProfileKind,
    /// The title the file is named after.
    pub title: String,
    /// The name when the title has nothing usable.
    pub fallback: String,
    /// Seconds, for progress.
    pub duration: Option<u32>,
    /// The destination folder.
    pub dest: PathBuf,
    /// Working folders to remove once the file is in place.
    pub cleanup: Vec<PathBuf>,
}

#[derive(Debug)]
struct Job {
    id: JobId,
    link: Link,
    profile: ProfileKind,
    section: Option<Section>,
    state: JobState,
    resolved: Option<Resolved>,
    plan: Option<FetchPlan>,
    fetch_dir: Option<PathBuf>,
    fetched: Option<PathBuf>,
    cleaned: bool,
    retries: u8,
    impersonate: bool,
    held: bool,
}

/// Every job and the rules that move them.
#[derive(Debug)]
pub struct Queue {
    settings: Settings,
    jobs: Vec<Job>,
    next_id: u64,
}

impl Job {
    /// Probed and waiting for the person, or for a profile it can meet.
    fn waits_for_start(&self) -> bool {
        self.state == JobState::Ready && (self.held || self.plan.is_none())
    }
}

impl Queue {
    /// An empty queue.
    #[must_use]
    pub const fn new(settings: Settings) -> Self {
        Self {
            settings,
            jobs: Vec::new(),
            next_id: 1,
        }
    }

    /// Applies `input` and returns what the runtime has to do, steps to start included.
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let mut fx = Vec::new();
        match input {
            Input::Paste(paste) => self.paste(&paste, &mut fx),
            Input::Cancel(id) => self.interrupt(id, JobEvent::Cancel, &mut fx),
            Input::Pause(id) => self.interrupt(id, JobEvent::Pause, &mut fx),
            Input::Resume(id) => {
                self.apply(id, JobEvent::Resume, &mut fx);
            }
            Input::Retry(id) => {
                if let Some(job) = self.job(id) {
                    job.retries = 0;
                    job.cleaned = false;
                }
                self.apply(id, JobEvent::Retry, &mut fx);
            }
            Input::Start(id) => {
                if let Some(job) = self.job(id) {
                    job.held = false;
                }
            }
            Input::Profile(id, profile) => self.reprofile(id, profile, &mut fx),
            Input::Dest(dest) if dest != self.settings.dest => {
                self.settings.dest = dest;
                fx.push(self.choices());
            }
            Input::DefaultProfile(profile) if profile != self.settings.profile => {
                self.settings.profile = profile;
                fx.push(self.choices());
            }
            Input::TutorialSeen(version) if version > self.settings.tutorial => {
                self.settings.tutorial = version;
                fx.push(self.choices());
            }
            Input::Dest(_) | Input::DefaultProfile(_) | Input::TutorialSeen(_) => {}
            Input::Done(id, outcome) => self.done(id, outcome, &mut fx),
            Input::Shutdown => {
                self.shutdown(&mut fx);
                return fx;
            }
        }
        self.start_steps(&mut fx);
        fx
    }

    /// Nothing runs or waits; only paused, held or finished jobs remain.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.jobs.iter().all(|j| {
            j.state.is_over() || matches!(j.state, JobState::Paused(_)) || j.waits_for_start()
        })
    }

    /// Every job's state, in queue order.
    #[must_use]
    pub fn states(&self) -> Vec<(JobId, JobState)> {
        self.jobs.iter().map(|j| (j.id, j.state)).collect()
    }

    fn job(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    fn paste(&mut self, paste: &Paste, fx: &mut Vec<Effect>) {
        let links = analyze(&paste.text);
        if links.is_empty() {
            fx.push(Effect::Show(ViewUpdate::NoLinks));
        }
        for link in links {
            let active = self
                .jobs
                .iter()
                .any(|j| !j.state.is_over() && j.link.dedup_key() == link.dedup_key());
            if active {
                continue;
            }
            let id = JobId(self.next_id);
            self.next_id += 1;
            let profile = paste.profile.unwrap_or(match link.intent {
                Intent::Audio => ProfileKind::Audio,
                Intent::Video => self.settings.profile,
            });
            fx.push(Effect::Show(ViewUpdate::Added {
                id,
                url: link.url.to_string(),
                profile,
            }));
            self.jobs.push(Job {
                id,
                link,
                profile,
                section: paste.section,
                state: JobState::Queued,
                resolved: None,
                plan: None,
                fetch_dir: None,
                fetched: None,
                cleaned: false,
                retries: 0,
                impersonate: false,
                held: paste.hold,
            });
        }
    }

    /// Cancels or pauses a job, stopping its worker first when one runs.
    fn interrupt(&mut self, id: JobId, event: JobEvent, fx: &mut Vec<Effect>) {
        let running = self
            .job(id)
            .is_some_and(|job| job.state.running().is_some());
        if running {
            fx.push(Effect::Stop(id));
        }
        let changed = self.apply(id, event, fx);
        if changed && event == JobEvent::Cancel {
            self.clean_job(id, fx);
        }
    }

    fn clean_job(&mut self, id: JobId, fx: &mut Vec<Effect>) {
        let data = self.settings.data.clone();
        let Some(job) = self.job(id) else {
            return;
        };
        if job.cleaned || job.state == JobState::Done {
            return;
        }
        job.cleaned = true;
        let mut dirs = Vec::with_capacity(2);
        if let Some(dir) = job.fetch_dir.take() {
            dirs.push(dir);
        }
        dirs.push(data.join("jobs").join(id.0.to_string()));
        fx.push(Effect::Clean(id, dirs));
    }

    fn shutdown(&mut self, fx: &mut Vec<Effect>) {
        let ids: Vec<JobId> = self.jobs.iter().map(|j| j.id).collect();
        for id in ids {
            self.interrupt(id, JobEvent::Cancel, fx);
            self.clean_job(id, fx);
        }
    }

    fn apply(&mut self, id: JobId, event: JobEvent, fx: &mut Vec<Effect>) -> bool {
        let Some(job) = self.job(id) else {
            return false;
        };
        let Some(state) = job.state.next(event) else {
            return false;
        };
        job.state = state;
        fx.push(Effect::Show(ViewUpdate::State { id, state }));
        true
    }

    fn done(&mut self, id: JobId, outcome: Outcome, fx: &mut Vec<Effect>) {
        let machine = self.settings.machine;
        let Some(job) = self.job(id) else {
            return;
        };
        // A step that finishes after a cancel or a pause has nothing to report.
        if job.state.running() != Some(outcome.step()) {
            return;
        }
        match outcome {
            Outcome::Probed(Ok(resolved)) => {
                let profile = job.profile;
                let planned = plan_fetch(&resolved.info, &profile.profile(), machine, job.section);
                let title = resolved.info.title.clone();
                let duration = resolved.info.duration;
                job.resolved = Some(resolved);
                match planned {
                    Ok(plan) => {
                        job.plan = Some(plan.clone());
                        let update = ViewUpdate::Planned {
                            id,
                            title,
                            profile,
                            duration,
                            plan,
                        };
                        fx.push(Effect::Show(update));
                    }
                    // A held job waits for another profile instead of failing.
                    Err(reason) if job.held => {
                        let update = ViewUpdate::NoPlan {
                            id,
                            title,
                            profile,
                            duration,
                            reason,
                        };
                        fx.push(Effect::Show(update));
                    }
                    Err(e) => {
                        let failure = Failure::new(FailureKind::NoFormat, e.to_string());
                        return self.fail(id, failure, fx);
                    }
                }
                self.apply(id, JobEvent::Finish(Step::Probe), fx);
            }
            Outcome::Fetched(Ok(file)) => {
                job.fetched = Some(file);
                self.apply(id, JobEvent::Finish(Step::Fetch), fx);
            }
            Outcome::Converted(Ok(file)) => {
                self.apply(id, JobEvent::Finish(Step::Convert), fx);
                fx.push(Effect::Show(ViewUpdate::Finished { id, file }));
            }
            Outcome::Probed(Err(f)) | Outcome::Fetched(Err(f)) | Outcome::Converted(Err(f)) => {
                self.fail(id, f, fx);
            }
        }
    }

    /// Fails the running step, or sends the job back to the probe when the policy retries.
    fn fail(&mut self, id: JobId, failure: Failure, fx: &mut Vec<Effect>) {
        let Some(job) = self.job(id) else {
            return;
        };
        let retry = match failure.kind {
            FailureKind::BotWall if !job.impersonate => {
                job.impersonate = true;
                true
            }
            FailureKind::Expired | FailureKind::Network if job.retries < MAX_RETRIES => {
                job.retries += 1;
                true
            }
            _ => false,
        };
        self.apply(id, JobEvent::Fail(failure.kind), fx);
        if retry {
            self.apply(id, JobEvent::Retry, fx);
        } else {
            fx.push(Effect::Show(ViewUpdate::Failed { id, failure }));
        }
    }

    fn choices(&self) -> Effect {
        Effect::Save(Choices {
            dest: Some(self.settings.dest.clone()),
            profile: Some(self.settings.profile),
            tutorial: (self.settings.tutorial > 0).then_some(self.settings.tutorial),
        })
    }

    fn reprofile(&mut self, id: JobId, profile: ProfileKind, fx: &mut Vec<Effect>) {
        let machine = self.settings.machine;
        let Some(job) = self.job(id) else {
            return;
        };
        let probed = match job.state {
            JobState::Queued | JobState::Probing | JobState::Paused(Waiting::Queued) => false,
            JobState::Ready | JobState::Paused(Waiting::Ready) => true,
            _ => return,
        };
        job.profile = profile;
        let Some(resolved) = job.resolved.as_ref().filter(|_| probed) else {
            return;
        };
        let title = resolved.info.title.clone();
        let duration = resolved.info.duration;
        let update = match plan_fetch(&resolved.info, &profile.profile(), machine, job.section) {
            Ok(plan) => {
                job.plan = Some(plan.clone());
                ViewUpdate::Planned {
                    id,
                    title,
                    profile,
                    duration,
                    plan,
                }
            }
            Err(reason) => {
                job.plan = None;
                ViewUpdate::NoPlan {
                    id,
                    title,
                    profile,
                    duration,
                    reason,
                }
            }
        };
        fx.push(Effect::Show(update));
    }

    fn start_steps(&mut self, fx: &mut Vec<Effect>) {
        let offered: Vec<(JobId, JobState)> = self
            .jobs
            .iter()
            .filter(|j| !j.waits_for_start())
            .map(|j| (j.id, j.state))
            .collect();
        for (id, step) in schedule(&offered, self.settings.permits) {
            let task = self.task(id, step);
            self.apply(id, JobEvent::Start(step), fx);
            if let Some(task) = task {
                fx.push(Effect::Start(id, task));
            } else {
                self.fail(id, Failure::internal(format!("no input for {step:?}")), fx);
            }
        }
    }

    fn task(&mut self, id: JobId, step: Step) -> Option<Task> {
        let data = self.settings.data.clone();
        let dest = self.settings.dest.clone();
        let job = self.jobs.iter_mut().find(|j| j.id == id)?;
        let name = id.0.to_string();
        let probe_dir = data.join("jobs").join(&name);
        // One folder per job, with no shared parent left behind in the destination.
        let fetch_dir = dest.join(format!(".pastegrab-{name}"));
        let task = match step {
            Step::Probe => Task::Probe(ProbeTask {
                link: job.link.clone(),
                work_dir: probe_dir,
                impersonate: job.impersonate,
            }),
            Step::Fetch => {
                let plan = job.plan.clone()?;
                let source = job.resolved.as_ref()?.source.clone();
                job.fetch_dir = Some(fetch_dir.clone());
                Task::Fetch(FetchTask {
                    plan,
                    source,
                    work_dir: fetch_dir,
                })
            }
            Step::Convert => {
                let resolved = job.resolved.as_ref()?;
                let fetch_dir = job.fetch_dir.clone().unwrap_or(fetch_dir);
                Task::Convert(ConvertTask {
                    file: job.fetched.clone()?,
                    profile: job.profile,
                    title: resolved.info.title.clone(),
                    fallback: job.link.id.clone().unwrap_or_else(|| "video".to_owned()),
                    duration: resolved.info.duration,
                    dest,
                    cleanup: vec![fetch_dir, probe_dir],
                })
            }
        };
        Some(task)
    }
}
