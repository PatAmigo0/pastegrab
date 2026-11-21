//! Query cleaning: known sites keep an allowlist, unknown sites lose known trackers only.
//!
//! An allowlist would break unknown sites, and a denylist alone misses next year's tracker.

use url::Url;

/// Parameters that only ever carry tracking, on any site. `utm_*` is matched by prefix.
const TRACKERS: &[&str] = &[
    "_hsenc", "_hsmi", "dclid", "fbclid", "gbraid", "gclid", "igsh", "igshid", "mc_cid", "mc_eid",
    "mkt_tok", "msclkid", "wbraid", "yclid", "ysclid",
];

/// Keeps only the named parameters, in their original order.
pub(super) fn keep_only(url: &mut Url, keep: &[&str]) {
    retain(url, |name| keep.contains(&name));
}

/// Drops tracking parameters; a query without any keeps its original bytes.
pub(super) fn drop_trackers(url: &mut Url) {
    retain(url, |name| !is_tracker(name));
}

/// Replaces the query with `pairs`, or removes it when there are none.
pub(super) fn set_pairs(url: &mut Url, pairs: &[(String, String)]) {
    if pairs.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
}

fn is_tracker(name: &str) -> bool {
    name.starts_with("utm_") || TRACKERS.contains(&name)
}

fn retain(url: &mut Url, keep: impl Fn(&str) -> bool) {
    if url.query().is_some_and(str::is_empty) {
        url.set_query(None);
        return;
    }
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    // Re-encoding can change a query that some site depends on, so an untouched one stays as is.
    if pairs.iter().all(|(key, _)| keep(key)) {
        return;
    }
    let kept: Vec<_> = pairs.into_iter().filter(|(key, _)| keep(key)).collect();
    set_pairs(url, &kept);
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{drop_trackers, keep_only};

    #[test]
    fn unknown_site_loses_trackers_only() {
        let mut url = Url::parse("https://example.org/a?utm_source=tg&id=5&ysclid=x").unwrap();
        drop_trackers(&mut url);
        assert_eq!(url.as_str(), "https://example.org/a?id=5");
    }

    #[test]
    fn query_without_trackers_keeps_its_bytes() {
        let mut url = Url::parse("https://example.org/a?q=a+b&flag").unwrap();
        drop_trackers(&mut url);
        assert_eq!(url.as_str(), "https://example.org/a?q=a+b&flag");
    }

    #[test]
    fn allowlist_keeps_order() {
        let mut url = Url::parse("https://example.org/w?si=1&v=abc&feature=x&t=9").unwrap();
        keep_only(&mut url, &["v", "t"]);
        assert_eq!(url.as_str(), "https://example.org/w?v=abc&t=9");
    }
}
