//! The site table decides what a URL is, and the URL then takes its canonical form.

use std::fmt;

use url::Url;

use super::clean;
use super::sites::{self, SiteRule};

/// A cleaned, canonical http(s) URL. Only the link pipeline builds one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CleanUrl(Url);

impl CleanUrl {
    /// The URL as text, ready for yt-dlp or an HTTP request.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The parsed URL.
    #[must_use]
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Display for CleanUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Sites pastegrab knows by name. Every other host goes to yt-dlp's generic extractor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    /// youtube.com, youtu.be and music.youtube.com.
    YouTube,
    /// olympics.com.
    Olympics,
    /// archive.org.
    ArchiveOrg,
    /// vk.com and vkvideo.ru.
    Vk,
    /// ok.ru.
    OkRu,
    /// rutube.ru.
    Rutube,
    /// dailymotion.com and dai.ly.
    Dailymotion,
    /// smotrim.ru.
    Smotrim,
    /// dzen.ru.
    Dzen,
    /// vimeo.com.
    Vimeo,
    /// x.com, twitter.com and their embed mirrors.
    X,
    /// tiktok.com.
    TikTok,
    /// soundcloud.com.
    SoundCloud,
}

impl Site {
    /// A stable lowercase name for logs and keys.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::YouTube => "youtube",
            Self::Olympics => "olympics",
            Self::ArchiveOrg => "archive.org",
            Self::Vk => "vk",
            Self::OkRu => "ok.ru",
            Self::Rutube => "rutube",
            Self::Dailymotion => "dailymotion",
            Self::Smotrim => "smotrim",
            Self::Dzen => "dzen",
            Self::Vimeo => "vimeo",
            Self::X => "x",
            Self::TikTok => "tiktok",
            Self::SoundCloud => "soundcloud",
        }
    }
}

/// What a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// One video or track.
    Single,
    /// A playlist or set.
    Playlist,
    /// A channel or user page.
    Channel,
    /// A media file served as is.
    DirectMedia,
    /// An HLS or DASH manifest.
    Manifest(ManifestKind),
    /// A page the site table has no shape for; yt-dlp decides what it holds.
    Page,
}

/// The two manifest formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestKind {
    /// An `.m3u8` playlist.
    Hls,
    /// An `.mpd` manifest.
    Dash,
}

/// What the person most likely wants from a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    /// The video, with its sound.
    Video,
    /// The sound only: music sites and audio files.
    Audio,
}

/// The playlist a YouTube video was opened from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaylistHint {
    /// The `list` parameter.
    pub id: String,
    /// The video's position in the list, from the `index` parameter.
    pub index: Option<u32>,
    /// `RD` lists are endless Mixes YouTube generates, never offered as playlists.
    pub endless_mix: bool,
}

/// One link from pasted text, cleaned and classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The cleaned, canonical URL.
    pub url: CleanUrl,
    /// The site, when the site table knows the host.
    pub site: Option<Site>,
    /// What the URL points at.
    pub kind: LinkKind,
    /// The site's id for the item, playlist or channel, when a route captured one.
    pub id: Option<String>,
    /// Video by default; audio for music sites and audio files.
    pub intent: Intent,
    /// Where playback starts, in seconds, from YouTube's `t` parameter.
    pub start: Option<u32>,
    /// The playlist a YouTube video was opened from.
    pub playlist: Option<PlaylistHint>,
}

/// What makes two links the same download.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DedupKey {
    /// An item on a known site, however its link was written.
    Item(Site, LinkKind, String),
    /// Anything else, by its cleaned URL.
    Url(String),
}

impl Link {
    /// Two links with the same key are the same download.
    #[must_use]
    pub fn dedup_key(&self) -> DedupKey {
        match (self.site, &self.id) {
            (Some(site), Some(id)) => DedupKey::Item(site, self.kind, id.clone()),
            _ => DedupKey::Url(self.url.as_str().to_owned()),
        }
    }

    fn unknown(url: Url, kind: LinkKind, intent: Intent) -> Self {
        Self {
            url: CleanUrl(url),
            site: None,
            kind,
            id: None,
            intent,
            start: None,
            playlist: None,
        }
    }
}

