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
    /// The button that drops finished, failed and cancelled jobs from the list.
    pub clear: &'static str,
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
    /// When the launcher finds no version to start, above the technical reason.
    pub cannot_start: &'static str,
    /// The tray notification for a job that ended while the window was in the background.
    pub notices: Notices,
    /// The installer.
    pub setup: Setup,
    /// `pastegrab.exe --uninstall`, from Programs and Features.
    pub uninstall: Uninstall,
}

/// The uninstaller's text.
#[derive(Debug)]
pub struct Uninstall {
    /// The title of its message boxes.
    pub title: &'static str,
    /// The question, with what goes and what stays.
    pub ask: &'static str,
    /// While the window is open.
    pub close_first: &'static str,
    /// When everything is gone.
    pub done: &'static str,
    /// Before the error when something stayed.
    pub failed: &'static str,
}

/// Tray notification headings; the video's title goes under them.
#[derive(Debug)]
pub struct Notices {
    /// The file is in place.
    pub done: &'static str,
    /// The job stopped with a failure, whose line follows the title.
    pub failed: &'static str,
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
    /// Where the shortcuts and the folder are, under it.
    pub done_where: fn(&str) -> String,
    /// The button that starts the installed program.
    pub open: &'static str,
    /// The shortcut's tooltip.
    pub shortcut: &'static str,
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
    /// The line under the heading while the VC++ redistributable runs.
    pub runtime: &'static str,
    /// The line under the heading while the self-test runs.
    pub checking: &'static str,
    /// The heading when the install worked but a tool failed its self-test.
    pub done_broken: &'static str,
    /// The button that shows the report.
    pub details: &'static str,
    /// The report under that button.
    pub report: Report,
}

/// Setup's report about the PC and its self-test.
#[derive(Debug)]
pub struct Report {
    /// The Windows line from the version's name and the build.
    pub windows: fn(&str, &str) -> String,
    /// The CPU line from its name, threads and features.
    pub cpu: fn(&str, u32, &str) -> String,
    /// Installed memory from MiB.
    pub memory: fn(u64) -> String,
    /// Free space in MiB and whether the system disk spins.
    pub disk: fn(u64, Option<bool>) -> String,
    /// The screen in pixels.
    pub screen: fn(u32, u32) -> String,
    /// A tool that answered, from its name with version and tenths of a second.
    pub works: fn(&str, u64) -> String,
    /// The test encode, from tenths of a second.
    pub encode: fn(u64) -> String,
    /// A tool that failed, from its name and the reason.
    pub broken: fn(&str, &str) -> String,
    /// The defaults, from downloads at once and the x264 preset.
    pub tuning: fn(u8, &str) -> String,
    /// Windows lacks the runtime the Python under yt-dlp needs.
    pub no_ucrt: &'static str,
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
