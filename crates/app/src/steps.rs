//! What a worker does for each step: plain functions over the ports, which tests drive with fakes.

use std::io;
use std::path::PathBuf;

use pastegrab_domain::job::FailureKind;
use pastegrab_domain::name::{MAX_PATH_UNITS, output_name};
use pastegrab_domain::plan::plan_convert;

use crate::cancel::Cancel;
use crate::failure::Failure;
use crate::ports::{
    FetchRequest, Fetcher, Files, MediaProbe, ProgressSink, Resolved, Resolver, Transcoder,
};
use crate::queue::{ConvertTask, FetchTask, ProbeTask};

/// The adapters steps run with, wired once in the composition root.
pub struct Ports {
    /// Tried in order; the first that accepts a link resolves it.
    pub resolvers: Vec<Box<dyn Resolver>>,
    /// Tried in order; the first that accepts a source fetches it.
    pub fetchers: Vec<Box<dyn Fetcher>>,
    /// Reads fetched files.
    pub probe: Box<dyn MediaProbe>,
    /// Converts them.
    pub transcoder: Box<dyn Transcoder>,
    /// Places them.
    pub files: Box<dyn Files>,
}

/// Asks the first resolver that accepts the link.
///
/// # Errors
///
/// `Unsupported` when no resolver accepts it, otherwise the resolver's failure.
pub fn probe(ports: &Ports, task: &ProbeTask, cancel: &Cancel) -> Result<Resolved, Failure> {
    let resolver = ports
        .resolvers
        .iter()
        .find(|r| r.accepts(&task.link))
        .ok_or_else(|| {
            Failure::new(
                FailureKind::Unsupported,
                format!("no resolver for {}", task.link.url),
            )
        })?;
    resolver.resolve(&task.link, &task.work_dir, task.impersonate, cancel)
}

/// Downloads with the first fetcher that accepts the source.
///
/// # Errors
///
/// The fetcher's failure, or `Internal` when none accepts the source.
pub fn fetch(
    ports: &Ports,
    task: &FetchTask,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
) -> Result<PathBuf, Failure> {
    let fetcher = ports
        .fetchers
        .iter()
        .find(|f| f.accepts(&task.source))
        .ok_or_else(|| Failure::internal("no fetcher for this source"))?;
    let request = FetchRequest {
        plan: &task.plan,
        source: &task.source,
        work_dir: &task.work_dir,
    };
    fetcher.fetch(request, progress, cancel)
}

/// Inspects the fetched file, converts it when the profile needs that, and places it under a
/// free name in the destination. Working folders go once the file is in place.
///
/// # Errors
///
/// The probe's or the transcoder's failure, `DiskFull`, or `Internal`.
pub fn convert(
    ports: &Ports,
    task: &ConvertTask,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
) -> Result<PathBuf, Failure> {
    let found = ports.probe.probe(&task.file, cancel)?;
    let profile = task.profile.profile();
    let ext = profile.container.extension();
    let folder_units = task.dest.to_string_lossy().encode_utf16().count();
    let budget = MAX_PATH_UNITS.saturating_sub(folder_units + 1);
    let name = output_name(&task.title, &task.fallback, ext, budget, |name| {
        ports.files.exists(&task.dest.join(name))
    })
    .ok_or_else(|| Failure::internal("the destination path leaves no room for a file name"))?;
    let target = task.dest.join(name);
    let placed = match plan_convert(&found, &profile, None) {
        None => ports.files.place(&task.file, &target),
        Some(plan) => {
            let converted = task.file.with_file_name(format!("converted.{ext}"));
            ports.transcoder.transcode(
                &task.file,
                &converted,
                &plan,
                task.duration,
                progress,
                cancel,
            )?;
            ports.files.place(&converted, &target)
        }
    };
    placed.map_err(|e| io_failure(&e, "placing the file"))?;
    for dir in &task.cleanup {
        // A leftover folder costs disk space, not the download; the sweep at start catches it.
        let _ = ports.files.remove_tree(dir);
    }
    Ok(target)
}

fn io_failure(e: &io::Error, doing: &str) -> Failure {
    let kind = if e.kind() == io::ErrorKind::StorageFull {
        FailureKind::DiskFull
    } else {
        FailureKind::Internal
    };
    Failure::new(kind, format!("{doing}: {e}"))
}
