//! The fakes keep the same contracts as the adapters.

use std::path::{Path, PathBuf};

use pastegrab_app::ports::ProcSpec;
use pastegrab_testkit::contract::{self, Programs};
use pastegrab_testkit::{MemFiles, MemSettings, ScriptedRunner, Transcript};

#[test]
fn mem_files_keep_the_files_contract() {
    let files = MemFiles::default();
    contract::files(&files, Path::new("/videos"), &|path| files.create(path));
}

#[test]
fn mem_settings_keep_the_settings_contract() {
    contract::settings(&MemSettings::default());
}

#[test]
fn the_scripted_runner_keeps_the_runner_contract() {
    let spec = |program: &str| ProcSpec {
        program: PathBuf::from(program),
        ..ProcSpec::default()
    };
    let runner = ScriptedRunner::default()
        .script("talk", Transcript::new(&["out", "err"], 3))
        .script("hang", Transcript::new(&["ready"], 0).hanging());
    let programs = Programs {
        talk: spec("talk"),
        hang: spec("hang"),
        missing: spec("missing"),
    };
    contract::process_runner(&runner, &programs);
}
