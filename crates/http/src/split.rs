//! One large file over several connections: the first keeps streaming from byte 0 while the
//! others steal chunks from the back of the file so no single slow connection leaves a long tail.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use pastegrab_app::ports::{Phase, ProgressSink};
use pastegrab_app::{Cancel, Failure};
use pastegrab_domain::job::FailureKind;
use ureq::Agent;

use crate::{BUFFER, REPORT_EVERY, io_failure, request_failure, sample};

const MIN_CHUNK: u64 = 2 * 1024 * 1024;
const MAX_CHUNK: u64 = 8 * 1024 * 1024;

/// Shared unclaimed interval `[front, back)` of the file.
struct Work {
    bounds: Mutex<(u64, u64)>,
    connections: u64,
    min_chunk: u64,
}

impl Work {
    /// Reserves one initial chunk per tail worker and leaves the rest in `[front, back)`.
    fn new(start: u64, total: u64, connections: usize) -> (Self, Vec<(u64, u64)>) {
        let count = u64::try_from(connections.max(1)).unwrap_or(1);
        let start = start.min(total);
        let min_chunk = ((total - start) / count).clamp(1, MIN_CHUNK);
        let work = Self {
            bounds: Mutex::new((start, total)),
            connections: count,
            min_chunk,
        };
        let mut initial = Vec::with_capacity(connections.saturating_sub(1));
        for _ in 1..connections {
            match work.take_back() {
                Some(chunk) => initial.push(chunk),
                None => break,
            }
        }
        (work, initial)
    }

    /// Claims up to `max` bytes from the front for the already-open stream.
    fn take_front(&self, max: u64) -> u64 {
        let mut bounds = self
            .bounds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let take = bounds.1.saturating_sub(bounds.0).min(max);
        bounds.0 += take;
        take
    }

    /// Claims the next chunk from the back, leaving at least `min_chunk` for the front stream.
    fn take_back(&self) -> Option<(u64, u64)> {
        let mut bounds = self
            .bounds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rem = bounds.1.saturating_sub(bounds.0);
        if rem < self.min_chunk.saturating_mul(2) {
            return None;
        }
        let max_steal = rem - self.min_chunk;
        let size = (rem / self.connections)
            .clamp(self.min_chunk, MAX_CHUNK)
            .min(max_steal);
        let end = bounds.1;
        let start = end - size;
        bounds.1 = start;
        Some((start, end))
    }
}

/// One file to fetch in ranges.
pub(crate) struct Split<'a> {
    pub(crate) agent: &'a Agent,
    pub(crate) url: &'a str,
    pub(crate) file: &'a Path,
    pub(crate) total: u64,
    pub(crate) connections: usize,
}

impl Split<'_> {
    /// Downloads the whole file; `first` already streams from byte 0, and `head` holds its first
    /// bytes.
    pub(crate) fn download(
        &self,
        mut first: impl Read + Send,
        head: &[u8],
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<(), Failure> {
        let mut file = File::create(self.file).map_err(|e| io_failure(&e, "creating the file"))?;
        file.set_len(self.total)
            .and_then(|()| file.write_all(head))
            .map_err(|e| io_failure(&e, "writing the file"))?;
        let head_len = u64::try_from(head.len()).unwrap_or(u64::MAX);
        let done = AtomicU64::new(head_len);
        let aborted = AtomicBool::new(false);
        let (work, initial) = Work::new(head_len, self.total, self.connections);
        let started = Instant::now();
        let file = thread::scope(|scope| {
            let mut workers = Vec::with_capacity(1 + initial.len());
            let front_worker = scope.spawn(|| {
                let result = stream_front(&mut first, &mut file, &work, &done, cancel, &aborted);
                if result.is_err() {
                    aborted.store(true, Ordering::Relaxed);
                }
                result.map(|()| file)
            });
            for chunk in initial {
                let (work, done, aborted) = (&work, &done, &aborted);
                workers.push(scope.spawn(move || {
                    let result = self.tail_worker(chunk, work, done, cancel, aborted);
                    if result.is_err() {
                        aborted.store(true, Ordering::Relaxed);
                    }
                    result
                }));
            }
            let mut reported = started;
            while !front_worker.is_finished() || workers.iter().any(|worker| !worker.is_finished())
            {
                thread::sleep(Duration::from_millis(50));
                if reported.elapsed() >= REPORT_EVERY {
                    reported = Instant::now();
                    let sofar = done.load(Ordering::Relaxed);
                    progress.report(Phase::Download, sample(sofar, Some(self.total), started));
                }
            }
            let front = front_worker
                .join()
                .unwrap_or_else(|_| Err(Failure::internal("a download thread panicked")));
            for worker in workers {
                worker
                    .join()
                    .unwrap_or_else(|_| Err(Failure::internal("a download thread panicked")))?;
            }
            front
        })?;
        file.sync_all()
            .map_err(|e| io_failure(&e, "flushing the file"))?;
        progress.report(
            Phase::Download,
            sample(self.total, Some(self.total), started),
        );
        Ok(())
    }

    fn tail_worker(
        &self,
        first_chunk: (u64, u64),
        work: &Work,
        done: &AtomicU64,
        cancel: &Cancel,
        aborted: &AtomicBool,
    ) -> Result<(), Failure> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(self.file)
            .map_err(|e| io_failure(&e, "opening the file"))?;
        let mut buf = vec![0; BUFFER];
        let mut next = Some(first_chunk);
        while let Some((start, end)) = next {
            if cancel.is_cancelled() || aborted.load(Ordering::Relaxed) {
                return Err(Failure::internal("cancelled"));
            }
            self.fetch_range(&mut file, &mut buf, start, end, done, cancel, aborted)?;
            next = work.take_back();
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared buffers and stop flags across range chunks"
    )]
    fn fetch_range(
        &self,
        file: &mut File,
        buf: &mut [u8],
        start: u64,
        end: u64,
        done: &AtomicU64,
        cancel: &Cancel,
        aborted: &AtomicBool,
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
        file.seek(SeekFrom::Start(start))
            .map_err(|e| io_failure(&e, "seeking in the file"))?;
        copy(
            &mut response.body_mut().as_reader(),
            file,
            buf,
            end - start,
            done,
            cancel,
            aborted,
        )
    }
}

