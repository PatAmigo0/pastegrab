//! Every string the person sees, one `Strings` per language. A language that misses a field
//! doesn't compile, and messages with numbers are functions.

#![forbid(unsafe_code)]

mod ru;

pub use ru::RU;

/// The text of one language.
#[derive(Debug)]
pub struct Strings {
    /// The hint inside the empty link field.
    pub paste_hint: &'static str,
    /// The button beside the link field.
    pub analyze: &'static str,
    /// The card's main button.
    pub download: &'static str,
    /// Before the destination folder.
    pub save_to: &'static str,
    /// The button that picks another folder.
    pub change_folder: &'static str,
    /// The job list's columns.
    pub columns: Columns,
    /// A paste without a link.
    pub no_links: &'static str,
    /// Profile names, in the format list's order.
    pub profiles: [&'static str; 6],
    /// The detail line under each profile, in the same order.
    pub profile_details: [&'static str; 6],
    /// A job's state.
    pub states: States,
    /// Why a job stopped, one line each.
    pub failures: Failures,
    /// Why the chosen profile finds nothing in this source.
    pub no_plan: NoPlan,
    /// "about 1.8 GB" from bytes.
    pub size: fn(u64) -> String,
    /// "2.1 MB/s" from bytes per second.
    pub speed: fn(u64) -> String,
    /// "2:41:04" from seconds.
    pub duration: fn(u32) -> String,
}

/// The job list's columns.
#[derive(Debug)]
pub struct Columns {
    /// The title.
    pub title: &'static str,
    /// The state and progress.
    pub state: &'static str,
}

/// A job's state as one word or two.
#[derive(Debug)]
pub struct States {
    /// Waiting for its turn.
    pub queued: &'static str,
    /// The site is being asked about the link.
    pub probing: &'static str,
    /// Probed, waiting for the download button.
    pub ready: &'static str,
    /// Bytes coming in.
    pub downloading: &'static str,
    /// Video and sound joined into one file.
    pub merging: &'static str,
    /// ffmpeg at work.
    pub converting: &'static str,
    /// The file is in place.
    pub done: &'static str,
    /// Held by the person.
    pub paused: &'static str,
    /// Stopped by the person.
    pub cancelled: &'static str,
}

/// One line per failure kind.
#[derive(Debug)]
#[expect(
    missing_docs,
    reason = "each field is the failure kind of the same name"
)]
pub struct Failures {
    pub engine_outdated: &'static str,
    pub bot_wall: &'static str,
    pub bot_check: &'static str,
    pub needs_login: &'static str,
    pub unavailable: &'static str,
    pub not_started: &'static str,
    pub unsupported: &'static str,
    pub expired: &'static str,
    pub network: &'static str,
    pub disk_full: &'static str,
    pub tool_broken: &'static str,
    pub no_format: &'static str,
    pub internal: &'static str,
}

/// Why a profile can't be met.
#[derive(Debug)]
#[expect(missing_docs, reason = "each field is the plan error of the same name")]
pub struct NoPlan {
    pub no_usable_format: &'static str,
    pub no_video: &'static str,
    pub no_audio: &'static str,
}
