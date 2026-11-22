# pastegrab handoff

Written 2026-11-22. The repository is `~/projects/pastegrab`, public at github.com/PatAmigo0/pastegrab. Branch `main` sits at `6de37e5`, clean and pushed. Release 0.2.0 is GitHub's latest release; 0.1.1 is a leftover test pre-release.

`SPEC.md`, 660 lines long, holds the full design. This file summarizes it and adds what the spec doesn't record: what is built, what is tested, what isn't, and how the owner wants the work done. It stays outside the repository because it names details of the owner's machine.

## The product

**pastegrab is a Windows desktop app that takes pasted text with a link in it and saves the video, or only its sound, as a file Windows 8.x plays without extra codecs.** It wraps yt-dlp and ffmpeg behind a native Win32 window. The UI is Russian.

One person uses it. He is an older, non-technical man who downloads old Olympic and sports footage to study running technique in ProSportsAnalyzer, another program the owner wrote for him. Nobody can reach his PC or ask him for specs. The app therefore installs from one file, updates itself, rolls back a bad update on its own and explains failures in plain words.

Because ProSportsAnalyzer 1.8 runs on his PC, we know his Windows is a 64-bit 8.0 or 8.1 with the UCRT installed, and that gyan.dev's ffmpeg 8.1.1 essentials already works there. His CPU, RAM, disk and Windows version stay unknown until setup measures them.

His main source, olympics.com, sits behind Akamai. From the lab's network it returned 403 to yt-dlp even with `--impersonate chrome`. Nobody has tried it from a home connection yet; section 15 of the spec has the one-line check.

## Platform constraints

Windows 8 decides most of the design.

- Rust's tier 1 Windows targets need Windows 10. pastegrab builds the app, launcher and pgcli for the Tier 3 `x86_64-win7-windows-gnu` target and the setup for `i686-win7-windows-gnu`, with `-Zbuild-std=std,panic_unwind` on `nightly-2026-11-20` and mingw-w64 inside the podman container in `tools/build/rust`. The binaries import only DLLs Windows 8 ships, `msvcrt.dll` included.
- yt-dlp's own Windows builds need Windows 10 since 2026.10.26. pastegrab ships python-build-standalone 3.11.18 running yt-dlp 2026.10.26's pure-Python release.
- YouTube needs a JS runtime, and Node and Deno don't start on 8.x. pastegrab ships Bellard's QuickJS 2026-06-04, built with mingw by `tools/build/quickjs`.
- ffmpeg and ffprobe are gyan.dev's 8.1.1 essentials, the build ProSportsAnalyzer uses.
- WebView2 stopped at 109 on 8.1, which rules out Tauri and any HTML UI.
- SChannel on 8.x has no TLS 1.3 and a stale root store. pastegrab's own code uses rustls with `ring` and bundled webpki roots.
- Windows 8 has no long-path support, so output paths stay under 260 UTF-16 units.
- Codegen stays baseline x86-64 with no `target-cpu`.
- The Python under yt-dlp needs the UCRT. Setup carries Microsoft's `vc_redist.x64.exe` 14.44.35211.0 and runs it behind UAC when `ucrtbase.dll` is missing.

## Workspace

**Ports and adapters across a Cargo workspace, edition 2024.** `domain` and `app` are pure and build and test on Linux. Adapters depend on `app`, no adapter depends on another, and only `apps/*` see adapters.

