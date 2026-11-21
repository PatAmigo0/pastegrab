//! Path templates for the site table, such as `/watch`, `/shorts/{id}` or `/{}/status/{n}`.

use url::Url;

use super::classify::LinkKind;

/// One URL shape on a site and the id it carries.
pub(super) struct Route {
    /// Segments are literal, `{}` matches any one segment, and `{id}` captures the id, or `{n}` if
    /// it must be digits. A literal prefix may precede the capture, as in `@{id}` or `video{id}`.
    /// A final `{id+}` captures the rest of the path, one segment or more.
    pub path: &'static str,
    /// Takes the id from this query parameter instead of the path.
    pub query_id: Option<&'static str>,
    pub kind: LinkKind,
}

impl Route {
    /// The id this route finds in `url`, or `None` when the route does not match.
    pub(super) fn id_in(&self, url: &Url) -> Option<String> {
        let PathMatch::Hit(path_id) = match_path(self.path, url.path()) else {
            return None;
        };
        match self.query_id {
            Some(name) => url
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .filter(|value| !value.is_empty()),
            None => path_id.map(str::to_owned),
        }
    }
}

/// How a path compares with a template.
#[derive(Debug, PartialEq, Eq)]
enum PathMatch<'p> {
    /// The path has another shape.
    Miss,
    /// The path fits; the id is there when the template captures one.
    Hit(Option<&'p str>),
}

fn match_path<'p>(template: &str, path: &'p str) -> PathMatch<'p> {
    let mut id = None;
    let mut rest = Some(path.trim_end_matches('/'));
    for want in template.trim_end_matches('/').split('/') {
        let Some(have) = rest else {
            return PathMatch::Miss;
        };
        if want == "{id+}" {
            return if have.is_empty() {
                PathMatch::Miss
            } else {
                PathMatch::Hit(Some(have))
            };
        }
        let (segment, next) = match have.split_once('/') {
            Some((segment, next)) => (segment, Some(next)),
            None => (have, None),
        };
        if !segment_matches(want, segment, &mut id) {
            return PathMatch::Miss;
        }
        rest = next;
    }
    if rest.is_none() {
        PathMatch::Hit(id)
    } else {
        PathMatch::Miss
    }
}

fn segment_matches<'p>(template: &str, segment: &'p str, id: &mut Option<&'p str>) -> bool {
    if template == "{}" {
        return !segment.is_empty();
    }
    let (prefix, digits) = if let Some(prefix) = template.strip_suffix("{id}") {
        (prefix, false)
    } else if let Some(prefix) = template.strip_suffix("{n}") {
        (prefix, true)
    } else {
        return template == segment;
    };
    match segment.strip_prefix(prefix) {
        Some(rest) if !rest.is_empty() && (!digits || rest.bytes().all(|b| b.is_ascii_digit())) => {
            *id = Some(rest);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{PathMatch, match_path};

    #[test]
    fn literal_and_capture() {
        assert_eq!(
            match_path("/shorts/{id}", "/shorts/abc"),
            PathMatch::Hit(Some("abc"))
        );
        assert_eq!(match_path("/watch", "/watch"), PathMatch::Hit(None));
        assert_eq!(match_path("/watch", "/watch/more"), PathMatch::Miss);
    }

    #[test]
    fn prefix_capture_and_trailing_slash() {
        assert_eq!(
            match_path("/@{id}", "/@channel/"),
            PathMatch::Hit(Some("channel"))
        );
        assert_eq!(
            match_path("/video{id}", "/video-1_2"),
            PathMatch::Hit(Some("-1_2"))
        );
        assert_eq!(match_path("/video{id}", "/video"), PathMatch::Miss);
    }

    #[test]
    fn digits_only_capture() {
        assert_eq!(
            match_path("/{}/status/{n}", "/a/status/123"),
            PathMatch::Hit(Some("123"))
        );
        assert_eq!(match_path("/{n}", "/about"), PathMatch::Miss);
    }

    #[test]
    fn rest_capture_takes_every_remaining_segment() {
        assert_eq!(
            match_path("/details/{id+}", "/details/item/file.mpg"),
            PathMatch::Hit(Some("item/file.mpg"))
        );
        assert_eq!(
            match_path("/details/{id+}", "/details/item/"),
            PathMatch::Hit(Some("item"))
        );
        assert_eq!(match_path("/details/{id+}", "/details"), PathMatch::Miss);
        assert_eq!(match_path("/details/{id+}", "/details/"), PathMatch::Miss);
    }
}
