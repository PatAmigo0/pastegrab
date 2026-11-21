//! Redirect wrappers such as `l.facebook.com/l.php?u=` give up the link they carry.

use url::Url;

use super::parse;

/// Three levels cover a Google result that points at a Facebook redirect that points at the video.
const MAX_DEPTH: usize = 3;

/// One redirect wrapper: where it lives and which query parameter carries the target.
struct Wrapper {
    hosts: Hosts,
    /// Exact path, or any path on the host when `None`.
    path: Option<&'static str>,
    /// Parameters that may carry the target; the first one present wins.
    params: &'static [&'static str],
}

enum Hosts {
    Exact(&'static [&'static str]),
    /// `google.com`, `www.google.ru` and every other Google country domain.
    Google,
}

const WRAPPERS: &[Wrapper] = &[
    Wrapper {
        hosts: Hosts::Google,
        path: Some("/url"),
        params: &["q", "url"],
    },
    Wrapper {
        hosts: Hosts::Exact(&["l.facebook.com", "lm.facebook.com"]),
        path: Some("/l.php"),
        params: &["u"],
    },
    Wrapper {
        hosts: Hosts::Exact(&["l.instagram.com"]),
        path: Some("/"),
        params: &["u"],
    },
    Wrapper {
        hosts: Hosts::Exact(&["vk.com", "m.vk.com"]),
        path: Some("/away.php"),
        params: &["to"],
    },
    Wrapper {
        hosts: Hosts::Exact(&["www.youtube.com", "youtube.com", "m.youtube.com"]),
        path: Some("/redirect"),
        params: &["q"],
    },
    Wrapper {
        hosts: Hosts::Exact(&["out.reddit.com"]),
        path: None,
        params: &["url"],
    },
];

/// Follows wrappers offline until the URL is no longer one.
pub(super) fn unwrap(mut url: Url) -> Url {
    for _ in 0..MAX_DEPTH {
        match target(&url) {
            Some(inner) => url = inner,
            None => break,
        }
    }
    url
}

fn target(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let wrapper = WRAPPERS
        .iter()
        .find(|w| w.hosts.contains(host) && w.path.is_none_or(|path| path == url.path()))?;
    let value = wrapper.params.iter().find_map(|name| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    })?;
    parse::parse(&value)
}

impl Hosts {
    fn contains(&self, host: &str) -> bool {
        match self {
            Self::Exact(hosts) => hosts.contains(&host),
            Self::Google => {
                let rest = host.strip_prefix("www.").unwrap_or(host);
                rest.strip_prefix("google.")
                    .is_some_and(|tld| !tld.is_empty())
            }
        }
    }
}
