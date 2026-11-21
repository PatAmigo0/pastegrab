//! The link pipeline against the examples in SPEC.md, section 5, plus property checks.

use pastegrab_domain::link::{Intent, Link, LinkKind, ManifestKind, PlaylistHint, Site, analyze};
use proptest::prelude::*;

fn one(text: &str) -> Link {
    let mut links = analyze(text);
    assert_eq!(links.len(), 1, "{text:?} gave {links:#?}");
    links.remove(0)
}

#[track_caller]
fn check(text: &str, url: &str, site: Option<Site>, kind: LinkKind, id: Option<&str>) -> Link {
    let link = one(text);
    assert_eq!(link.url.as_str(), url, "url of {text:?}");
    assert_eq!(link.site, site, "site of {text:?}");
    assert_eq!(link.kind, kind, "kind of {text:?}");
    assert_eq!(link.id.as_deref(), id, "id of {text:?}");
    link
}

#[test]
fn youtu_be_in_russian_text_loses_share_tracking() {
    check(
        "глянь https://youtu.be/AbC123xyz_0?si=Q1w2 !!",
        "https://www.youtube.com/watch?v=AbC123xyz_0",
        Some(Site::YouTube),
        LinkKind::Single,
        Some("AbC123xyz_0"),
    );
}

#[test]
fn olympics_video_loses_uxreference() {
    check(
        "https://olympics.com/ru/video/slavomir-knazovicky-canoe-sprint-c1-500m-silver-medal-atlanta-1996?uxreference=playlist",
        "https://olympics.com/ru/video/slavomir-knazovicky-canoe-sprint-c1-500m-silver-medal-atlanta-1996",
        Some(Site::Olympics),
        LinkKind::Single,
        Some("slavomir-knazovicky-canoe-sprint-c1-500m-silver-medal-atlanta-1996"),
    );
}

#[test]
fn shorts_without_scheme_become_watch_links() {
    check(
        "youtube.com/shorts/AbC123xyz_0",
        "https://www.youtube.com/watch?v=AbC123xyz_0",
        Some(Site::YouTube),
        LinkKind::Single,
        Some("AbC123xyz_0"),
    );
}

#[test]
fn mobile_youtube_folds_into_www() {
    check(
        "https://m.youtube.com/watch?v=AbC123xyz_0&feature=share",
        "https://www.youtube.com/watch?v=AbC123xyz_0",
        Some(Site::YouTube),
        LinkKind::Single,
        Some("AbC123xyz_0"),
    );
}

#[test]
fn video_inside_a_playlist_keeps_the_hint() {
    let link = check(
        "https://www.youtube.com/watch?v=AbC123xyz_0&list=PLq9z&index=4&pp=x",
        "https://www.youtube.com/watch?v=AbC123xyz_0&list=PLq9z&index=4",
        Some(Site::YouTube),
        LinkKind::Single,
        Some("AbC123xyz_0"),
    );
    assert_eq!(
        link.playlist,
        Some(PlaylistHint {
            id: "PLq9z".into(),
            index: Some(4),
            endless_mix: false,
        })
    );
}

#[test]
fn mix_is_marked_endless() {
    let link = one("https://www.youtube.com/watch?v=AbC123xyz_0&list=RDAbC123xyz_0");
    assert!(link.playlist.is_some_and(|hint| hint.endless_mix));
}

#[test]
fn start_time_comes_from_t() {
    assert_eq!(
        one("https://www.youtube.com/watch?v=AbC123xyz_0&t=95").start,
        Some(95)
    );
    assert_eq!(one("https://youtu.be/AbC123xyz_0?t=1m35s").start, Some(95));
}

#[test]
fn youtube_music_means_audio() {
    let link = check(
        "https://music.youtube.com/watch?v=AbC123xyz_0&si=abc",
        "https://music.youtube.com/watch?v=AbC123xyz_0",
        Some(Site::YouTube),
        LinkKind::Single,
        Some("AbC123xyz_0"),
    );
    assert_eq!(link.intent, Intent::Audio);
}

#[test]
fn playlist_and_channel_pages() {
    check(
        "https://www.youtube.com/playlist?list=PLq9z",
        "https://www.youtube.com/playlist?list=PLq9z",
        Some(Site::YouTube),
        LinkKind::Playlist,
        Some("PLq9z"),
    );
    check(
        "https://www.youtube.com/@olympics/videos",
        "https://www.youtube.com/@olympics/videos",
        Some(Site::YouTube),
        LinkKind::Channel,
        Some("olympics"),
    );
}

#[test]
fn facebook_redirect_unwraps_to_vimeo() {
    check(
        "https://l.facebook.com/l.php?u=https%3A%2F%2Fvimeo.com%2F123456&h=AT0",
        "https://vimeo.com/123456",
        Some(Site::Vimeo),
        LinkKind::Single,
        Some("123456"),
    );
}

#[test]
fn google_redirect_unwraps_to_rutube() {
    check(
        "https://www.google.ru/url?q=https://rutube.ru/video/abc123/&sa=D",
        "https://rutube.ru/video/abc123/",
        Some(Site::Rutube),
        LinkKind::Single,
        Some("abc123"),
    );
}

