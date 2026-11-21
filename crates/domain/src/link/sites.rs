//! The site table. A new site is a new row; nothing else in the pipeline changes.

use super::classify::{Intent, LinkKind, Site};
use super::route::Route;

/// What a recognized link turns into.
pub(super) struct Canonical {
    /// Replaces the host, such as `x.com` for twitter.com and its mirrors.
    pub host: Option<&'static str>,
    /// Rebuilds a single item's path and id parameter, such as `/watch?v={id}`.
    pub single: Option<&'static str>,
}

/// Everything pastegrab knows about one site.
pub(super) struct SiteRule {
    pub site: Site,
    /// Hosts with `www.`, `m.` or `mobile.` already stripped.
    pub hosts: &'static [&'static str],
    /// Tried in order; the first match decides the kind and the id.
    pub routes: &'static [Route],
    /// Query parameters that survive cleaning.
    pub keep: &'static [&'static str],
    pub intent: Intent,
    pub canonical: Canonical,
}

/// The rule for a host, after its mobile or `www.` prefix is stripped.
pub(super) fn rule_for(host: &str) -> Option<&'static SiteRule> {
    let bare = ["www.", "m.", "mobile."]
        .iter()
        .find_map(|prefix| host.strip_prefix(prefix))
        .unwrap_or(host);
    SITES.iter().find(|rule| rule.hosts.contains(&bare))
}

const fn path(path: &'static str, kind: LinkKind) -> Route {
    Route {
        path,
        query_id: None,
        kind,
    }
}

const fn query(path: &'static str, param: &'static str, kind: LinkKind) -> Route {
    Route {
        path,
        query_id: Some(param),
        kind,
    }
}

const fn host(host: &'static str) -> Canonical {
    Canonical {
        host: Some(host),
        single: None,
    }
}

use LinkKind::{Channel, Playlist, Single};

const YOUTUBE: Canonical = Canonical {
    host: Some("www.youtube.com"),
    single: Some("/watch?v={id}"),
};

const DAILYMOTION: Canonical = Canonical {
    host: Some("www.dailymotion.com"),
    single: Some("/video/{id}"),
};

