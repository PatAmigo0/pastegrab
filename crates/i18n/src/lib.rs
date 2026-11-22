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
    /// The job list's right-click menu.
    pub menu: Menu,
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
    /// After the first title when several videos wait: "and 2 more".
    pub more: fn(usize) -> String,
    /// The window's title.
    pub window_title: &'static str,
    /// The installer.
    pub setup: Setup,
}

/// The installer's text.
#[derive(Debug)]
pub struct Setup {
    /// The window title.
    pub title: &'static str,
    /// The question on the first page.
    pub ask: &'static str,
    /// The size and folder under the question, from "about 248 MB" and the folder.
    pub where_to: fn(&str, &str) -> String,
    /// The button that starts the install.
    pub install: &'static str,
    /// The heading while files unpack.
    pub installing: &'static str,
    /// The line under it.
    pub unpacking: &'static str,
    /// The heading when it worked.
    pub done: &'static str,
    /// The folder line under it.
    pub done_where: fn(&str) -> String,
    /// The heading when it failed.
    pub failed: &'static str,
    /// The advice under the error.
    pub failed_advice: &'static str,
    /// A 32-bit Windows.
    pub needs_64_bit: &'static str,
    /// Windows 7 or older.
    pub needs_windows_8: &'static str,
    /// A build of the installer without the program's files.
    pub empty: &'static str,
}

/// The job list's columns.
#[derive(Debug)]
pub struct Columns {
    /// The title.
    pub title: &'static str,
    /// The state and progress.
    pub state: &'static str,
}

/// The job list's right-click menu.
#[derive(Debug)]
pub struct Menu {
    /// Opens the finished file.
    pub open: &'static str,
    /// Shows the file in its folder.
    pub show_in_folder: &'static str,
    /// Starts a failed or cancelled job again.
    pub retry: &'static str,
    /// Stops a job.
    pub cancel: &'static str,
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
