//! A plan that is one plain https file, which the direct fetcher can take over several
//! connections instead of yt-dlp's one.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use pastegrab_domain::plan::FetchPlan;
use serde::Deserialize;

/// Headers yt-dlp sends to every site; a format that asks for nothing more is a plain file.
const PLAIN_HEADERS: [&str; 4] = ["user-agent", "accept", "accept-language", "sec-fetch-mode"];

#[derive(Deserialize)]
struct Info {
    formats: Vec<InfoFormat>,
}

#[derive(Deserialize)]
struct InfoFormat {
    format_id: String,
    url: Option<String>,
    protocol: Option<String>,
    #[serde(default)]
    http_headers: BTreeMap<String, String>,
}

/// The URL of the plan's one format when it is a plain http(s) file: no merge, no section, no
/// browser impersonation, and no header beyond the ones yt-dlp sends everywhere.
pub(crate) fn plain_url(plan: &FetchPlan, info_json: &Path, impersonate: bool) -> Option<String> {
    let [id] = plan.format_ids.as_slice() else {
        return None;
    };
    if impersonate || plan.merge_into.is_some() || plan.section.is_some() {
        return None;
    }
    let info: Info = serde_json::from_slice(&fs::read(info_json).ok()?).ok()?;
    let format = info
        .formats
        .into_iter()
        .find(|format| format.format_id == id.as_str())?;
    let plain_protocol = matches!(format.protocol.as_deref(), Some("https" | "http"));
    let plain_headers = format
        .http_headers
        .keys()
        .all(|name| PLAIN_HEADERS.contains(&name.to_ascii_lowercase().as_str()));
    format.url.filter(|_| plain_protocol && plain_headers)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pastegrab_domain::media::FormatId;
    use pastegrab_domain::plan::{FetchPlan, Section};
    use pastegrab_domain::profile::Container;
    use pastegrab_testkit::TempDir;

    use super::plain_url;

    fn plan(ids: &[&str]) -> FetchPlan {
        FetchPlan {
            format_ids: ids.iter().filter_map(|id| FormatId::parse(id)).collect(),
            merge_into: None,
            section: None,
            expected_bytes: None,
            encodes_video: false,
        }
    }

    const INFO: &str = r#"{"formats": [
        {"format_id": "512kb", "protocol": "https", "url": "https://archive.org/download/Cops1922/Cops1922_512kb.mp4",
         "http_headers": {"User-Agent": "Mozilla/5.0", "Accept": "*/*", "Accept-Language": "en-us,en;q=0.5", "Sec-Fetch-Mode": "navigate"}},
        {"format_id": "hls-720", "protocol": "m3u8_native", "url": "https://example.com/720.m3u8", "http_headers": {}},
        {"format_id": "refer", "protocol": "https", "url": "https://vk.example/v.mp4",
         "http_headers": {"Referer": "https://vk.com/"}}
    ]}"#;

    #[test]
    fn one_plain_https_format_goes_direct_and_anything_else_stays_with_yt_dlp() {
        let dir = TempDir::new("plain").expect("a temp folder");
        let info = dir.path().join("probe.info.json");
        fs::write(&info, INFO).expect("an info file");
        assert_eq!(
            plain_url(&plan(&["512kb"]), &info, false).as_deref(),
            Some("https://archive.org/download/Cops1922/Cops1922_512kb.mp4")
        );
        assert_eq!(
            plain_url(&plan(&["512kb"]), &info, true),
            None,
            "impersonation"
        );
        assert_eq!(
            plain_url(&plan(&["hls-720"]), &info, false),
            None,
            "fragments"
        );
        assert_eq!(
            plain_url(&plan(&["refer"]), &info, false),
            None,
            "a header the site wants"
        );
        assert_eq!(
            plain_url(&plan(&["512kb", "refer"]), &info, false),
            None,
            "a merge"
        );
        let mut cut = plan(&["512kb"]);
        cut.section = Section::new(std::time::Duration::ZERO, std::time::Duration::from_secs(5));
        assert_eq!(plain_url(&cut, &info, false), None, "a section");
        let mut merged = plan(&["512kb"]);
        merged.merge_into = Some(Container::Mkv);
        assert_eq!(plain_url(&merged, &info, false), None, "a remux");
        assert_eq!(
            plain_url(&plan(&["512kb"]), &dir.path().join("none.json"), false),
            None
        );
    }
}