| Crate | Holds |
|---|---|
| `crates/domain` | link pipeline (find, parse, unwrap redirect wrappers, strip trackers, classify by a site table), format planner, file naming, job state machine, scheduler; no I/O |
| `crates/app` | ports, the queue and core loop, steps, the `Board` view model with rows, `Activity` and `clear_ended`, saved choices |
| `crates/ytdlp` | Resolver and Fetcher over yt-dlp: fixed flags, probe and fetch arguments, progress parser, failure table, and the hand-off of one plain https format to the direct fetcher (`plain.rs`, `YtDlp::with_direct`) |
| `crates/ffmpeg` | Transcoder and MediaProbe; arguments are a pure function of the planner's `Convert`, with the x264 preset from `deploy.json` |
| `crates/http` | ureq with rustls: the direct fetcher `Direct`, split downloads in `split.rs`, size-limited downloads in `downloads.rs` |
| `crates/os` | ProcessRunner (job objects on Windows, process groups on Linux), file operations, `Zone.Identifier`, known folders |
| `crates/store` | `settings.json` written with flush and rename; the loader skips a UTF-8 BOM |
| `crates/win32` | the window; every Win32 call lives in `src/sys/` |
| `crates/cli` | the terminal view for pgcli |
| `crates/i18n` | one `const Strings` per language, Russian only; messages with numbers are functions that handle plurals |
| `crates/deploy` | install layout, `state.json`, machine profile and tuning, `deploy.json`, self-test, unpacking, shortcuts and uninstall entry, UAC elevation, the app updater |
| `crates/update` | pure: `Version`, the signed manifest check (`Updater`, `Offer`, `Refusal`), the launcher's trial and rollback (`Launch`, `Start`) |
| `crates/winres` | icon (`pastegrab.svg` rendered to `pastegrab.ico`), version info and manifest through embed-resource |
| `crates/secrets` | reads `secrets.toml` at build time for the future Telegram token and compiles it XOR-masked |
| `crates/testkit` | fakes, a scripted process runner, contract suites for the ports |
| `apps/window` | `pastegrab-app.exe`, the GUI's composition root: wiring, update check, uninstall |
| `apps/launcher` | `pastegrab.exe`, which starts `app\<version>\pastegrab-app.exe`, counts trial starts and rolls back |
| `apps/pgcli` | the CLI, with `--profile`, `--dest`, `--screen`, `--tools` and `--plan` |
| `apps/setup` | `pastegrab-setup.exe`, the 32-bit installer with the payload embedded |

Lints and review hold these rules:

- `unsafe` appears only in the `sys` modules of `os`, `win32` and `deploy`, in `apps/setup/src/ui.rs` and in `apps/launcher/src/sys.rs`. Every block carries a `// SAFETY:` comment. The workspace denies `unsafe_code`, and pure crates forbid it.
- Clippy runs at pedantic level with `-D warnings`. `unwrap`, `expect`, `panic!`, `todo!`, `dbg!`, `print!` and `eprint!` are denied, and `missing_docs` warns.
- A trait exists only when it has two implementations, the adapter plus a fake. A port has at most three methods. Variation lives in static tables, never in new `match` arms in `app`.
- There is no async runtime. A UI thread, one core thread that owns all state and a worker thread per running step talk through channels. Progress travels through atomics with a coalesced `WM_APP` wake-up. The scheduler's permits allow two probes, 1 to 4 fetches (from `deploy.json`) and one conversion at a time.
- The release profile uses `opt-level 3`, fat LTO, one codegen unit, `panic = "unwind"` so a worker panic fails one job, and overflow checks. The launcher builds with `opt-level "z"` in the `size` profile and weighs 412 KB.

Neither the spec's `cargo-deny` config nor `xtask imports`, which would compare every PE import against the exports of both lab VMs, exists yet. A load test inside the VMs stands in for the second.

## How a download runs

1. Pasted text goes through the link pipeline. A link to a media file skips yt-dlp.
2. The probe runs `python.exe -I <tools>\yt-dlp\<version>` with the fixed flags, `--skip-download --write-info-json` and a `summary.json` print template, and from that summary the planner picks exact format ids such as `137+140`, never a selector.
3. The fetch loads the probe's info JSON back with `--load-info-json`. The site gets extracted once, and YouTube's JS challenge, about 4.3 s on the VMs, gets solved once.
4. When the plan is one plain https format with no merge, no section and no impersonation, the yt-dlp adapter hands it to the direct fetcher. A file of 8 MiB or more whose server answers `Range: bytes=0-` with 206 comes in over four ranged connections into a preallocated file; anything else streams over one. If that fails for any reason except a cancel, yt-dlp downloads it.
5. ffprobe reads the real file. The convert step remuxes with `-c copy` where the codecs fit and otherwise encodes with libx264 at the tuned preset and crf 23, AAC at 160k and MP3 at `-q:a 2`, adding `bwdif` for interlaced input. ffmpeg runs at below-normal priority.
6. The file moves to a name built from the title, with characters Windows forbids removed, reserved names avoided and a « (2)» suffix instead of an overwrite. It gets a `Zone.Identifier` stream with `ZoneId=3`.

Every child starts suspended with `CREATE_NO_WINDOW` inside a kill-on-close job object, and a cancel terminates the job. yt-dlp runs with `--encoding utf-8`. TEMP points at the 8.3 short path of `<data>\tmp` because QuickJS opens its script through the ANSI code page.

