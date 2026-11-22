//! Release files: small ones read whole, packages streamed to disk, each refused once it passes
//! the size its caller expects.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use ureq::Agent;

/// Plain downloads, for app updates.
#[derive(Debug, Clone)]
pub struct Downloads {
    agent: Agent,
}

impl Default for Downloads {
    fn default() -> Self {
        let agent = Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .user_agent(concat!("pastegrab/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }
}

impl Downloads {
    /// The body of `url`.
    ///
    /// # Errors
    ///
    /// `NotFound` for a missing file, `FileTooLarge` past `limit` bytes, or the network's error.
    pub fn read(&self, url: &str, limit: u64) -> io::Result<Vec<u8>> {
        let mut response = self.agent.get(url).call().map_err(io_error)?;
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit.saturating_add(1))
            .read_to_end(&mut body)?;
        if u64::try_from(body.len()).unwrap_or(u64::MAX) > limit {
            return Err(too_large(limit));
        }
        Ok(body)
    }

    /// Streams `url` into `dest`, which holds nothing after a failure.
    ///
    /// # Errors
    ///
    /// As `read`, plus the disk's.
    pub fn save(&self, url: &str, dest: &Path, limit: u64) -> io::Result<()> {
        let saved = self.stream(url, dest, limit);
        if saved.is_err() {
            let _ = fs::remove_file(dest);
        }
        saved
    }

    fn stream(&self, url: &str, dest: &Path, limit: u64) -> io::Result<()> {
        let mut response = self.agent.get(url).call().map_err(io_error)?;
        let mut file = File::create(dest)?;
        let mut body = response
            .body_mut()
            .as_reader()
            .take(limit.saturating_add(1));
        if io::copy(&mut body, &mut file)? > limit {
            return Err(too_large(limit));
        }
        file.sync_all()
    }
}

fn io_error(e: ureq::Error) -> io::Error {
    match e {
        ureq::Error::Io(e) => e,
        ureq::Error::StatusCode(404 | 410) => {
            io::Error::new(io::ErrorKind::NotFound, e.to_string())
        }
        other => io::Error::other(other),
    }
}

fn too_large(limit: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        format!("longer than the {limit} bytes expected"),
    )
}