#[test]
fn twitter_mirror_becomes_x() {
    check(
        "https://vxtwitter.com/someone/status/1790000000000000000?s=20",
        "https://x.com/someone/status/1790000000000000000",
        Some(Site::X),
        LinkKind::Single,
        Some("1790000000000000000"),
    );
}

#[test]
fn vk_keeps_its_access_key() {
    check(
        "https://m.vk.com/video-12345_67890?list=ln-abc&from=feed",
        "https://vk.com/video-12345_67890?list=ln-abc",
        Some(Site::Vk),
        LinkKind::Single,
        Some("-12345_67890"),
    );
}

#[test]
fn dailymotion_short_link_expands() {
    check(
        "https://dai.ly/x8abc12",
        "https://www.dailymotion.com/video/x8abc12",
        Some(Site::Dailymotion),
        LinkKind::Single,
        Some("x8abc12"),
    );
}

#[test]
fn direct_files_and_manifests() {
    check(
        "https://example.org/media/clip.mp4",
        "https://example.org/media/clip.mp4",
        None,
        LinkKind::DirectMedia,
        None,
    );
    check(
        "https://cdn.example.org/live/master.m3u8?utm_source=x",
        "https://cdn.example.org/live/master.m3u8",
        None,
        LinkKind::Manifest(ManifestKind::Hls),
        None,
    );
    assert_eq!(one("https://example.org/a/song.mp3").intent, Intent::Audio);
}

#[test]
fn an_archive_org_file_page_is_the_site_not_a_direct_file() {
    let page =
        "https://archive.org/details/Election_Ads/Commercial-Nixon1960ElectionAdToughonDefense.mpg";
    check(
        page,
        page,
        Some(Site::ArchiveOrg),
        LinkKind::Single,
        Some("Election_Ads/Commercial-Nixon1960ElectionAdToughonDefense.mpg"),
    );
    check(
        "https://archive.org/download/Election_Ads/Commercial-Nixon1960ElectionAdToughonDefense.mpg",
        "https://archive.org/download/Election_Ads/Commercial-Nixon1960ElectionAdToughonDefense.mpg",
        None,
        LinkKind::DirectMedia,
        None,
    );
}

#[test]
fn two_files_of_one_archive_org_item_are_two_downloads() {
    let links = analyze(
        "https://archive.org/details/Cops1922/a.mp4 https://archive.org/details/Cops1922/b.mp4 \
         https://archive.org/details/Cops1922",
    );
    assert_eq!(links.len(), 3, "{links:#?}");
}

#[test]
fn unknown_page_loses_trackers_only() {
    check(
        "https://example.org/news/some-article?utm_source=tg&id=5&ysclid=abc",
        "https://example.org/news/some-article?id=5",
        None,
        LinkKind::Page,
        None,
    );
}

#[test]
fn several_links_keep_their_order() {
    let links = analyze(
        "https://youtu.be/AAAAAAAAAAA\nhttps://vimeo.com/1\nи ещё https://example.org/x.mp4.",
    );
    let urls: Vec<_> = links.iter().map(|link| link.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://www.youtube.com/watch?v=AAAAAAAAAAA",
            "https://vimeo.com/1",
            "https://example.org/x.mp4",
        ]
    );
}

#[test]
fn the_same_video_twice_is_one_link() {
    let links = analyze("https://youtu.be/AbC123xyz_0 https://www.youtube.com/watch?v=AbC123xyz_0");
    assert_eq!(links.len(), 1);
}

#[test]
fn non_web_links_are_ignored() {
    assert_eq!(
        analyze("ftp://example.org/clip.mp4 и просто текст"),
        Vec::<Link>::new()
    );
    assert_eq!(analyze(""), Vec::<Link>::new());
}

fn url_like() -> impl Strategy<Value = String> {
    let host = prop::sample::select(vec![
        "youtube.com",
        "m.youtube.com",
        "youtu.be",
        "music.youtube.com",
        "olympics.com",
        "vk.com",
        "vxtwitter.com",
        "dai.ly",
        "vimeo.com",
        "example.org",
        "l.facebook.com",
    ]);
    let segments = prop::collection::vec("[a-zA-Z0-9_@-]{1,12}", 0..4);
    let query = prop::collection::vec(("[a-z_]{1,6}", "[a-zA-Z0-9%._-]{0,8}"), 0..4);
    (host, segments, query).prop_map(|(host, segments, query)| {
        let mut url = format!("https://{host}/{}", segments.join("/"));
        if !query.is_empty() {
            let pairs: Vec<_> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
            url.push('?');
            url.push_str(&pairs.join("&"));
        }
        url
    })
}

proptest! {
    #[test]
    fn analysis_never_panics(text in "\\PC*") {
        let _ = analyze(&text);
    }

    #[test]
    fn analysis_is_idempotent(url in url_like()) {
        for link in analyze(&url) {
            let again = analyze(link.url.as_str());
            prop_assert_eq!(again.len(), 1);
            prop_assert_eq!(&again[0], &link);
        }
    }
}
