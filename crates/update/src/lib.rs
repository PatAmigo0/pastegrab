//! App self-update without I/O: which signed release to install, and which installed version
//! the launcher starts. The apps fetch the files and keep `state.json`.

#![forbid(unsafe_code)]

mod launch;
mod manifest;
mod version;

pub use launch::{Launch, Start};
pub use manifest::{Offer, Package, Refusal, Updater};
pub use version::Version;