| Profile | Output |
|---|---|
| «Видео», the default | MP4, H.264 and AAC, up to the screen's height |
| «Видео, лучшее качество» | MKV, best streams, VP9 or AV1 included |
| «Видео, малый размер» | MP4, 480p at most |
| «Только звук» | M4A |
| «Только звук, MP3» | MP3 |
| «Для анализа» | MP4, frame rate ranked first, up to 1080p, a keyframe every second |

Failure kinds sit in `crates/ytdlp/src/failure.rs`: EngineOutdated, BotWall, BotCheck, NeedsLogin, Unavailable, NotStarted, Unsupported, Expired, Network, DiskFull, ToolBroken and Internal. Each has a Russian message in `crates/win32/src/text.rs`. BotWall retries once with `--impersonate chrome`.

## Files on his PC

```
%LOCALAPPDATA%\Programs\pastegrab\
  pastegrab.exe       the launcher; shortcuts and the uninstall entry start it
  state.json          the version to start, the trial count, rolled-back versions
  app\<version>\      pastegrab-app.exe, pgcli.exe
  tools\              python with yt-dlp, qjs, ffmpeg
%LOCALAPPDATA%\pastegrab\
  settings.json       last folder and profile
  deploy.json         machine profile, tuning, tool paths and versions, written by setup
  update-checked, update-last.txt
  jobs\  tmp\  cache\yt-dlp\
```

Everything is per-user. Only the optional UCRT install asks for admin rights.

## Setup

**`pastegrab-setup.exe`, an i686 exe of 83 MB for 0.2.0, carries the whole install folder as one tar.zst, compressed at level 19 with a 128 MiB window, plus the VC++ redistributable.** A task dialog asks first. Then setup unpacks next to the install folder and swaps it in, writes `state.json`, adds desktop and Start menu shortcuts and an HKCU uninstall entry (`pastegrab.exe --uninstall`), and probes the machine. Without `ucrtbase.dll` it runs the redistributable behind UAC and probes again. A self-test follows, with a 120 s limit per check: yt-dlp `--version`, ffmpeg and ffprobe `-version`, a one-second x264 encode of `testsrc` at the tuned preset, and QuickJS `print(6*7)`. Setup writes `deploy.json` and ends on a page with «Открыть pastegrab» and a report under «Подробнее о компьютере». A 32-bit Windows gets a plain Russian explanation instead of an install.

Setup tunes two settings. Parallel downloads follow RAM, with 1 under 3 GiB, 2 under 6 GiB, 3 under 12 GiB and 4 above that. The x264 preset follows the core count, superfast for 2 or fewer, veryfast for up to 4 and faster beyond. The app reads both from `deploy.json`.

The uninstall removes the install folder, the data folder, both shortcuts and the entry, and leaves downloaded videos alone. A running exe can't delete itself, so it moves to `%TEMP%` and stays there.

## Updates and rollback

**Once a day, after its window shows, the app fetches `update.json` and `update.json.minisig` from `https://github.com/PatAmigo0/pastegrab/releases/latest/download/`.** The manifest is `{version, expires, packages: [{target: "x86_64-windows", url, sha256, size}]}`, signed with minisign in prehashed mode and checked by `minisign-verify` against the key compiled in from `keys/release.pub`, key ID 7C1667CCFFD7F347. The app refuses a version that isn't newer and a manifest past its expiry, which packaging sets a year ahead. The package unpacks into `app\<version>\` and goes on trial.

The launcher starts the version `state.json` names and counts starts of a version on trial. The window confirms its own version once it is on screen, which ends the trial. After two starts that never reached the window, the third start runs the previous version and lists the new one as rolled back. A start that only hands links to an open window, found by its window class and fed through `WM_COPYDATA`, doesn't count.

