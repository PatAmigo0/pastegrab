//! What setup does once he says yes: unpack, register, add the runtime Windows may lack, test
//! every tool, and keep what it learned in `deploy.json`.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use pastegrab_deploy::machine::{self, Deployment};
use pastegrab_deploy::{Entry, launch, selftest};
use pastegrab_i18n::RU;

use crate::report;

/// The step the dialog's line names.
pub(crate) const UNPACKING: u8 = 0;
pub(crate) const RUNTIME: u8 = 1;
pub(crate) const CHECKING: u8 = 2;

/// An install that went through, and how well its tools work.
pub(crate) struct Finished {
    /// The report under «Подробнее о компьютере».
    pub(crate) report: String,
    /// A tool failed its self-test, or Windows still lacks the runtime.
    pub(crate) broken: bool,
}

/// What the work needs from the dialog.
pub(crate) struct Job<'a> {
    pub(crate) payload: &'a [u8],
    pub(crate) redist: &'a [u8],
    pub(crate) dest: &'a Path,
    pub(crate) size_kib: u32,
    /// Compressed bytes unpacked so far, for the progress bar.
    pub(crate) read: &'a AtomicU64,
    /// The step under way.
    pub(crate) step: &'a AtomicU8,
}

/// Does it all, in order.
///
/// # Errors
///
/// A step that leaves no working install: unpacking, the launcher's state or the shortcuts.
pub(crate) fn run(job: &Job<'_>) -> io::Result<Finished> {
    pastegrab_deploy::install(job.payload, job.dest, job.read)?;
    let state = launch::state(job.dest)
        .ok_or_else(|| io::Error::other("the payload carries no version of pastegrab"))?;
    launch::write_state(job.dest, &state)?;
    pastegrab_deploy::register(&Entry {
        exe: &program(job.dest),
        folder: job.dest,
        version: env!("CARGO_PKG_VERSION"),
        size_kib: job.size_kib,
        description: RU.setup.shortcut,
    })?;
    let mut found = pastegrab_deploy::probe(job.dest);
    if !found.ucrt && !job.redist.is_empty() {
        job.step.store(RUNTIME, Ordering::Relaxed);
        add_runtime(job.redist);
        found = pastegrab_deploy::probe(job.dest);
    }
    job.step.store(CHECKING, Ordering::Relaxed);
    let tuning = found.tuning();
    let sidecars = pastegrab_deploy::bundled(&job.dest.join("tools")).map_err(io::Error::other)?;
    let checks = selftest::run(&sidecars, tuning.preset);
    let tools = checks
        .iter()
        .filter_map(|check| {
            let version = check.result.as_ref().ok().filter(|v| !v.is_empty())?;
            Some((report::tool(check.probe).to_owned(), version.clone()))
        })
        .collect();
    let text = report::report(&found, tuning, &checks, &RU);
    let broken = !report::all_work(&checks) || !found.ucrt;
    let deployment = Deployment {
        machine: found,
        tuning,
        tools,
    };
    if let Some(data) = pastegrab_deploy::data_folder() {
        machine::write(&data, &deployment)?;
    }
    Ok(Finished {
        report: text,
        broken,
    })
}

/// The launcher, which starts the version `state.json` names.
pub(crate) fn program(dest: &Path) -> PathBuf {
    dest.join("pastegrab.exe")
}

/// Runs the VC++ redistributable behind the UAC prompt. Its outcome shows in the next probe:
/// a declined prompt or a failed install leaves the runtime missing, and the report says so.
fn add_runtime(redist: &[u8]) {
    let file = env::temp_dir().join("pastegrab-vc_redist.x64.exe");
    if fs::write(&file, redist).is_ok() {
        let _ = pastegrab_deploy::run_elevated(&file, "/install /passive /norestart");
    }
    let _ = fs::remove_file(&file);
}
