//! A candidate becomes a URL only if it is http or https and has a host.

use url::Url;

/// Parses a candidate, adding `https://` to a bare domain such as `youtube.com/shorts/x`.
pub(super) fn parse(candidate: &str) -> Option<Url> {
    let url = if has_scheme(candidate) {
        Url::parse(candidate).ok()?
    } else {
        Url::parse(&format!("https://{candidate}")).ok()?
    };
    let web = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|host| !host.is_empty());
    web.then_some(url)
}

/// Whether the text starts with `scheme://`.
fn has_scheme(text: &str) -> bool {
    text.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    })
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn bare_domain_gets_https() {
        let url = parse("youtube.com/shorts/abc").unwrap();
        assert_eq!(url.as_str(), "https://youtube.com/shorts/abc");
    }

    #[test]
    fn other_schemes_are_dropped() {
        assert!(parse("ftp://example.org/clip.mp4").is_none());
        assert!(parse("file:///C:/clip.mp4").is_none());
    }
}
