//! The first-run guide: five skippable steps as static data, plus a presenter that tracks the
//! active step. Raising `VERSION` shows the guide once more after an update adds steps.

/// The current guide version; `settings.json` stores the highest version the person saw.
pub const VERSION: u32 = 1;

/// Which control a step outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// No single control; the step describes the whole flow.
    None,
    /// The link box.
    Link,
    /// The profile selector and download button.
    Card,
    /// The job list.
    List,
    /// The destination folder and its change button.
    Folder,
}

/// Which message a step shows; the view turns it into the person's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// Copy a link in the browser and switch back.
    Clipboard,
    /// Paste into the link box.
    Paste,
    /// Pick a profile and start the download.
    Choose,
    /// Watch progress, cancel or open finished videos in the list.
    List,
    /// Where files land and how to replay the guide.
    Folder,
}

/// One step of the first-run guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Control to outline while this step is active.
    pub target: Target,
    /// Message key for this step.
    pub prompt: Prompt,
}

/// All five steps, in order.
pub const STEPS: [Step; 5] = [
    Step {
        target: Target::None,
        prompt: Prompt::Clipboard,
    },
    Step {
        target: Target::Link,
        prompt: Prompt::Paste,
    },
    Step {
        target: Target::Card,
        prompt: Prompt::Choose,
    },
    Step {
        target: Target::List,
        prompt: Prompt::List,
    },
    Step {
        target: Target::Folder,
        prompt: Prompt::Folder,
    },
];

/// Tracks which step of the guide is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tutorial {
    index: Option<usize>,
}

impl Tutorial {
    /// Opens at step 1 when `seen` is older than `VERSION`, or starts closed.
    #[must_use]
    pub const fn for_seen(seen: u32) -> Self {
        Self {
            index: if seen < VERSION { Some(0) } else { None },
        }
    }

    /// The active step, its 1-based position and the total count, or `None` when closed.
    #[must_use]
    pub const fn current(&self) -> Option<(Step, usize, usize)> {
        let Some(index) = self.index else {
            return None;
        };
        Some((STEPS[index], index + 1, STEPS.len()))
    }

    /// Whether the guide is showing.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.index.is_some()
    }

    /// Whether the active step is the last one.
    #[must_use]
    pub const fn is_last(&self) -> bool {
        matches!(self.index, Some(index) if index + 1 == STEPS.len())
    }

    /// Advances one step; returns `true` when that finishes the guide.
    pub const fn next(&mut self) -> bool {
        let Some(index) = self.index else {
            return false;
        };
        if index + 1 < STEPS.len() {
            self.index = Some(index + 1);
            false
        } else {
            self.index = None;
            true
        }
    }

    /// Closes the guide right away; returns `true` when it was open.
    pub const fn skip(&mut self) -> bool {
        self.index.take().is_some()
    }

    /// Starts the guide again from step 1.
    pub const fn replay(&mut self) {
        self.index = Some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{Prompt, STEPS, Target, Tutorial, VERSION};

    #[test]
    fn first_start_walks_all_five_steps_and_finishes() {
        let mut guide = Tutorial::for_seen(0);
        assert!(guide.is_open());
        let expected = [
            (Target::None, Prompt::Clipboard),
            (Target::Link, Prompt::Paste),
            (Target::Card, Prompt::Choose),
            (Target::List, Prompt::List),
            (Target::Folder, Prompt::Folder),
        ];
        for (i, (target, prompt)) in expected.into_iter().enumerate() {
            let (step, number, total) = guide.current().expect("open");
            assert_eq!((step.target, step.prompt), (target, prompt));
            assert_eq!((number, total), (i + 1, STEPS.len()));
            let last = i + 1 == STEPS.len();
            assert_eq!(guide.is_last(), last);
            assert_eq!(guide.next(), last);
        }
        assert!(!guide.is_open());
        assert_eq!(guide.current(), None);
        assert!(!guide.next());
    }

    #[test]
    fn seen_version_stays_closed_until_replayed_and_skip_closes_at_any_step() {
        let mut guide = Tutorial::for_seen(VERSION);
        assert!(!guide.is_open());
        assert!(!guide.skip());

        guide.replay();
        assert_eq!(guide.current().map(|(_, n, _)| n), Some(1));
        assert!(!guide.next());
        assert_eq!(guide.current().map(|(_, n, _)| n), Some(2));
        assert!(guide.skip());
        assert!(!guide.is_open());
    }
}
