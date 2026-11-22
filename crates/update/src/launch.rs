//! The launcher's memory: the version it starts, the one before it, and a new version's trial.
//! A version that twice fails to open its window is rolled back and never offered again.

use serde::{Deserialize, Serialize};

use crate::Version;

/// Starts a version on trial may take without opening its window.
const TRIES: u8 = 2;

/// What the launcher keeps in `state.json` between starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    current: Version,
    #[serde(default)]
    previous: Option<Version>,
    /// Starts of `current` since it was staged, until it opens its window.
    #[serde(default)]
    trial: Option<u8>,
    #[serde(default)]
    bad: Vec<Version>,
}

/// Which version a start runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// The current version.
    Run(Version),
    /// `failed` never opened its window, and `run` replaces it.
    RolledBack {
        /// The version that runs now.
        run: Version,
        /// The version rolled back.
        failed: Version,
    },
}

impl Launch {
    /// An install with one version.
    #[must_use]
    pub const fn new(current: Version) -> Self {
        Self {
            current,
            previous: None,
            trial: None,
            bad: Vec::new(),
        }
    }

    /// A downloaded version, which runs on trial from the next start.
    pub fn stage(&mut self, version: Version) {
        self.previous = Some(self.current);
        self.current = version;
        self.trial = Some(0);
    }

    /// Counts a start of a version on trial and rolls it back once it has failed `TRIES` times.
    pub fn start(&mut self) -> Start {
        match (self.trial, self.previous) {
            (Some(tries), Some(previous)) if tries >= TRIES => {
                let failed = self.current;
                self.bad.push(failed);
                self.current = previous;
                self.previous = None;
                self.trial = None;
                Start::RolledBack {
                    run: previous,
                    failed,
                }
            }
            (Some(tries), _) => {
                self.trial = Some(tries.saturating_add(1));
                Start::Run(self.current)
            }
            (None, _) => Start::Run(self.current),
        }
    }

    /// The running version opened its window, which ends its trial.
    pub const fn confirm(&mut self) {
        self.trial = None;
    }

    /// The version starts run.
    #[must_use]
    pub const fn current(&self) -> Version {
        self.current
    }

    /// Versions rolled back on this PC, which updates skip.
    #[must_use]
    pub fn bad(&self) -> &[Version] {
        &self.bad
    }
}

#[cfg(test)]
mod tests {
    use super::{Launch, Start};
    use crate::Version;

    fn v(text: &str) -> Version {
        text.parse().expect("a version")
    }

    #[test]
    fn a_new_version_that_opens_its_window_stays() {
        let mut launch = Launch::new(v("0.1.0"));
        launch.stage(v("0.2.0"));
        assert_eq!(launch.start(), Start::Run(v("0.2.0")));
        launch.confirm();
        for _ in 0..3 {
            assert_eq!(launch.start(), Start::Run(v("0.2.0")));
        }
        assert_eq!(launch.bad(), []);
    }

    #[test]
    fn a_new_version_that_fails_twice_is_rolled_back_and_marked_bad() {
        let mut launch = Launch::new(v("0.1.0"));
        launch.stage(v("0.2.0"));
        assert_eq!(launch.start(), Start::Run(v("0.2.0")));
        assert_eq!(launch.start(), Start::Run(v("0.2.0")));
        assert_eq!(
            launch.start(),
            Start::RolledBack {
                run: v("0.1.0"),
                failed: v("0.2.0")
            }
        );
        assert_eq!(launch.start(), Start::Run(v("0.1.0")));
        assert_eq!(launch.bad(), [v("0.2.0")]);
    }

    #[test]
    fn the_state_survives_json() {
        let mut launch = Launch::new(v("0.1.0"));
        launch.stage(v("0.2.0"));
        let _ = launch.start();
        let json = serde_json::to_string(&launch).expect("json");
        let back: Launch = serde_json::from_str(&json).expect("the same state");
        assert_eq!(back, launch);
    }
}