fn stream_front(
    first: &mut impl Read,
    file: &mut File,
    work: &Work,
    done: &AtomicU64,
    cancel: &Cancel,
    aborted: &AtomicBool,
) -> Result<(), Failure> {
    let mut buf = vec![0; BUFFER];
    let step = u64::try_from(BUFFER).unwrap_or(u64::MAX);
    loop {
        let len = work.take_front(step);
        if len == 0 {
            return Ok(());
        }
        copy(first, file, &mut buf, len, done, cancel, aborted)?;
    }
}

/// Copies exactly `len` bytes from `from` into `to`, counting them in `done`.
fn copy(
    from: &mut impl Read,
    to: &mut File,
    buf: &mut [u8],
    len: u64,
    done: &AtomicU64,
    cancel: &Cancel,
    aborted: &AtomicBool,
) -> Result<(), Failure> {
    let mut left = len;
    while left > 0 {
        if cancel.is_cancelled() || aborted.load(Ordering::Relaxed) {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_CHUNK, MIN_CHUNK, Work};

    #[test]
    fn small_split_reserves_one_chunk_per_tail_worker_with_no_overlap() {
        let (work, mut chunks) = Work::new(64, 3_000_000, 4);
        assert_eq!(chunks.len(), 3);
        assert_eq!(work.take_back(), None);
        let mut front_total = 0;
        loop {
            let n = work.take_front(256 * 1024);
            if n == 0 {
                break;
            }
            front_total += n;
        }
        chunks.push((64, 64 + front_total));
        chunks.sort_unstable();
        let mut cursor = 64;
        for (start, end) in chunks {
            assert_eq!(start, cursor);
            assert!(end > start);
            cursor = end;
        }
        assert_eq!(cursor, 3_000_000);
    }

    #[test]
    fn large_split_tapers_chunks_and_covers_every_byte_once() {
        let total = 164_088_517;
        let (work, mut chunks) = Work::new(64, total, 4);
        assert_eq!(chunks.len(), 3);
        for &(start, end) in &chunks {
            assert_eq!(end - start, MAX_CHUNK);
        }
        let mut front = 64;
        while let Some(chunk) = work.take_back() {
            assert!((MIN_CHUNK..=MAX_CHUNK).contains(&(chunk.1 - chunk.0)));
            chunks.push(chunk);
            front += work.take_front(256 * 1024);
        }
        loop {
            let n = work.take_front(256 * 1024);
            if n == 0 {
                break;
            }
            front += n;
        }
        chunks.push((64, front));
        chunks.sort_unstable();
        let mut cursor = 64;
        for (start, end) in chunks {
            assert_eq!(start, cursor);
            assert!(end > start);
            cursor = end;
        }
        assert_eq!(cursor, total);
    }
}