Only `app\<version>\` updates this way; the launcher and the tools change only through a new setup. `PASTEGRAB_RELEASES` overrides the base URL so a lab install can take a pre-release.

## Window

Native controls, system colors and the system font at 11 pt, 880 px wide by default. It has:

- a link box, with Ctrl+V anywhere and clipboard pickup when the window gets focus;
- a card with title, site, length, size, the profile list and «Скачать»;
- a virtual `LVS_OWNERDATA` job list, with the themed progress bar custom-drawn in the state column, whose width is `max(330, list/2)`;
- failures in muted red `0x002828A0`, except under High Contrast;
- a row menu with «Открыть», «Показать в папке», «Повторить» and «Отменить», and Delete to cancel the selected job;
- «Очистить список», which drops finished, failed and cancelled rows;
- a folder picker through `IFileOpenDialog`;
- taskbar progress through `ITaskbarList3`, and `SetThreadExecutionState` so the PC stays awake while jobs run;
- a tray balloon when a job finishes or fails for good while the window is in the background;
- a 180 ms ease-out fade on the card, off when `SPI_GETCLIENTAREAANIMATION` says Windows animations are off;
- one window per user; a second start passes its links over and exits.

## Build, test, release

On Linux:

- `just check` runs the rustfmt check, clippy and the tests. All 184 pass at `6de37e5`.
- `just clippy-windows` lints the x86_64 win7 build and the i686 setup inside the container.
- `tools/build/rust/build.sh <dir> <target> [cargo args]` builds in the container. `PASTEGRAB_PROFILE=size` selects the launcher's profile.
- `just icon` re-renders the icon from its SVG.

A release follows `docs/release.md`:

1. Bump `version` in `apps/window/Cargo.toml`. Installs only take a newer version.
2. `just package target/tools-stage` writes the setup, the update package `pastegrab-app-<version>-x86_64.tar.zst` and an unsigned `update.json` to `target/dist/<version>/`. `target/tools-stage` holds the unpacked `python`, `qjs` and `ffmpeg` folders. It isn't in git, and no script in the repository builds it yet.
3. `just sign <version>` is the owner's step, because minisign asks for the key password. The secret key lives at `~/.minisign/minisign.key`, rendered from the owner's password manager.
4. `just publish <version>` creates the GitHub release; `--prerelease` keeps it out of `latest`.
5. Every release file should pass Defender in both VMs through `lab/defender-scan` and VirusTotal through `tools/release/virustotal.sh`. The exe is unsigned, and SmartScreen stops its first start once.

## Lab

**Two VMs on `qemu:///system`, defined in `lab/`, stand in for his PC.** `pastegrab-w81` runs Windows 8.1 Pro x64 and `pastegrab-w80` runs Windows 8 x64, both in Russian with 2 vCPUs and 4 GiB each. Snapshot `clean` has no UCRT and `ucrt` mirrors his PC. Networking is passt in user mode. Both VMs are shut off.

`lab/guest-run VM 'powershell'` runs a script through the guest agent. `guest-put`, `guest-gui`, `click`, `screenshot` and `defender-scan` sit beside it. A window that Task Scheduler starts doesn't get the foreground, and `lab/click` activates it through the USB tablet. Drivers must stay at virtio-win 0.1.215, because later releases sign their Windows 8 catalogs with a development certificate that 8.x refuses.

## Status

**M0, the platform check, and M1, the core with pgcli on Linux, are done. M2, the Windows UI with setup and self-update, is mostly done and shipped as 0.2.0.**

Tested in the VMs:

- The 8.1 VM passed setup, shortcuts, uninstall, saved settings, the whole window flow, taskbar progress and keep-awake, the tray balloon, progress bars, red failures, the launcher's rollback and confirm rules, and a real update from 0.1.0 to 0.1.1 through a GitHub pre-release.
- The 8.0 VM, started from `clean`, showed setup's UAC prompt for the redistributable, installed the UCRT, wrote a correct `deploy.json` and report, and downloaded from YouTube, VK, Rutube and OK.ru. Private memory read 2.4 MB with jobs running and 9 MB idle, against budgets of 30 and 15 MB.
- pgcli on Linux downloaded from YouTube, VK, Rutube, OK.ru and archive.org, as `docs/corpus.md` lists. archive.org's `Cops1922` came in at about 210 KB/s over one connection on the 8.0 VM, which led to split downloads. On the owner's Linux machine the split version averaged about 1.6 MB/s. The two numbers come from different machines.

Never run on Windows:

- split downloads and the hand-off from yt-dlp;
- «Очистить список»;
- the app reading its preset and download count from `deploy.json`;
- the 0.2.0 build as a whole;
- setup's report on 8.1 with the UCRT already present, setup's page when a tool fails its self-test, and a declined UAC prompt.

Still missing from M2:

- a visible ✕ on running rows, which the owner was promised; right-click «Отменить» and Delete already cancel;
- the first-run tutorial, five skippable steps described in section 8 of the spec;
- per-monitor DPI on 8.1 through `SetProcessDpiAwareness`, loaded at runtime;
- the remaining fades on new rows, finished-job buttons and the tutorial panel;
- a measured launch time against the 150 ms warm budget;
- olympics.com from a home connection;
- notes in `docs/platform.md` for the 8.0 setup run and the split-download numbers.

