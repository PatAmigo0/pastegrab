//! The report on setup's last page: the PC, how each tool answered, and the defaults picked.

use pastegrab_deploy::machine::{Machine, Tuning};
use pastegrab_deploy::selftest::{Check, Probe};
use pastegrab_i18n::Strings;

/// The report, one fact per line.
pub(crate) fn report(machine: &Machine, tuning: Tuning, checks: &[Check], s: &Strings) -> String {
    let r = &s.setup.report;
    let (name, build) = windows_name(&machine.windows);
    let mut lines = vec![
        (r.windows)(&name, &build),
        (r.cpu)(&machine.cpu, machine.cores, &machine.features.join(" ")),
        (r.memory)(machine.ram_mib),
        (r.disk)(machine.free_mib, machine.hdd),
        (r.screen)(machine.screen.0, machine.screen.1),
    ];
    if !machine.ucrt {
        lines.push(r.no_ucrt.to_owned());
    }
    lines.extend(checks.iter().map(|check| {
        let tenths = (check.took.as_millis().saturating_add(50) / 100)
            .try_into()
            .unwrap_or(u64::MAX);
        match (&check.result, check.probe) {
            (Ok(_), Probe::Encode) => (r.encode)(tenths),
            (Ok(version), probe) if version.is_empty() => (r.works)(tool(probe), tenths),
            (Ok(version), probe) => (r.works)(&format!("{} {version}", tool(probe)), tenths),
            (Err(reason), probe) => (r.broken)(tool(probe), reason),
        }
    }));
    lines.push((r.tuning)(tuning.downloads, tuning.preset.name()));
    lines.join("\n")
}

/// Whether every check answered.
pub(crate) fn all_work(checks: &[Check]) -> bool {
    checks.iter().all(|check| check.result.is_ok())
}

/// The tool's usual name; names stay in Latin script in Russian text.
pub(crate) const fn tool(probe: Probe) -> &'static str {
    match probe {
        Probe::YtDlp => "yt-dlp",
        Probe::Ffmpeg => "ffmpeg",
        Probe::Ffprobe => "ffprobe",
        Probe::QuickJs => "QuickJS",
        Probe::Encode => "x264",
    }
}

/// "8.1" and "9600" from "6.3.9600".
fn windows_name(version: &str) -> (String, String) {
    let mut parts = version.splitn(3, '.');
    let (major, minor, build) = (parts.next(), parts.next(), parts.next());
    let name = match (major, minor) {
        (Some("6"), Some("1")) => "7".to_owned(),
        (Some("6"), Some("2")) => "8".to_owned(),
        (Some("6"), Some("3")) => "8.1".to_owned(),
        (Some(major), Some("0")) => major.to_owned(),
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => version.to_owned(),
    };
    (name, build.unwrap_or_default().to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pastegrab_deploy::machine::{Machine, Preset, Tuning};
    use pastegrab_deploy::selftest::{Check, Probe};
    use pastegrab_i18n::RU;

    use super::{all_work, report};

    fn machine() -> Machine {
        Machine {
            windows: "6.3.9600".into(),
            cpu: "Intel(R) Core(TM) i3-2120 CPU @ 3.30GHz".into(),
            cores: 4,
            features: vec!["sse4.2".into(), "avx".into()],
            ram_mib: 4096,
            free_mib: 120_000,
            hdd: Some(true),
            screen: (1366, 768),
            ucrt: true,
        }
    }

    fn check(probe: Probe, result: Result<&str, &str>, millis: u64) -> Check {
        Check {
            probe,
            result: result.map(str::to_owned).map_err(str::to_owned),
            took: Duration::from_millis(millis),
        }
    }

    #[test]
    fn the_report_names_the_pc_each_tool_and_the_defaults() {
        let checks = [
            check(Probe::YtDlp, Ok("2026.10.26"), 1240),
            check(Probe::Ffmpeg, Ok("8.1.1-essentials_build-www.gyan.dev"), 90),
            check(Probe::Encode, Ok(""), 810),
            check(Probe::QuickJs, Err("no answer in 120000 ms"), 120_000),
        ];
        let tuning = Tuning {
            downloads: 2,
            preset: Preset::Veryfast,
        };
        assert_eq!(
            report(&machine(), tuning, &checks, &RU),
            "Windows 8.1, сборка 9600\n\
             Процессор: Intel(R) Core(TM) i3-2120 CPU @ 3.30GHz, потоков: 4, sse4.2 avx\n\
             Память: 4 ГБ\n\
             Свободно на диске: 117 ГБ, жёсткий диск\n\
             Экран: 1366×768\n\
             yt-dlp 2026.10.26: работает, 1,2 с\n\
             ffmpeg 8.1.1-essentials_build-www.gyan.dev: работает, 0,1 с\n\
             Кодирование секунды видео: 0,8 с\n\
             QuickJS: не работает (no answer in 120000 ms)\n\
             Загрузок одновременно: 2, кодирование x264: veryfast"
        );
        assert!(!all_work(&checks));
        assert!(all_work(&checks[..3]));
    }

    #[test]
    fn a_missing_runtime_and_an_older_windows_read_plainly() {
        let mut old = machine();
        old.windows = "6.2.9200".into();
        old.ucrt = false;
        old.hdd = None;
        old.ram_mib = 3584;
        let checks = [check(Probe::QuickJs, Ok(""), 140)];
        let text = report(&old, Tuning::default(), &checks, &RU);
        assert!(text.contains("\nQuickJS: работает, 0,1 с\n"), "{text}");
        assert!(text.starts_with("Windows 8, сборка 9200\n"), "{text}");
        assert!(text.contains("Память: 3,5 ГБ\n"), "{text}");
        assert!(text.contains("Свободно на диске: 117 ГБ\n"), "{text}");
        assert!(
            text.contains("Компонентов Windows для Python нет"),
            "{text}"
        );
    }
}
