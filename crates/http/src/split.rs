//! One large file over several connections: the first keeps reading from byte 0, the others
//! fetch their byte ranges, and each writes at its own offset of the one file.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use pastegrab_app::ports::{Phase, ProgressSink};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;
use ureq::Agent;

use crate::{BUFFER, REPORT_EVERY, io_failure, request_failure, sample};

/// Byte ranges, start inclusive and end exclusive, that split `total` evenly.
pub(crate) fn ranges(total: u64, connections: usize) -> Vec<(u64, u64)> {
    let count = u64::try_from(connections.max(1)).unwrap_or(1);
    let size = total.div_ceil(count);
    (0..count)
        .map(|i| (i * size, ((i + 1) * size).min(total)))
        .filter(|(start, end)| start < end)
        .collect()
}

/// One file to fetch in ranges.
pub(crate) struct Split<'a> {
    pub(crate) agent: &'a Agent,
    pub(crate) url: &'a str,
    pub(crate) file: &'a Path,
    pub(crate) total: u64,
}

impl Split<'_> {
    /// Downloads every range of `ranges`; `first` already streams from byte 0, and `head` holds
    /// its first bytes.
    pub(crate) fn download(
        &self,
        first: impl Read + Send,
        head: &[u8],
        ranges: &[(u64, u64)],
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<(), Failure> {
        let mut file = File::create(self.file).map_err(|e| io_failure(&e, "creating the file"))?;
        file.set_len(self.total)
            .and_then(|()| file.write_all(head))
            .map_err(|e| io_failure(&e, "writing the file"))?;
        let done = AtomicU64::new(u64::try_from(head.len()).unwrap_or(u64::MAX));
        let first_end = ranges.first().map_or(self.total, |&(_, end)| end);
        let started = Instant::now();
        thread::scope(|scope| {
            let mut workers = vec![scope.spawn(|| {
                let left = first_end.saturating_sub(u64::try_from(head.len()).unwrap_or(0));
                copy(first, file, left, &done, cancel)
            })];
            workers.extend(ranges.iter().skip(1).map(|&(start, end)| {
                let done = &done;
                scope.spawn(move || self.fetch_range(start, end, done, cancel))
            }));
            while workers.iter().any(|worker| !worker.is_finished()) {
                thread::sleep(Duration::from_millis(100));
                if started.elapsed() >= REPORT_EVERY {
                    let sofar = done.load(Ordering::Relaxed);
                    progress.report(Phase::Download, sample(sofar, Some(self.total), started));
                }
            }
            workers.into_iter().try_for_each(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err(Failure::internal("a download thread panicked")))
            })
        })?;
        progress.report(
            Phase::Download,
            sample(self.total, Some(self.total), started),
        );
        Ok(())
    }

    fn fetch_range(
        &self,
        start: u64,
        end: u64,
        done: &AtomicU64,
        cancel: &Cancel,
    ) -> Result<(), Failure> {
        let mut response = self
            .agent
            .get(self.url)
            .header("Range", format!("bytes={start}-{}", end - 1))
            .call()
            .map_err(request_failure)?;
        let served_from = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("bytes "))
            .and_then(|value| value.split('-').next())
            .and_then(|value| value.parse::<u64>().ok());
        if response.status().as_u16() != 206 || served_from != Some(start) {
            return Err(Failure::new(
                FailureKind::Network,
                format!("the server didn't serve bytes {start} to {end}"),
            ));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .open(self.file)
            .map_err(|e| io_failure(&e, "opening the file"))?;
        file.seek(SeekFrom::Start(start))
            .map_err(|e| io_failure(&e, "seeking in the file"))?;
        copy(
            response.body_mut().as_reader(),
            file,
            end - start,
            done,
            cancel,
        )
    }
}

/// Copies exactly `len` bytes from `from` into `to`, counting them in `done`.
fn copy(
    mut from: impl Read,
    mut to: File,
    len: u64,
    done: &AtomicU64,
    cancel: &Cancel,
) -> Result<(), Failure> {
    let mut buf = vec![0; BUFFER];
    let mut left = len;
    while left > 0 {
        if cancel.is_cancelled() {
            return Err(Failure::internal("cancelled"));
        }
        let want = usize::try_from(left).unwrap_or(usize::MAX).min(buf.len());
        let n = match from.read(&mut buf[..want]) {
            Ok(0) => {
                return Err(Failure::new(
                    FailureKind::Network,
                    "the connection closed early",
                ));
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Failure::new(FailureKind::Network, e.to_string())),
        };
        to.write_all(&buf[..n])
            .map_err(|e| io_failure(&e, "writing the file"))?;
        let n = u64::try_from(n).unwrap_or(u64::MAX);
        left = left.saturating_sub(n);
        done.fetch_add(n, Ordering::Relaxed);
    }
    to.sync_all()
        .map_err(|e| io_failure(&e, "flushing the file"))
}

#[cfg(test)]
mod tests {
    use super::ranges;

    #[test]
    fn ranges_cover_the_file_once_in_order() {
        assert_eq!(ranges(10, 4), [(0, 3), (3, 6), (6, 9), (9, 10)]);
        assert_eq!(ranges(3, 4), [(0, 1), (1, 2), (2, 3)]);
        assert_eq!(ranges(8, 1), [(0, 8)]);
    }
}
