//! yt-dlp's error text turned into a failure kind; `app` decides what each kind means.

use pastegrab_domain::job::{FailureKind, Step};

/// "DLL not found" and "entry point not found", from a tool that needs a newer Windows.
const DLL_EXIT_CODES: [u32; 2] = [0xC000_0135, 0xC000_0139];

/// First match wins, so a narrow pattern goes above a wider one.
const PATTERNS: &[(&str, FailureKind)] = &[
    ("No space left on device", FailureKind::DiskFull),
    (
        "There is not enough space on the disk",
        FailureKind::DiskFull,
    ),
    ("[Errno 28]", FailureKind::DiskFull),
    ("No module named", FailureKind::ToolBroken),
    ("Unsupported URL", FailureKind::Unsupported),
    ("This live event will begin", FailureKind::NotStarted),
    ("Premieres in", FailureKind::NotStarted),
    ("not a bot", FailureKind::BotCheck),
    ("HTTP Error 429", FailureKind::BotCheck),
    ("Private video", FailureKind::NeedsLogin),
    ("Sign in to confirm your age", FailureKind::NeedsLogin),
    ("Video unavailable", FailureKind::Unavailable),
    ("This video is unavailable", FailureKind::Unavailable),
    ("This video has been removed", FailureKind::Unavailable),
    ("available in your country", FailureKind::Unavailable),
    ("HTTP Error 404", FailureKind::Unavailable),
    ("HTTP Error 410", FailureKind::Unavailable),
    ("Cloudflare anti-bot challenge", FailureKind::BotWall),
    ("Unable to extract", FailureKind::EngineOutdated),
    ("Signature extraction failed", FailureKind::EngineOutdated),
    ("Signature solving failed", FailureKind::EngineOutdated),
    ("n challenge solving failed", FailureKind::EngineOutdated),
    ("Only images are available", FailureKind::EngineOutdated),
    ("timed out", FailureKind::Network),
    ("Connection reset", FailureKind::Network),
    ("Connection refused", FailureKind::Network),
    ("Connection aborted", FailureKind::Network),
    ("Network is unreachable", FailureKind::Network),
    ("Temporary failure in name resolution", FailureKind::Network),
    ("Name or service not known", FailureKind::Network),
    ("getaddrinfo failed", FailureKind::Network),
    ("Remote end closed connection", FailureKind::Network),
    ("IncompleteRead", FailureKind::Network),
    (
        "EOF occurred in violation of protocol",
        FailureKind::Network,
    ),
    ("[WinError 10054]", FailureKind::Network),
    ("[WinError 10060]", FailureKind::Network),
    ("HTTP Error 500", FailureKind::Network),
    ("HTTP Error 502", FailureKind::Network),
    ("HTTP Error 503", FailureKind::Network),
    ("HTTP Error 504", FailureKind::Network),
];

pub(crate) fn classify(lines: &[String], code: Option<i32>, step: Step) -> FailureKind {
    if code.is_some_and(|code| DLL_EXIT_CODES.contains(&code.cast_unsigned())) {
        return FailureKind::ToolBroken;
    }
    // The ERROR line is yt-dlp's verdict; warnings before it are context.
    let errors = lines.iter().filter(|line| line.starts_with("ERROR:"));
    let rest = lines.iter().filter(|line| !line.starts_with("ERROR:"));
    errors
        .rev()
        .chain(rest.rev())
        .find_map(|line| kind_of(line, step))
        .unwrap_or(FailureKind::Internal)
}

fn kind_of(line: &str, step: Step) -> Option<FailureKind> {
    if let Some(&(_, kind)) = PATTERNS.iter().find(|(pattern, _)| line.contains(pattern)) {
        return Some(kind);
    }
    // Refused while extracting, it's a bot filter; while downloading, the links went stale.
    if line.contains("HTTP Error 403") {
        return Some(match step {
            Step::Probe => FailureKind::BotWall,
            Step::Fetch | Step::Convert => FailureKind::Expired,
        });
    }
    let unavailable_format = line.contains("Requested format is not available");
    (unavailable_format && step == Step::Probe).then_some(FailureKind::EngineOutdated)
}

#[cfg(test)]
mod tests {
    use pastegrab_domain::job::{FailureKind, Step};

    use super::classify;

    fn kind(lines: &[&str], step: Step) -> FailureKind {
        let lines: Vec<String> = lines.iter().map(|&line| line.to_owned()).collect();
        classify(&lines, Some(1), step)
    }

    #[test]
    fn recorded_messages() {
        let cases = [
            (
                "ERROR: [youtube] BaW_jenozKc: This video is unavailable",
                FailureKind::Unavailable,
            ),
            (
                "ERROR: [youtube] AAAAAAAAAAA: Sign in to confirm you\u{2019}re not a bot. \
                 Use --cookies-from-browser or --cookies for the authentication.",
                FailureKind::BotCheck,
            ),
            (
                "ERROR: [youtube] AAAAAAAAAAA: Private video. Sign in if you've been granted \
                 access to this video",
                FailureKind::NeedsLogin,
            ),
            (
                "ERROR: [generic] Unable to download webpage: HTTP Error 403: Forbidden \
                 (caused by <HTTPError 403: Forbidden>)",
                FailureKind::BotWall,
            ),
            (
                "ERROR: [generic] Got HTTP Error 403 caused by Cloudflare anti-bot challenge; \
                 try again with --extractor-args \"generic:impersonate\"",
                FailureKind::BotWall,
            ),
            (
                "ERROR: Unsupported URL: https://example.org/news/some-article",
                FailureKind::Unsupported,
            ),
            (
                "ERROR: [youtube] AAAAAAAAAAA: Requested format is not available. Use \
                 --list-formats for a list of available formats",
                FailureKind::EngineOutdated,
            ),
            (
                "ERROR: [youtube] AAAAAAAAAAA: Unable to download API page: <urlopen error \
                 [Errno -3] Temporary failure in name resolution> (caused by \
                 TransportError('<urlopen error [Errno -3] Temporary failure in name \
                 resolution>'))",
                FailureKind::Network,
            ),
            ("ERROR: something new", FailureKind::Internal),
        ];
        for (line, expected) in cases {
            assert_eq!(kind(&[line], Step::Probe), expected, "{line}");
        }
    }

    #[test]
    fn a_403_while_downloading_means_stale_links() {
        let line = "ERROR: unable to download video data: HTTP Error 403: Forbidden";
        assert_eq!(kind(&[line], Step::Fetch), FailureKind::Expired);
    }

    #[test]
    fn the_error_line_outranks_the_warnings_before_it() {
        let lines = [
            "WARNING: [youtube] AAAAAAAAAAA: n challenge solving failed: Some formats may be \
             missing.",
            "ERROR: [youtube] AAAAAAAAAAA: Private video. Sign in if you've been granted access",
        ];
        assert_eq!(kind(&lines, Step::Probe), FailureKind::NeedsLogin);
        assert_eq!(kind(&lines[..1], Step::Probe), FailureKind::EngineOutdated);
    }

    #[test]
    fn a_missing_dll_breaks_the_tool_whatever_it_printed() {
        let lines = vec!["ERROR: Video unavailable".to_owned()];
        let code = Some(0xC000_0135_u32.cast_signed());
        assert_eq!(classify(&lines, code, Step::Probe), FailureKind::ToolBroken);
    }
}
