//! The `@pg|...` and `@pp|...` lines the progress templates print. Parsing allocates nothing.

use pastegrab_app::ports::Progress;

pub(crate) const DOWNLOAD_TEMPLATE: &str = concat!(
    "download:@pg|%(progress.downloaded_bytes)s|%(progress.total_bytes)s",
    "|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s",
);

pub(crate) const POSTPROCESS_TEMPLATE: &str =
    "postprocess:@pp|%(progress.postprocessor)s|%(progress.status)s";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    Download(Sample),
    /// A postprocessor that rewrites the file started.
    Merge,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub done: u64,
    pub total: Option<u64>,
    pub speed: Option<u64>,
    pub eta: Option<u64>,
}

pub(crate) fn parse(line: &[u8]) -> Option<Event> {
    if let Some(rest) = line.strip_prefix(b"@pg|") {
        let mut fields = rest.split(|&b| b == b'|');
        let mut next = || fields.next().and_then(number);
        let done = next()?;
        let total = next();
        let estimate = next();
        return Some(Event::Download(Sample {
            done,
            total: total.or(estimate),
            speed: next(),
            eta: next(),
        }));
    }
    let rest = line.strip_prefix(b"@pp|")?;
    let started = rest.ends_with(b"|started");
    let rewrites = rest.starts_with(b"Merger|") || rest.starts_with(b"FFmpeg");
    Some(if started && rewrites {
        Event::Merge
    } else {
        Event::Other
    })
}

/// `12`, `12.7` cut to 12, or `NA`.
fn number(field: &[u8]) -> Option<u64> {
    if field.iter().any(|b| matches!(b, b'e' | b'E')) {
        return None;
    }
    let whole = field.split(|&b| b == b'.').next()?;
    std::str::from_utf8(whole).ok()?.parse().ok()
}

/// yt-dlp counts each format's file from zero; this adds the parts up into one total.
#[derive(Debug, Default)]
pub(crate) struct Tally {
    base: u64,
    last_done: u64,
    last_total: u64,
}

impl Tally {
    pub(crate) fn add(&mut self, sample: Sample, expected: Option<u64>) -> Progress {
        if sample.done < self.last_done {
            self.base = self
                .base
                .saturating_add(self.last_total.max(self.last_done));
        }
        self.last_done = sample.done;
        self.last_total = sample.total.unwrap_or(sample.done);
        let done = self.base.saturating_add(sample.done);
        let known = sample.total.map(|total| self.base.saturating_add(total));
        Progress {
            done,
            total: match (expected, known) {
                (Some(expected), Some(known)) => Some(expected.max(known)),
                (expected, known) => expected.or(known),
            },
            speed: sample.speed,
            eta: sample.eta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, Sample, Tally, parse};

    fn sample(done: u64, total: Option<u64>) -> Sample {
        Sample {
            done,
            total,
            speed: None,
            eta: None,
        }
    }

    #[test]
    fn download_lines() {
        assert_eq!(
            parse(b"@pg|130048|322379|NA|1727038.230575362|0"),
            Some(Event::Download(Sample {
                done: 130_048,
                total: Some(322_379),
                speed: Some(1_727_038),
                eta: Some(0),
            }))
        );
        assert_eq!(
            parse(b"@pg|2048|NA|9000000.5|NA|NA"),
            Some(Event::Download(sample(2048, Some(9_000_000))))
        );
        assert_eq!(parse(b"@pg|NA|NA|NA|NA|NA"), None);
        assert_eq!(parse(b"@pg|1.5e-05|NA|NA|NA|NA"), None);
    }

    #[test]
    fn postprocessor_lines() {
        assert_eq!(parse(b"@pp|Merger|started"), Some(Event::Merge));
        assert_eq!(parse(b"@pp|FFmpegFixupM4a|started"), Some(Event::Merge));
        assert_eq!(parse(b"@pp|Merger|finished"), Some(Event::Other));
        assert_eq!(parse(b"@pp|MoveFiles|started"), Some(Event::Other));
        assert_eq!(parse(b"WARNING: [youtube] something"), None);
    }

    #[test]
    fn a_second_part_adds_to_the_first() {
        let mut tally = Tally::default();
        let expected = Some(631_667);
        assert_eq!(tally.add(sample(1024, Some(322_379)), expected).done, 1024);
        let first = tally.add(sample(322_379, Some(322_379)), expected);
        assert_eq!((first.done, first.total), (322_379, expected));
        let second = tally.add(sample(1024, Some(309_288)), expected);
        assert_eq!((second.done, second.total), (323_403, expected));
        let end = tally.add(sample(309_288, Some(309_288)), None);
        assert_eq!((end.done, end.total), (631_667, Some(631_667)));
    }
}
