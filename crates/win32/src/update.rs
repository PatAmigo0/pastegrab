//! Pure update status types, height-cap choices and Settings dialog text formatting.

#![forbid(unsafe_code)]

use pastegrab_domain::profile::HeightCap;
use pastegrab_i18n::SettingsDialog;

/// Video quality choices in the Settings dropdown, in display order.
pub const HEIGHT_CAPS: [HeightCap; 5] = [
    HeightCap::Screen,
    HeightCap::AtMost(480),
    HeightCap::AtMost(720),
    HeightCap::AtMost(1080),
    HeightCap::Unlimited,
];

/// What the updater knows about the current install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// Currently running version string.
    pub current: String,
    /// Newer version already downloaded and staged for the next launch, if any.
    pub staged: Option<String>,
    /// Unix timestamp of the last successful update check.
    pub last_checked: Option<u64>,
    /// Unix timestamp when the staged update (or current version) was downloaded.
    pub last_downloaded: Option<u64>,
    /// Outcome shown on the status line.
    pub state: UpdateState,
}

/// High-level update state for the Settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateState {
    /// No newer version is available.
    UpToDate,
    /// A newer version is downloaded and waiting for restart.
    Ready,
    /// Checking or downloading the update failed.
    Failed,
    /// Running outside an installed `app\<version>` folder.
    NotInstalled,
}

/// Port the Settings dialog calls to read update status and trigger a manual check.
pub trait UpdateControl: Send + Sync {
    /// Current status from local state on disk (fast, no network).
    fn status(&self) -> UpdateInfo;

    /// Checks the release server now, staging any newer release, and returns updated status.
    fn check(&self) -> UpdateInfo;
}

/// Fallback updater for environments without update wiring.
#[derive(Debug, Clone)]
pub struct UnavailableUpdater {
    current: String,
}

impl UnavailableUpdater {
    /// Reports updates unavailable for `current`.
    #[must_use]
    pub fn new(current: impl Into<String>) -> Self {
        Self {
            current: current.into(),
        }
    }
}

impl UpdateControl for UnavailableUpdater {
    fn status(&self) -> UpdateInfo {
        UpdateInfo {
            current: self.current.clone(),
            staged: None,
            last_checked: None,
            last_downloaded: None,
            state: UpdateState::NotInstalled,
        }
    }

    fn check(&self) -> UpdateInfo {
        self.status()
    }
}

/// Index in [`HEIGHT_CAPS`] matching `cap`, defaulting to `0` (`Screen`).
#[must_use]
pub fn height_cap_index(cap: HeightCap) -> usize {
    HEIGHT_CAPS
        .iter()
        .position(|&item| item == cap)
        .unwrap_or(0)
}

/// Height cap at `index` in [`HEIGHT_CAPS`], defaulting to `HeightCap::Screen`.
#[must_use]
pub fn height_cap_at(index: usize) -> HeightCap {
    HEIGHT_CAPS.get(index).copied().unwrap_or(HeightCap::Screen)
}

/// Formats local calendar parts as `DD.MM.YYYY в HH:MM`.
#[must_use]
pub fn format_date_time(day: u16, month: u16, year: u16, hour: u16, minute: u16) -> String {
    format!("{day:02}.{month:02}.{year:04} в {hour:02}:{minute:02}")
}

/// Version line text.
#[must_use]
pub fn version_line(info: &UpdateInfo, s: &SettingsDialog) -> String {
    (s.version)(&info.current)
}

/// Last-checked line text.
#[must_use]
pub fn checked_line(
    info: &UpdateInfo,
    format_time: impl FnOnce(u64) -> String,
    s: &SettingsDialog,
) -> String {
    let when = info
        .last_checked
        .map_or_else(|| s.never.to_owned(), format_time);
    (s.last_checked)(&when)
}

/// Last-downloaded/installed update line text.
#[must_use]
pub fn downloaded_line(
    info: &UpdateInfo,
    format_time: impl FnOnce(u64) -> String,
    s: &SettingsDialog,
) -> String {
    let when = info
        .last_downloaded
        .map_or_else(|| s.unknown.to_owned(), format_time);
    (s.last_downloaded)(&when)
}

/// Status line text.
#[must_use]
pub fn status_text(info: &UpdateInfo, s: &SettingsDialog) -> String {
    match info.state {
        UpdateState::UpToDate => s.up_to_date.to_owned(),
        UpdateState::Ready => info
            .staged
            .as_deref()
            .map_or_else(|| s.up_to_date.to_owned(), s.ready),
        UpdateState::Failed => s.failed.to_owned(),
        UpdateState::NotInstalled => s.not_installed.to_owned(),
    }
}

/// Whether the manual check button should be enabled.
#[must_use]
pub const fn can_check(info: &UpdateInfo) -> bool {
    !matches!(info.state, UpdateState::NotInstalled)
}

/// Whether the restart-to-update button should be enabled.
#[must_use]
pub const fn can_restart(info: &UpdateInfo) -> bool {
    info.staged.is_some()
}

#[cfg(test)]
mod tests {
    use pastegrab_domain::profile::HeightCap;
    use pastegrab_i18n::RU;

    use super::{
        HEIGHT_CAPS, UnavailableUpdater, UpdateControl, UpdateInfo, UpdateState, can_check,
        can_restart, checked_line, downloaded_line, format_date_time, height_cap_at,
        height_cap_index, status_text, version_line,
    };

    #[test]
    fn height_cap_round_trips_for_every_choice() {
        for (index, cap) in HEIGHT_CAPS.into_iter().enumerate() {
            assert_eq!(height_cap_index(cap), index);
            assert_eq!(height_cap_at(index), cap);
        }
        assert_eq!(height_cap_index(HeightCap::AtMost(360)), 0);
        assert_eq!(height_cap_at(99), HeightCap::Screen);
    }

    #[test]
    fn update_lines_and_buttons_follow_state() {
        let s = &RU.settings;
        let ready = UpdateInfo {
            current: "0.2.0".into(),
            staged: Some("0.2.1".into()),
            last_checked: Some(1_700_000_000),
            last_downloaded: Some(1_700_000_120),
            state: UpdateState::Ready,
        };
        assert_eq!(version_line(&ready, s), "Версия: 0.2.0");
        assert_eq!(
            checked_line(&ready, |_| format_date_time(5, 9, 2026, 8, 4), s),
            "Последняя проверка: 05.09.2026 в 08:04"
        );
        assert_eq!(
            downloaded_line(&ready, |_| format_date_time(5, 9, 2026, 8, 6), s),
            "Последнее обновление: 05.09.2026 в 08:06"
        );
        assert_eq!(
            status_text(&ready, s),
            "Обновление 0.2.1 готово к установке"
        );
        assert!(can_check(&ready));
        assert!(can_restart(&ready));

        let unavail = UnavailableUpdater::new("0.2.0").status();
        assert_eq!(
            checked_line(&unavail, |_| String::new(), s),
            "Последняя проверка: ещё не проверялось"
        );
        assert_eq!(
            downloaded_line(&unavail, |_| String::new(), s),
            "Последнее обновление: неизвестно"
        );
        assert_eq!(status_text(&unavail, s), RU.settings.not_installed);
        assert!(!can_check(&unavail));
        assert!(!can_restart(&unavail));
    }
}