After M2:

1. Telegram reports. «Отправить разработчику» shows the exact text, encrypts it in age's format to the owner's X25519 key and sends it through a dedicated bot's `sendDocument`. The token and chat ID come from the git-ignored `secrets.toml`, which `just secrets` in `local/local.just` renders from the password manager. Section 9 of the spec has the design.
2. M3: yt-dlp updates between releases, with EngineOutdated triggering one; failure messages; resume after restart through `queue.json`; Copy diagnostics and a log file; local file conversion; fragments with «с» and «по» fields, which `domain` and the yt-dlp arguments already support; the analysis profile end to end; playlists with item selection; browser cookie login.
3. M4: a `pastegrab:` URL protocol with a bookmarklet, drag from the browser, live recording, GIF export, an English string table.

The spec also describes features nobody has built: thumbnails decoded by WIC, a free-space check before a fetch, resume with `If-Range`, `portable.txt`, and the `h264_mf` encoder.

## Known limits

- In a split download the last range to finish runs alone at single-connection speed.
- yt-dlp needs Python 3.11 or newer. When it drops 3.11, likely by October 2027, Windows 8.0 has no newer Python to move to.
- `update.json` expires a year after packaging, and a PC that missed that year needs a re-signed manifest.
- The launcher and the tools update only through a new setup.
- YouTube bot checks seen in the lab come from its shared address and prove nothing until they repeat on a home connection.

## Lessons from the lab

- Windows 8 creates layered child windows only for programs whose manifest lists Windows 8 under `supportedOS`. Without that entry the fades fail.
- Python on a Russian Windows writes cp1251 into a pipe unless yt-dlp gets `--encoding utf-8`, and emoji in titles vanish.
- PowerShell and Notepad on Windows 8 write UTF-8 with a byte order mark. `serde_json` refuses it, so the settings loader strips it.
- yt-dlp's release file holds 1,046 modules and compiles them on every launch. Precompiled bytecode starts in 0.3 to 0.4 s warm but loses on a cold HDD, where each loose `.pyc` costs a seek.
- QuickJS has no version output. The self-test passes when `print(6*7)` prints 42 and records no version.
- Fields of packed i686 structs such as `SHELLEXECUTEINFOW` can't be borrowed; copy the value out first.

## How the owner wants the work done

- Read the Rust guidance and the SOLID principles before writing code, and keep the dependency rule above.
- Commits use `type(scope): lowercase subject`, signed, with no Co-Authored-By line or AI attribution. Pushing to `main` is allowed without asking. A history rewrite needs a backup branch first, and every intermediate commit must build.
- Run `just check` and `just clippy-windows` before each commit.
- The owner signs and publishes releases. Don't publish or touch the password manager without them. Never `rbw edit` an item that a Bitwarden app created, because rbw 1.15 drops the item's own key and leaves its fields undecryptable. New secrets go in with `rbw add`.
- Keep the owner's network and VPN details, the work VM, the password manager and links to ISO downloads out of the public repository. Secrets never go into git.
- Never attach a lab VM to libvirt's `default` network (virbr0). It is firewalled for another VM and gives a lab VM no internet.
- Don't run sudo.
- Don't work around a site's bot protection while testing.
- Comments are short, rare and in the present tense, with a single ` - ` for an aside. Measurements go in `docs/`, never in comments.
- Docs, commit messages and chat replies use plain, direct prose with no em dashes and no filler. A doc heading is a short noun phrase, and a measurement in `docs/` keeps its numbers and its machine.
- UI text uses plain Russian words he would use himself, with no slogans and no exclamation marks. The owner rejected «Видео, играет везде» as AI-sounding.
- The look stays native, with system colors and nothing that glows. Besides progress, the only motion is a 150 to 200 ms fade that Windows' animation switch turns off.
- The owner's usage quota is tight. No subagents, few screenshots, and report findings early.

## Further reading

| File | What |
|---|---|
| `SPEC.md` | design, decisions, milestones, risks, open questions |
| `docs/platform.md` | Windows measurements, feature by feature |
| `docs/release.md` | signing, publishing, antivirus, SmartScreen |
| `docs/corpus.md` | results per site |
| `lab/README.md` | rebuilding and driving the VMs |
