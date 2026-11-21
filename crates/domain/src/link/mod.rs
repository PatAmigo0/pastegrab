//! Pasted text in, cleaned and classified links out.
//!
//! The stages run in order and never touch the network: find candidates in the text, keep the
//! http(s) ones, strip redirect wrappers, then match the site table, which cleans the query and
//! rewrites the URL to its canonical form.

mod classify;
mod clean;
mod find;
mod parse;
mod route;
mod sites;
mod unwrap;

use std::collections::HashSet;

pub use classify::{CleanUrl, Intent, Link, LinkKind, ManifestKind, PlaylistHint, Site};

/// Every link in `text`, cleaned and classified, in order of appearance and without duplicates.
#[must_use]
pub fn analyze(text: &str) -> Vec<Link> {
    let mut seen = HashSet::new();
    find::candidates(text)
        .into_iter()
        .filter_map(parse::parse)
        .map(unwrap::unwrap)
        .map(classify::classify)
        .filter(|link| seen.insert(link.dedup_key()))
        .collect()
}
