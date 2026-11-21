//! Candidate links in free text.

use linkify::{LinkFinder, LinkKind};

/// Spans of `text` that look like web links, scheme optional, surrounding punctuation trimmed.
pub(super) fn candidates(text: &str) -> Vec<&str> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]).url_must_have_scheme(false);
    finder.links(text).map(|link| link.as_str()).collect()
}
