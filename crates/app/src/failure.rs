//! What a step reports when it fails.

use std::fmt;

use pastegrab_domain::job::FailureKind;

/// A failure kind, which decides the retry policy and the message, plus a detail for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// What went wrong, as the policy sees it.
    pub kind: FailureKind,
    /// The tool's own words or the error text, for the log and diagnostics.
    pub detail: String,
}

impl Failure {
    /// A failure of `kind` with `detail` for the log.
    #[must_use]
    pub fn new(kind: FailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// A failure nothing better describes.
    #[must_use]
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(FailureKind::Internal, detail)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for Failure {}