const SITES: &[SiteRule] = &[
    SiteRule {
        site: Site::YouTube,
        hosts: &["youtube.com", "youtube-nocookie.com"],
        routes: &[
            query("/watch", "v", Single),
            path("/shorts/{id}", Single),
            path("/live/{id}", Single),
            path("/embed/{id}", Single),
            path("/v/{id}", Single),
            query("/playlist", "list", Playlist),
            path("/@{id}", Channel),
            path("/@{id}/{}", Channel),
            path("/channel/{id}", Channel),
            path("/channel/{id}/{}", Channel),
            path("/c/{id}", Channel),
            path("/user/{id}", Channel),
        ],
        keep: &["v", "list", "index", "t"],
        intent: Intent::Video,
        canonical: YOUTUBE,
    },
    SiteRule {
        site: Site::YouTube,
        hosts: &["youtu.be"],
        routes: &[path("/{id}", Single)],
        keep: &["list", "index", "t"],
        intent: Intent::Video,
        canonical: YOUTUBE,
    },
    SiteRule {
        site: Site::YouTube,
        hosts: &["music.youtube.com"],
        routes: &[
            query("/watch", "v", Single),
            query("/playlist", "list", Playlist),
        ],
        keep: &["v", "list", "index", "t"],
        intent: Intent::Audio,
        canonical: Canonical {
            host: Some("music.youtube.com"),
            single: Some("/watch?v={id}"),
        },
    },
    SiteRule {
        site: Site::Olympics,
        hosts: &["olympics.com"],
        routes: &[
            path("/{}/video/{id}", Single),
            path("/{}/olympic-games/{}/replay/{id}", Single),
        ],
        keep: &[],
        intent: Intent::Video,
        canonical: host("olympics.com"),
    },
    SiteRule {
        site: Site::ArchiveOrg,
        hosts: &["archive.org"],
        routes: &[
            path("/details/{id}", Single),
            path("/details/{id}/{}", Single),
            path("/embed/{id}", Single),
        ],
        keep: &[],
        intent: Intent::Video,
        canonical: host("archive.org"),
    },
    SiteRule {
        site: Site::Vk,
        hosts: &["vk.com"],
        routes: &[path("/video{id}", Single), path("/clip{id}", Single)],
        // `list` is the access key of a private video, not a playlist.
        keep: &["list"],
        intent: Intent::Video,
        canonical: host("vk.com"),
    },
    SiteRule {
        site: Site::Vk,
        hosts: &["vkvideo.ru"],
        routes: &[path("/video{id}", Single), path("/clip{id}", Single)],
        keep: &["list"],
        intent: Intent::Video,
        canonical: host("vkvideo.ru"),
    },
    SiteRule {
        site: Site::OkRu,
        hosts: &["ok.ru", "odnoklassniki.ru"],
        routes: &[
            path("/video/{n}", Single),
            path("/live/{n}", Single),
            path("/videoembed/{n}", Single),
        ],
        keep: &[],
        intent: Intent::Video,
        canonical: host("ok.ru"),
    },
    SiteRule {
        site: Site::Rutube,
        hosts: &["rutube.ru"],
        routes: &[
            path("/video/{id}", Single),
            path("/shorts/{id}", Single),
            path("/play/embed/{id}", Single),
        ],
        // `p` is the access key of a private video.
        keep: &["p"],
        intent: Intent::Video,
        canonical: host("rutube.ru"),
    },
    SiteRule {
        site: Site::Dailymotion,
        hosts: &["dailymotion.com"],
        routes: &[
            path("/video/{id}", Single),
            path("/embed/video/{id}", Single),
        ],
        keep: &[],
        intent: Intent::Video,
        canonical: DAILYMOTION,
    },
    SiteRule {
        site: Site::Dailymotion,
        hosts: &["dai.ly"],
        routes: &[path("/{id}", Single)],
        keep: &[],
        intent: Intent::Video,
        canonical: DAILYMOTION,
    },
    SiteRule {
        site: Site::Smotrim,
        hosts: &["smotrim.ru"],
        routes: &[path("/video/{n}", Single), path("/live/{n}", Single)],
        keep: &[],
        intent: Intent::Video,
        canonical: host("smotrim.ru"),
    },
    SiteRule {
        site: Site::Dzen,
        hosts: &["dzen.ru"],
        routes: &[path("/video/watch/{id}", Single)],
        keep: &[],
        intent: Intent::Video,
        canonical: host("dzen.ru"),
    },
    SiteRule {
        site: Site::Vimeo,
        hosts: &["vimeo.com"],
        routes: &[
            path("/{n}", Single),
            path("/channels/{}/{n}", Single),
            path("/groups/{}/videos/{n}", Single),
        ],
        keep: &[],
        intent: Intent::Video,
        canonical: host("vimeo.com"),
    },
    SiteRule {
        site: Site::X,
        hosts: &[
            "x.com",
            "twitter.com",
            "vxtwitter.com",
            "fxtwitter.com",
            "fixupx.com",
            "fixvx.com",
        ],
        routes: &[
            path("/{}/status/{n}", Single),
            path("/{}/status/{n}/{}", Single),
            path("/{}/status/{n}/{}/{}", Single),
        ],
        keep: &[],
        intent: Intent::Video,
        canonical: host("x.com"),
    },
    SiteRule {
        site: Site::TikTok,
        hosts: &["tiktok.com"],
        routes: &[path("/{}/video/{n}", Single)],
        keep: &[],
        intent: Intent::Video,
        canonical: host("www.tiktok.com"),
    },
    SiteRule {
        site: Site::TikTok,
        // Short links resolve only over the network; yt-dlp follows them.
        hosts: &["vm.tiktok.com", "vt.tiktok.com"],
        routes: &[path("/{id}", Single)],
        keep: &[],
        intent: Intent::Video,
        canonical: Canonical {
            host: None,
            single: None,
        },
    },
    SiteRule {
        site: Site::SoundCloud,
        hosts: &["soundcloud.com"],
        routes: &[path("/{}/sets/{id}", Playlist), path("/{}/{id}", Single)],
        keep: &[],
        intent: Intent::Audio,
        canonical: host("soundcloud.com"),
    },
];