/// Classifies one URL, cleans its query and rewrites it to the site's canonical form.
pub(super) fn classify(mut url: Url) -> Link {
    let rule = url.host_str().and_then(sites::rule_for);
    let route = rule.and_then(|rule| {
        rule.routes
            .iter()
            .find_map(|route| route.id_in(&url).map(|id| (route.kind, Some(id))))
    });
    // A site's own page wins over a file name in its path; any other path naming a file is that file.
    if route.is_none()
        && let Some((kind, intent)) = by_extension(url.path())
    {
        clean::drop_trackers(&mut url);
        return Link::unknown(url, kind, intent);
    }
    let Some(rule) = rule else {
        clean::drop_trackers(&mut url);
        return Link::unknown(url, LinkKind::Page, Intent::Video);
    };
    let (kind, id) = route.unwrap_or((LinkKind::Page, None));
    let youtube_single = rule.site == Site::YouTube && kind == LinkKind::Single;
    let start = youtube_single
        .then(|| query_value(&url, "t"))
        .flatten()
        .and_then(|t| parse_timestamp(&t));
    let playlist = youtube_single.then(|| playlist_hint(&url)).flatten();

    clean::keep_only(&mut url, rule.keep);
    url.set_fragment(None);
    canonicalize(&mut url, rule, kind, id.as_deref());

    Link {
        url: CleanUrl(url),
        site: Some(rule.site),
        kind,
        id,
        intent: rule.intent,
        start,
        playlist,
    }
}

/// Media files and manifests are known by their extension, unless a site route claims the URL.
fn by_extension(path: &str) -> Option<(LinkKind, Intent)> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    let found = match ext.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "flv" | "wmv" | "mpg" | "mpeg" | "ts"
        | "3gp" => (LinkKind::DirectMedia, Intent::Video),
        "mp3" | "m4a" | "aac" | "ogg" | "oga" | "opus" | "flac" | "wav" => {
            (LinkKind::DirectMedia, Intent::Audio)
        }
        "m3u8" => (LinkKind::Manifest(ManifestKind::Hls), Intent::Video),
        "mpd" => (LinkKind::Manifest(ManifestKind::Dash), Intent::Video),
        _ => return None,
    };
    Some(found)
}

fn canonicalize(url: &mut Url, rule: &SiteRule, kind: LinkKind, id: Option<&str>) {
    // Both calls fail only for non-special schemes or empty hosts, which parsing already ruled out.
    let _ = url.set_scheme("https");
    if let Some(host) = rule.canonical.host {
        let _ = url.set_host(Some(host));
    }
    let (Some(template), Some(id), LinkKind::Single) = (rule.canonical.single, id, kind) else {
        return;
    };
    let (path, id_param) = match template.split_once('?') {
        Some((path, query)) => (path, query.strip_suffix("={id}")),
        None => (template, None),
    };
    let mut pairs = Vec::new();
    if let Some(name) = id_param {
        pairs.push((name.to_owned(), id.to_owned()));
    }
    pairs.extend(
        url.query_pairs()
            .filter(|(key, _)| Some(key.as_ref()) != id_param)
            .map(|(key, value)| (key.into_owned(), value.into_owned())),
    );
    url.set_path(&path.replace("{id}", id));
    clean::set_pairs(url, &pairs);
}

fn query_value(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn playlist_hint(url: &Url) -> Option<PlaylistHint> {
    let id = query_value(url, "list").filter(|list| !list.is_empty())?;
    let index = query_value(url, "index").and_then(|index| index.parse().ok());
    let endless_mix = id.starts_with("RD");
    Some(PlaylistHint {
        id,
        index,
        endless_mix,
    })
}

/// YouTube's `t`: plain seconds such as `95` or `95s`, or units such as `1h2m3s`.
fn parse_timestamp(text: &str) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    let mut total: u32 = 0;
    let mut number: u32 = 0;
    for c in text.chars() {
        if let Some(digit) = c.to_digit(10) {
            number = number.checked_mul(10)?.checked_add(digit)?;
            continue;
        }
        let unit = match c {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        total = total.checked_add(number.checked_mul(unit)?)?;
        number = 0;
    }
    total.checked_add(number)
}

#[cfg(test)]
mod tests {
    use super::{LinkKind, ManifestKind, by_extension, parse_timestamp};

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("95"), Some(95));
        assert_eq!(parse_timestamp("95s"), Some(95));
        assert_eq!(parse_timestamp("1m35s"), Some(95));
        assert_eq!(parse_timestamp("1h2m3s"), Some(3723));
        assert_eq!(parse_timestamp("1:35"), None);
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn extensions() {
        assert_eq!(
            by_extension("/a/clip.MP4").map(|found| found.0),
            Some(LinkKind::DirectMedia)
        );
        assert_eq!(
            by_extension("/live/master.m3u8").map(|found| found.0),
            Some(LinkKind::Manifest(ManifestKind::Hls))
        );
        assert_eq!(by_extension("/watch"), None);
        assert_eq!(by_extension("/v1.2/page"), None);
    }
}
