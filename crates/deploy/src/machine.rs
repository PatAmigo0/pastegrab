//! What setup learns about the PC, the defaults it picks from that, and `deploy.json`, where the
//! app finds both without probing the hardware on every start.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files::write_whole;

const FILE: &str = "deploy.json";

/// The PC as setup found it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    /// Major, minor and build, such as `6.3.9600`.
    pub windows: String,
    /// The CPU's name as Windows reports it.
    pub cpu: String,
    /// Logical processors.
    pub cores: u32,
    /// Instruction set extensions ffmpeg and x264 use, such as `avx2`.
    pub features: Vec<String>,
    /// Installed memory in MiB.
    pub ram_mib: u64,
    /// Free space on the install drive in MiB.
    pub free_mib: u64,
    /// Whether the system disk spins; `None` when Windows doesn't say.
    pub hdd: Option<bool>,
    /// The primary screen in pixels.
    pub screen: (u32, u32),
    /// Whether Windows has the universal C runtime the Python under yt-dlp needs.
    pub ucrt: bool,
}

/// The x264 presets pastegrab encodes with, fastest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// For two cores or fewer.
    Superfast,
    /// For three or four.
    Veryfast,
    /// For more.
    Faster,
}

impl Preset {
    /// The name x264 takes.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Superfast => "superfast",
            Self::Veryfast => "veryfast",
            Self::Faster => "faster",
        }
    }
}

/// The defaults setup picks for the PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuning {
    /// Downloads at once, 1 to 4.
    pub downloads: u8,
    /// The x264 preset.
    pub preset: Preset,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            downloads: 2,
            preset: Preset::Veryfast,
        }
    }
}

impl Machine {
    /// Downloads by memory, since each runs a Python of its own; the preset by the cores x264
    /// spreads an encode over.
    #[must_use]
    pub const fn tuning(&self) -> Tuning {
        let downloads = match self.ram_mib {
            ..3072 => 1,
            3072..6144 => 2,
            6144..12288 => 3,
            _ => 4,
        };
        let preset = match self.cores {
            ..=2 => Preset::Superfast,
            3..=4 => Preset::Veryfast,
            _ => Preset::Faster,
        };
        Tuning { downloads, preset }
    }
}

/// `deploy.json` in pastegrab's data folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    /// The PC.
    pub machine: Machine,
    /// The defaults picked for it.
    pub tuning: Tuning,
    /// Each tool and the version it reported in setup's self-test.
    pub tools: Vec<(String, String)>,
}

/// `deploy.json` from the data folder; `None` when it is missing or unreadable.
#[must_use]
pub fn read(data: &Path) -> Option<Deployment> {
    let text = std::fs::read(data.join(FILE)).ok()?;
    serde_json::from_slice(&text).ok()
}

/// Replaces `deploy.json` whole.
///
/// # Errors
///
/// Errors from the file system.
pub fn write(data: &Path, deployment: &Deployment) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(deployment).map_err(io::Error::other)?;
    write_whole(&data.join(FILE), &json)
}

#[cfg(test)]
mod tests {
    use super::{Deployment, Machine, Preset, Tuning, read, write};

    fn machine(cores: u32, ram_mib: u64) -> Machine {
        Machine {
            windows: "6.3.9600".into(),
            cpu: "Intel(R) Core(TM) i3-2120 CPU @ 3.30GHz".into(),
            cores,
            ram_mib,
            ..Machine::default()
        }
    }

    #[test]
    fn memory_sets_parallel_downloads_and_cores_set_the_preset() {
        let cases = [
            (2, 2048, 1, Preset::Superfast),
            (2, 4096, 2, Preset::Superfast),
            (4, 4096, 2, Preset::Veryfast),
            (4, 8192, 3, Preset::Veryfast),
            (8, 16384, 4, Preset::Faster),
        ];
        for (cores, ram, downloads, preset) in cases {
            let tuning = machine(cores, ram).tuning();
            assert_eq!(
                tuning,
                Tuning { downloads, preset },
                "{cores} cores, {ram} MiB"
            );
        }
        assert_eq!(Preset::Veryfast.name(), "veryfast");
    }

    #[test]
    fn deploy_json_survives_a_round_trip_and_a_broken_one_reads_as_none() {
        let dir = std::env::temp_dir().join(format!("pastegrab-deploy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let machine = machine(4, 8192);
        let deployment = Deployment {
            tuning: machine.tuning(),
            machine,
            tools: vec![(
                "ffmpeg".into(),
                "8.1.1-essentials_build-www.gyan.dev".into(),
            )],
        };
        write(&dir, &deployment).expect("a save");
        assert_eq!(read(&dir), Some(deployment));
        std::fs::write(dir.join("deploy.json"), "{").expect("a broken file");
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
