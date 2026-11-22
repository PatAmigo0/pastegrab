# pastegrab

**A desktop app for Windows 8.x that takes any pasted text with a link in it and saves the video, or only its audio, as a file that machine can play.** One non-technical person uses it, mostly for old Olympic and other sports footage that he studies in ProSportsAnalyzer. You maintain it from a distance.

`pgcli`, M1's command line, downloads and converts on Linux and on the 8.1 VM. `docs/platform.md` holds the Windows measurements and `docs/corpus.md` the results per site. Section 1 lists the choices and section 15 the open questions. The test VMs live in `lab/`, described in section 11.

## 1. Decisions

| Area | Choice | Reason |
|---|---|---|
| Site support | yt-dlp as a child process | 1,755 extractors in 2026.10.26 and frequent releases that track YouTube's changes. Rust extractors would mean owning that churn forever. |
| yt-dlp's runtime | python-build-standalone 3.11, pruned and shipped with pastegrab, under yt-dlp's 3 MB pure-Python release | yt-dlp's own Windows builds need Windows 10 since 2026.10.26. Windows 8.0 runs Python up to 3.11, and current yt-dlp needs 3.11 or newer. |
| Direct media links | Downloaded in-process | A plain `.mp4` URL needs no Python start. |
| Merging and conversion | ffmpeg as a child process: gyan.dev's 8.1.1 essentials build | The exact build ProSportsAnalyzer 1.8 already runs on his PC. gyan lists Windows 10 as its floor, which the lab checks on 8.0 and 8.1. |
| YouTube's JavaScript challenge | Bellard's QuickJS 2026-06-04, built with mingw-w64 | Since 2025.11.12 yt-dlp needs a JS runtime for YouTube. Deno and Node embed V8, and current Node refuses to start below Windows 10. In the lab QuickJS solved the challenge in 4.3 s, QuickJS-NG in 5.2 s. |
| UI | Native Win32 through the `windows` crate | No GPU or WebView dependency, native look and screen reader support, a few MB of memory. WebView2 stopped at version 109 on 8.1, which rules out Tauri. |
| UI language | Russian only, every string in one table per language | A language added later is one new file, and the compiler rejects it while a string is missing. |
| Installation | `pastegrab-setup.exe`, a 32-bit Rust bootstrapper | It runs on any Windows 8.x, measures the machine nobody can ask about, installs the 64-bit build, tunes settings to the hardware and handles prerequisites. |
| Concurrency | `std::thread` and channels, no async runtime | A handful of child processes and sockets at most. Blocking reads on pipes fit that directly. |
| Toolchain | Pinned nightly, Tier 3 `x86_64-win7-windows-gnu`, `-Zbuild-std`, mingw-w64 in podman | Tier 1 Windows targets need Windows 10 since Rust 1.78. The win7 targets keep std's fallbacks for 7, 8 and 8.1. The GNU target links the `msvcrt.dll` every Windows 8 has and needs no Microsoft SDK. |
| Windows 8 guarantee | CI checks every PE import against the exports both lab VMs share | A Windows 10 import becomes a failed build instead of a crash on the person's PC. 8.0 is the floor, so a function only 8.1 has fails the build too. |
| TLS in our code | rustls with `ring` and bundled Mozilla roots | SChannel on 8.1 has no TLS 1.3, and the root store of an unpatched PC goes stale. |
| Serialization | `serde_json` for yt-dlp output, settings and the queue | One parser in the binary. |
| Reports | A Telegram bot, token compiled into the exe, report bodies encrypted to your key | No server to run. A leaked token costs a revoke and a release, and whoever holds it sees only ciphertext in the chat history. |
| Structure | Ports and adapters across a Cargo workspace | Crate boundaries turn the dependency rule into compile errors, and the core builds and tests on Linux. |

## 2. Scope

Goals:

1. Paste or drop, press Enter, get a file.
2. Every site yt-dlp supports, plus direct media files and HLS or DASH playlists.
3. MP4 with H.264 and AAC for video and M4A for audio by default, since Windows Media Player on 8.1 plays both without extra codecs.
4. Conversion of downloaded or local files through a few presets.
5. A lean process with a native UI, zero activity when idle and no re-encode the planner can avoid.
6. Self-maintenance. yt-dlp stays current, failures get plain-language explanations, and a diagnostics blob lets you debug remotely.
7. Fragments of long sports broadcasts, and an analysis profile that ProSportsAnalyzer opens directly.

Out of scope:

- DRM-protected services such as Netflix or Spotify. yt-dlp refuses them, and pastegrab will not work around that.
- Extractors written in Rust.
- A player, an editor or a browser extension. A bookmarklet plus a URL protocol covers the browser in M4.
- A macOS or Linux GUI. A Linux CLI build exists for development and doubles as your own downloader.

## 3. Platform constraints

**Windows 8.x on 2012-2015 hardware decides most of this spec.**

ProSportsAnalyzer tells us a little about his PC. Its 64-bit 1.8 build runs there, which means a 64-bit Windows. Its `python38.dll` imports the UCRT while the bundle ships no UCRT DLLs, which means the UCRT update is installed. It also bundles gyan.dev's ffmpeg 8.1.1 essentials build. Whether it is Windows 8.0 or 8.1, and which CPU, RAM and disk it has, stays unknown, and nobody can ask him. pastegrab measures the machine itself at install time, and the lab runs both 8.0 and 8.1.

| Fact | Consequence |
|---|---|
| Windows 8.0 lost support in January 2016 | Nobody can upgrade his PC, so the lab tests 8.0 as well as 8.1 |
| Rust's tier 1 Windows targets need Windows 10 | Tier 3 `*-win7-windows-*` targets, built with `-Zbuild-std` because no prebuilt std exists for them |
| WebView2 Runtime 109 was the last one for 8.1 | No Tauri, no HTML UI |
| Node removed 8.1 from its support table and checks for Windows 10 at startup | No Node or Deno for yt-dlp; QuickJS instead |
| BtbN's FFmpeg builds start at Windows 10 22H2, gyan.dev's at Windows 10 | gyan's 8.1.1 essentials, proven on his PC and on both lab VMs |
| yt-dlp's Windows builds need Windows 10 from 2026.10.26, when they moved to Python 3.14; its maintainers report Python 3.11 as the last that Windows 8 runs | python-build-standalone 3.11 under yt-dlp's pure-Python release, which M0 ran on 8.0 and 8.1 |
| SChannel lacks TLS 1.3, and root certificates stop updating | rustls with bundled roots in our code; yt-dlp carries its own OpenSSL and certifi |
| No `LongPathsEnabled` before Windows 10 | Output paths stay under 260 characters |
| Browsers end at Chrome 109 and Firefox 115 ESR | Cookie import for logged-in sites targets those two |
| x86-64-v2 needs SSE4.2 and POPCNT, which Core 2 and Phenom II lack | Baseline x86-64 codegen, no `target-cpu` |

## 4. Architecture

### Workspace

```
pastegrab/
  crates/
    domain/   link pipeline, site rules, format planner, job state machine; no I/O
    app/      ports, use cases, scheduler, core loop, view models
    ytdlp/    Resolver and Fetcher over yt-dlp, its fixed flags, its error table
    ffmpeg/   Transcoder and MediaProbe over ffmpeg and ffprobe
    http/     HttpClient over ureq, direct-file Fetcher, tool downloads
    os/       ProcessRunner, SystemInfo, sleep prevention, known folders, atomic writes, file logger
    store/    settings and queue as JSON
    win32/    View over Win32
    cli/      View over a terminal
    i18n/     one string table per language; no dependencies
    deploy/   machine profile, build choice, verified downloads, tool installs; used by setup and the updater
    testkit/  fakes and contract suites for every port
  apps/
    pastegrab/   Windows GUI composition root
    pgcli/       CLI composition root
    setup/       pastegrab-setup.exe
  lab/           test VMs: libvirt XML, unattend files, the M0 harness, the API probe
  tools/build/   podman images for the Windows toolchain and QuickJS
  xtask/         import check, sidecar packaging
```

**Dependencies point inward. Adapters depend on `app`, `app` depends on `domain`, and only `apps/*` see adapters.** No adapter depends on another one; `ytdlp` and `ffmpeg` start processes through the `ProcessRunner` port and never import `os`. Three mechanical checks hold the line:

- `cargo-deny` bans `windows`, `ureq` and `serde_json` except as direct dependencies of the crates that wrap them, named in its `wrappers` list.
- `clippy.toml` in `domain` and `app` disallows `std::fs`, `std::process` and `std::net`.
- `#![forbid(unsafe_code)]` everywhere except `os` and `win32`. In those two, `unsafe` lives in a `sys` module and every block carries a `// SAFETY:` comment.

### SOLID here

| Principle | Rule in this codebase | Checked by |
|---|---|---|
| Single responsibility | A module owns one decision. `ytdlp` turns yt-dlp's text into a `FailureKind`; `app` decides what to do about that kind. | Crate split, review |
| Open/closed | Site hints, output profiles and error patterns are rows in static tables. Engines are `Resolver` and `Fetcher` implementations registered in `apps/*`. Adding either never edits a `match` in `app`. | Review |
| Liskov substitution | Every port has a contract suite in `testkit`, and every implementation, fakes included, runs it. `accepts` is an offline check, and the first resolver or fetcher that accepts a link or source gets it. | `cargo test` |
| Interface segregation | A port has at most three methods. The UI receives `ViewUpdate`s and never sees domain types. | Review |
| Dependency inversion | `domain` and `app` define the traits and never touch Windows, files, sockets or processes. | `cargo-deny`, `clippy.toml` |

**A trait exists only when it has two implementations, the production adapter plus a test double or an alternative.** The link pipeline is pure code with nothing to invert. Its variation lives in data tables, and it gets no traits.

**Ports are `dyn` traits, wired once in `main`.** A port call happens a few times per job, next to a process launch that costs milliseconds, and the vtable lookup disappears in that noise. One copy of the core instead of a monomorphized copy per adapter combination keeps the exe small. Hot loops, meaning line parsing, URL matching and planner scoring, are plain functions.

### Ports

| Port | Methods | Adapters | Second implementation |
|---|---|---|---|
| `Resolver` | `accepts`, `resolve` | yt-dlp, direct HTTP | fixture replay |
| `Fetcher` | `accepts`, `fetch` | yt-dlp, direct HTTP | fixture replay |
| `Transcoder` | `caps`, `transcode` | ffmpeg | recorder |
| `MediaProbe` | `probe` | ffprobe | fixture replay |
| `ProcessRunner` | `spawn` | Job Objects on Windows, process groups on Linux | scripted transcripts |
| `HttpClient` | `get` with an optional range | ureq | in-memory server |
| `ToolStore` | `current`, `install`, `prune` | versioned folders | in-memory |
| `JobStore`, `SettingsStore` | `load`, `save` | JSON with atomic replace | in-memory |
| `ProgressSink` | `report` | atomics plus a UI wake-up | recorder |
| `View` | `apply` | Win32, CLI | recorder |
| `SystemInfo` | `machine`, `free_space` | Win32, Linux | fixed values |
| `Clock` | `now` | system | manual |

```rust
pub trait Resolver: Send + Sync {
    /// Offline check, no I/O.
    fn accepts(&self, link: &Link) -> bool;
    fn resolve(&self, link: &Link, cancel: &Cancel) -> Result<Resolved, Failure>;
}

pub trait Fetcher: Send + Sync {
    fn accepts(&self, plan: &FetchPlan) -> bool;
    fn fetch(&self, plan: &FetchPlan, progress: &dyn ProgressSink, cancel: &Cancel)
        -> Result<Fetched, Failure>;
}

pub trait ProcessRunner: Send + Sync {
    /// Starts the child inside its own kill-on-close job and registers the kill on `cancel`.
    fn spawn(&self, spec: &ProcSpec, cancel: &Cancel) -> Result<Box<dyn Lines>, Failure>;
}

pub trait Lines: Send {
    /// Reads one line into `buf`, reusing its allocation. `Ok(false)` at EOF.
    fn next_line(&mut self, buf: &mut Vec<u8>) -> std::io::Result<bool>;
    fn wait(self: Box<Self>) -> std::io::Result<ExitCode>;
}
```

`Cancel` holds a flag and the kill hooks that the running step registered. Calling `cancel()` sets the flag and runs the hooks, after which a blocked reader hits EOF and the worker returns `Cancelled`. The HTTP fetcher checks the flag between chunks and relies on socket timeouts for the rest.

`Failure` is a `FailureKind` plus a detail string for logs. Each kind maps to one retry policy and one message, both listed in section 7.

### Threads

```
UI thread ──Command──▶ core thread ──spawn──▶ worker threads
(message loop)         (owns all state)        (one per running step,
    ▲                        │       ▲           each owns one child)
    │ ViewUpdate queue       │       │ WorkerEvent, bounded channel
    └────────────────────────┘       └──────────────────────────────
    ▲
    └── PostMessage(WM_APP) from workers when progress changes, coalesced
```

- The core thread is the only writer of application state, which covers jobs, settings and the scheduler. Commands from the UI and events from workers arrive on one bounded channel, and no lock guards the state.
- A worker runs one step of one job, such as a probe, a fetch or a conversion, and owns that step's child process. A thread per step costs microseconds next to a process launch and leaves nothing parked when the queue is empty.
- Progress skips the core. A worker writes the latest numbers into the job's `ProgressCell`, a set of atomics, and posts a `WM_APP` wake-up only when none is pending. The UI repaints changed rows at most every 250 ms. With no progress, the UI thread sleeps.
- A panic inside a worker is caught at the thread boundary and fails that one job. A panic on the core thread writes a crash report and exits, and the job objects take the children down with it.

### Domain model

- `Link` holds a cleaned URL, site, kind, id and hints such as a playlist or a start time. Only the link pipeline can build its `CleanUrl`.
- `MediaInfo` holds the title, the duration, a live flag and every `Format`. Thumbnail and playlist entries join with the yt-dlp adapter.
- `Format` holds yt-dlp's id, an optional video stream with codec, height and fps, an optional audio stream with codec and bitrate, a size or a bitrate for estimates, the protocol and a DRM flag.
- `FetchPlan` holds the exact format ids, the merge container, the section, the expected size and the source it came from: a yt-dlp probe file or a direct URL. `Fetcher::accepts` decides by that source. `Convert` holds a copy, encode or drop decision per stream and the exact cut.
- A stream the source lists without a codec, as HLS variants without a CODECS attribute do, has the codec `Unknown`. It still plans, and ffprobe names the codec after the fetch.
- `JobState` runs `Queued → Probing → Ready → Fetching → Converting → Done`, plus `Paused`, `Cancelled` and `Failed(FailureKind)`. Transitions are one pure function with an exhaustive table test.
- The scheduler is pure too. Given the state and the free permits, it returns the steps to start; the permits allow two probes, two fetches and one conversion at once. Threads only carry out its decisions, which keeps scheduling tests free of threads.

## 5. Link understanding

**Pasted text becomes a `Link` through five stages in `domain`, all offline.**

1. Find. `linkify` scans the text with the scheme optional, trims trailing punctuation and returns every candidate. Three links on three lines make three jobs.
2. Parse. The `url` crate applies WHATWG rules. Only `http` and `https` survive, a bare domain gets `https://`, and IDN hosts become punycode.
3. Unwrap. Redirect wrappers lose their shell, up to three levels deep: `google.com/url?q=`, `l.facebook.com/l.php?u=`, `l.instagram.com/?u=`, `vk.com/away.php?to=`, `youtube.com/redirect?q=`, `out.reddit.com`. Shorteners that need a request, like `t.co`, `bit.ly` and `vk.cc`, go to yt-dlp, which follows redirects.
4. Clean. A known site keeps only its allowlisted parameters; YouTube keeps `v`, `list`, `index` and `t`. An unknown site loses only known trackers: `utm_*`, `fbclid`, `gclid`, `yclid`, `igshid`, `mc_eid`. An allowlist would break unknown sites, and a denylist alone misses next year's tracker.
5. Classify. A static table maps host and path to site, kind, id and default intent. Mirror and mobile hosts fold into the canonical one. `youtu.be` and `m.youtube.com` become `www.youtube.com`, while `vxtwitter.com`, `fxtwitter.com` and `mobile.twitter.com` become `x.com`.

```rust
pub struct SiteRule {
    pub site: Site,
    pub hosts: &'static [&'static str],       // matched on label boundaries
    pub routes: &'static [Route],             // "/watch?v={id}", "/shorts/{id}"
    pub keep_params: &'static [&'static str],
    pub intent: Intent,                       // Video or Audio
}
```

The table starts where his footage lives: YouTube, olympics.com through yt-dlp's `OlympicsReplay`, archive.org, VK, OK.ru, Rutube, Dailymotion and smotrim. Common sites follow: Dzen, TikTok, Instagram, Telegram public posts, X, Reddit, Twitch, Vimeo. It drives defaults and hints only. yt-dlp decides whether a link works, and an unknown host goes straight to the probe.

Examples, with made-up IDs:

| Pasted | Result |
|---|---|
| `глянь https://youtu.be/AbC123xyz_0?si=Q1w2 !!` | YouTube video `AbC123xyz_0`, `si` dropped |
| `https://olympics.com/ru/video/slavomir-knazovicky-canoe-sprint-c1-500m-silver-medal-atlanta-1996?uxreference=playlist` | olympics.com video through `OlympicsReplay`, `uxreference` dropped |
| `youtube.com/shorts/AbC123xyz_0` | YouTube video, `https://` added |
| `https://www.youtube.com/watch?v=AbC123xyz_0&list=PLq9z&index=4` | Item 4 of a playlist. Offers this video, the whole playlist or everything from here on, with this video preselected |
| `https://www.youtube.com/watch?v=AbC123xyz_0&list=RDAbC123xyz_0` | Single video. `RD` lists are endless Mixes and never get offered as playlists |
| `https://www.youtube.com/watch?v=AbC123xyz_0&t=95` | Video, with "start at 1:35" offered in M4 |
| `https://music.youtube.com/watch?v=AbC123xyz_0` | Audio by default |
| `https://l.facebook.com/l.php?u=https%3A%2F%2Fvimeo.com%2F123456&h=AT0` | Vimeo `123456` |
| `https://vxtwitter.com/someone/status/1790000000000000000?s=20` | `https://x.com/someone/status/1790000000000000000` |
| `https://example.org/media/clip.mp4` | Direct file, no yt-dlp |
| `https://cdn.example.org/live/master.m3u8` | HLS playlist, handled by yt-dlp |
| `https://example.org/news/some-article` | Unknown host. The probe runs, and yt-dlp's generic extractor looks for embedded players |

Around the pipeline:

- Duplicates match on `(site, id)` for known sites and on the cleaned URL otherwise. Pasting a finished link offers Open and Download again.
- Direct-file test. For an unknown host, a one-byte ranged GET runs before any yt-dlp launch, and `Content-Type: video/*` or `audio/*` marks a direct file. A hit saves a Python start; a miss costs one round trip.
- Probe on paste. The probe starts the moment a link is recognized, and the fetch reuses its output as section 7 describes. Title, size and plan are usually ready before the person clicks Download. Changing the profile re-runs the planner on the same probe result.
- The clipboard is read once when the window gets focus, and a recognized link not seen before fills the input. No background clipboard listener runs.
- Links dragged from the browser arrive through OLE `IDropTarget`. Dropped files go to conversion.
- In M4 a `pastegrab:` URL protocol and a bookmarklet send the current page from the browser. A protocol launch only fills the input, because any web page can fire a protocol URL.

## 6. Format planning

**The planner picks source streams so that the output needs no re-encode whenever the site offers a matching codec.** It runs in two pure steps. `plan_fetch` picks exact format ids from yt-dlp's list before the download. `plan_convert` decides copy, encode or drop per stream afterwards, from what ffprobe finds in the real file, because yt-dlp's list never says whether a video is interlaced. Local files go through the second step only.

| Label | Container | Video | Audio |
|---|---|---|---|
| «Видео» | MP4 | H.264 up to the height cap | AAC |
| «Видео, лучшее качество» | MKV | best available, VP9 or AV1 included | best available |
| «Видео, малый размер» | MP4 | H.264, 480p at most | AAC |
| «Только звук» | M4A | none | AAC, copied when offered |
| «Только звук, MP3» | MP3 | none | MP3 V2 |
| «Для анализа» | MP4 | H.264, highest frame rate first, then height up to 1080p | AAC |

«Видео» is the default, and music sites default to «Только звук». Files from «Видео, лучшее качество» need VLC or MPC-HC on Windows 8, and the detail line under the label says so.

Selection:

1. Drop DRM formats, storyboards and protocols nothing here can fetch.
2. For video, the tallest format at or under the cap with the profile's codec wins. If no format has that codec, the tallest one that fits wins and a transcode gets planned.
3. «Для анализа» ranks frame rate above everything else. 60 fps VP9 beats 30 fps H.264, because conversion keeps the frame rate and nothing brings lost motion back.
4. Audio follows the codec rule with the profile's audio codec, then the bitrate.
5. Ties go to the codec that is cheaper to decode, VP9 before AV1 on an old CPU, then to the smaller file.
6. The plan names exact format ids such as `137+140`. A selector string would let yt-dlp choose again at fetch time, and the plan would stop being testable. Ids pass through a `FormatId` type that allows letters, digits, `.`, `_`, `-` and `=`, and refuses selector words such as `all` or `best`, which a site could otherwise use as a format's name.
7. When the best video has no sound and no audio-only format exists, a format that carries both wins over it. A source with no sound anywhere stays a silent video.
8. A video without a height, as OK.ru lists them, ranks by its size, and the bigger file wins.

The "Auto" height cap is the largest of 360, 480, 720, 1080, 1440 and 2160 that fits the screen. A 1366×768 laptop gets 720p.

Conversion:

- Remux with `-c copy` whenever the codecs fit the target container.
- No `-movflags +faststart`. It helps web servers stream and costs a second full write of the file.
- H.264 through `libx264 -preset veryfast -crf 23`. `h264_mf`, Windows' Media Foundation encoder, which reaches the GPU's encoder where the driver provides one, after a one-frame test passes at first run.
- AAC audio is copied when possible and otherwise encoded with ffmpeg's `aac` at 160k. MP3 goes through `libmp3lame -q:a 2`.
- ffmpeg runs at `BELOW_NORMAL_PRIORITY_CLASS` and reports through `-progress pipe:1 -stats_period 0.5`. One conversion runs at a time.
- Local files take the same path. Dropping a file or pressing "Convert a file" runs ffprobe, then the planner.

Old broadcast footage adds two rules. Interlaced sources get `bwdif=mode=send_field` whenever they are transcoded, and always under "For analysis": 25 interlaced frames a second become 50 progressive ones, which doubles the motion samples a running-technique study works with. Whenever it encodes, «Для анализа» also keeps a keyframe at least every second, so scrubbing in ProSportsAnalyzer lands on exact frames. An encode with no codec dictated by the profile goes to H.264, never to VP9 or AV1, whose encoders crawl on an old CPU.

Olympic replays run for hours while a race lasts seconds. The preview card has «с» and «по» fields that become `--download-sections "*1:23:10-1:23:40"`, and most of the replay never crosses the network. yt-dlp cuts on keyframes without encoding, then the convert step makes the exact cut while it encodes. The clip gets one encode, not two.

A section makes yt-dlp download through ffmpeg, which can't impersonate a browser. When a fragment from a site that needs impersonation fails with 403, the job downloads the whole stream through yt-dlp's own downloader and the convert step cuts it.

## 7. Engines

### yt-dlp

**Each item costs two yt-dlp runs and one site extraction, because the probe saves the full info JSON and the fetch loads it back.**

Probe:

```
yt-dlp <fixed flags> --no-playlist --no-simulate --skip-download --write-info-json
       -P "<job>" -o probe --print-to-file "<summary template>" summary.json
       -- "<url>"
```

For YouTube the full info JSON is large, since it lists every format URL and caption track. It stays on disk. pastegrab reads only `summary.json`, a few kilobytes built with a `%(...)j` template over the fields the planner needs; `j` writes JSON with non-ASCII characters escaped, which no code page can damage. `--output-na-placeholder null` covers a field the site leaves out, which yt-dlp otherwise prints as a bare `NA` even inside `j`. Until playlists arrive in M3, a page that holds several videos, such as an archive.org collection, fails as unsupported. Playlists add `--flat-playlist` and get one summary line per entry. Entries are probed one at a time as their turn comes, and a 300-item playlist never starts 300 probes. Each entry URL the site supplies goes through the link pipeline first, so an entry can't bring in a scheme other than http(s).

Fetch:

```
yt-dlp <fixed flags> --load-info-json "<job>\probe.info.json" -f 137+140
       --merge-output-format mp4 -P "<dest>\.pastegrab-<job id>"
       -o "%(id)s.%(ext)s" --print-to-file after_move:filepath result.txt
```

Files yt-dlp writes are named relative to `-P`. yt-dlp reads those names as output templates, and a `%` in an absolute path would turn into a field.

yt-dlp never picks the final name. pastegrab moves its file to a name from `output_name` in `domain`: the title without characters Windows forbids, never a reserved name such as `CON` or `NUL`, a length counted in UTF-16 units so the whole path stays under 260, the extension from the profile's container, and a « (2)» suffix instead of replacing a file that exists. A local file converted to its own format gets the suffix too, so the source survives. When the fetch ends, the probe's info JSON is deleted, because with browser login it carries cookies in every format's headers.

Fixed flags, every one present in `yt-dlp --help` of 2026.10.26:

```
--ignore-config --no-plugin-dirs --no-remote-components
--no-js-runtimes --js-runtimes "quickjs:<tools>\qjs\<version>\qjs.exe"
--ffmpeg-location "<tools>\ffmpeg\<version>" --cache-dir "<data>\cache\yt-dlp"
--quiet --progress --newline --progress-delta 0.5 --color never
--progress-template "download:@pg|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"
--progress-template "postprocess:@pp|%(progress.postprocessor)s|%(progress.status)s"
--encoding utf-8 --windows-filenames
--downloader-args "ffmpeg_i:-tls_verify 1 -ca_file <tools>\python\<version>\site-packages\certifi\cacert.pem"
-N 4 --retries 10 --fragment-retries 10
```

Adapter rules:

- yt-dlp runs as `python.exe -I <tools>\yt-dlp\<version>`: python-build-standalone 3.11 with yt-dlp's release file unpacked and compiled to bytecode once, at install or update. On the 8.0 VM that starts in 0.6 s. Run as shipped, the release file compiles its 1,046 modules on every launch and takes 2.1 to 2.8 s. `-I` keeps `PYTHON*` variables and the user's site folder out.
- The working directory is the job folder, and `TEMP` is the short 8.3 form of `<data>\tmp`. QuickJS opens its script through the ANSI code page, and a user name outside cp1251 would otherwise break YouTube's challenge.
- `--` comes before every URL. Input starting with `-` can never turn into an option; the parser rejects non-http input first, and this is the second lock.
- Nothing parsed depends on the console code page. Without `--encoding utf-8`, Python writes to a pipe in the ANSI code page, cp1251 on a Russian Windows, and in M0 an emoji in a title came out as nothing. Progress lines are ASCII by construction, the summary and the final path come from files yt-dlp writes in UTF-8, and error text is decoded lossily for display only.
- stdout and stderr share one `std::io::pipe`. Each child gets one reader, and an unread stderr can never deadlock it.
- `CREATE_NO_WINDOW` on every spawn, or each run flashes a console window.
- A test diffs the fixed flags against the bundled version's `--help`. A renamed flag fails CI instead of a download.

Failure kinds. The patterns sit in one table in `ytdlp`, tested against recorded output and extended from every new failure. The policy lives in `app` and sees only the kind.

| Kind | Recognized by | Automatic action | Shown to the person |
|---|---|---|---|
| `EngineOutdated` | `Unable to extract`, `Signature extraction failed`, solver errors recorded in M0 | Update yt-dlp, retry once | The site changed; updating and retrying |
| `BotWall` | `Cloudflare anti-bot challenge`, HTTP 403 from a site known to sit behind Akamai or Cloudflare | Retry once with `--impersonate chrome`, or `--extractor-args generic:impersonate` for the generic extractor | Nothing |
| `BotCheck` | `not a bot`, HTTP 429 | Pause that site's jobs | Suggests browser login |
| `NeedsLogin` | `Private video`, `Sign in to confirm your age` | None | Needs an account, and where to turn on browser login |
| `Unavailable` | `Video unavailable`, `available in your country` | None | Removed, or blocked in this country |
| `NotStarted` | `This live event will begin` | None | The stream has not started |
| `Unsupported` | `Unsupported URL` | None | No video on this page; copy the video's own link |
| `Expired` | HTTP 403 during a fetch | Re-probe once | Nothing |
| `Network` | Timeouts, resets, DNS errors | Three retries with backoff | No connection, retrying |
| `DiskFull` | `No space left on device` | Pause the queue | The disk is full |
| `ToolBroken` | Exit code `0xC0000135` or `0xC0000139`, failed spawn | Reinstall that tool once | Repairing components |
| `Internal` | Anything else, panics | None | Copy diagnostics and send them to you |

`0xC0000135` and `0xC0000139` are Windows' "DLL not found" and "entry point not found", the two codes a tool update with a Windows 10 dependency produces on 8.1.

**olympics.com, his main source, sits behind Akamai's bot filter, and from the lab it refused yt-dlp with 403 even with `--impersonate chrome`.** The lab's traffic doesn't come from a home connection, so the test doesn't say how the site treats one; section 15 has the check. `curl_cffi` itself works on 8.0 and 8.1, so impersonation is available wherever a site accepts it.

### Direct HTTP

- ureq with rustls on `ring` and `webpki-roots`, one 256 KiB buffer per download.
- Writes `<name>.part` in the destination folder and renames it when done.
- Checks the first bytes against media signatures (ISO BMFF `ftyp`, Matroska and WebM, MPEG-TS sync bytes, MPEG-PS, MP3 with or without ID3, FLAC, Ogg, AVI and ASF) and stops on anything else, whatever `Content-Type` claims. A server can't hand him an `.exe` named like a video.
- Resumes with `Range` and `If-Range` when the server sent `ETag` or `Last-Modified`.
- Reserves the expected size up front through `FileAllocationInfo` when the server reports it, which keeps the file in fewer fragments on an HDD.
- Socket read timeouts; the cancel flag is checked between chunks.

### ffmpeg

- `ffmpeg.exe` and `ffprobe.exe` come from gyan.dev's essentials build, pinned at 8.1.1, as section 10 describes.
- Encoder capabilities are probed once per ffmpeg version and cached in settings.
- gyan's 8.1.1 skips certificate checks unless told otherwise; its `tls_verify` defaults to false. Wherever yt-dlp hands ffmpeg a URL, the fixed `--downloader-args` turn verification on with certifi's bundle from the Python package, which also sidesteps Windows 8's stale root store.
- Arguments come from a pure function of the planner's `Convert`, covered by golden tests.

### Process control

- Spawn suspended with `CREATE_NO_WINDOW`, assign to a new Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resume. Every process yt-dlp starts later, ffmpeg included, inherits the job.
- Cancel calls `TerminateJobObject`. When pastegrab dies, its job handles close and Windows kills every child.
- Windows 8 added nested jobs, and this works even when a launcher already put pastegrab inside a job.
- `SetErrorMode(SEM_FAILCRITICALERRORS)` before the first spawn. Children inherit it, and a missing DLL returns an exit code instead of a system dialog in front of the person.
- `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)` first thing in `main`, and every sidecar starts by absolute path. A DLL planted next to a download can't load into pastegrab or setup. The probe in `lab/probe` checks suspended start, job assignment and kill-on-close on both VMs.
- The Linux CLI uses a process group per child and kills the group.

## 8. Interface

```
┌ pastegrab ────────────────────────────────────────────────────────┐
│ [ Вставьте ссылку (Ctrl+V)                                 ] [OK] │
│ ┌───────┐ Название видео                                          │
│ │ превью│ youtube.com · 2:41:04 · около 1,8 ГБ                    │
│ └───────┘ [Для анализа ▾] [Авто: 720p ▾]               [Скачать]  │
│           Фрагмент: с [1:23:10] по [1:23:40]                      │
│───────────────────────────────────────────────────────────────────│
│ ▸ Первое видео            ███████░░░  71%  2,1 МБ/с  0:40     ✕   │
│ ✓ Второе видео            Готово                Открыть  Папка    │
│ ! Третье видео            Нужен вход в аккаунт        Почему?     │
│───────────────────────────────────────────────────────────────────│
│ Сохранять в: C:\Users\...\Videos  [Изменить] [Конвертировать файл] │
└───────────────────────────────────────────────────────────────────┘
```

The UI is Russian. The name pastegrab, format and codec names such as MP4, H.264 and MP3, site names and key names keep their usual Latin spelling.

### Visual design

**Plain, calm and readable, built from Windows' own controls and colors. Nothing glows, and short fades are the only motion besides the progress bar.** He is an older man, not a power user, and the window should look like a tool that has always been on his PC.

- Native Win32 controls in the system's visual style, which on Windows 8 is already flat. No custom skin, gradient, shadow, glass, rounded cards, neon, emoji or illustration.
- Short fades keep the window alive. The preview card, a new row, the tutorial panel and a finished job's buttons fade in or out over 150 to 200 ms. Nothing loops or bounces, a fade never delays input, and the system switch «Показывать анимацию в Windows», read through `SPI_GETCLIENTAREAANIMATION`, turns them off. Windows 8 supports layered child windows, so a fade is one alpha value on a timer that stops when the fade ends.
- Colors come from the system through `GetSysColor`, so his theme and High Contrast work. The single exception is a muted dark red for failure text, and High Contrast replaces it too.
- Text uses the system message font, Segoe UI on Windows 8, at 11 pt instead of the default 9 pt, because small gray text is what an older reader loses first. Headings are semibold, never colored. Secondary text keeps a contrast of at least 4.5:1.
- One column, a 16 px margin and 8 px gaps scaled with DPI, buttons at least 32 px tall. Every state has one main action, such as «Скачать» or «Открыть»; everything else is an ordinary button beside it.
- Text uses the words he would use himself: plain nouns and verbs, no slogans, no clever phrasing. Buttons say «Скачать», «Повторить», «Открыть папку». Profiles are «Видео», «Видео, лучшее качество», «Видео, малый размер», «Только звук», «Только звук, MP3» and «Для анализа», with MP4 or 720p only in the detail line under them. No exclamation marks, no jokes, no «Упс».
- A failure is one line next to its job: what happened and what to do. Modal dialogs appear only when he has to decide something.
- Tooltips may repeat information, never carry it alone.
- The app icon is one flat glyph, an arrow into a tray, in a single muted color.

The tutorial panel and the installer follow the same rules.

- Ctrl+V anywhere pastes and analyzes, Enter downloads, Delete cancels the selected job, and Ctrl+O opens its folder.
- The job list is a virtual `LVS_OWNERDATA` ListView with progress drawn in custom draw. Memory stays flat however long the history gets, and no row owns a control.
- The taskbar button shows total progress through `ITaskbarList3`. A tray balloon announces finished jobs while the window sits in the background.
- `SetThreadExecutionState` keeps the PC awake while jobs run and releases it once the queue is empty.
- The folder picker is `IFileOpenDialog` with `FOS_PICKFOLDERS`. Video defaults to Videos and audio to Music, both resolved through `SHGetKnownFolderPath`.
- Single instance through a named mutex. A second launch forwards its argument with `WM_COPYDATA` and exits.
- DPI awareness is per-monitor on 8.1 through `SetProcessDpiAwareness`, loaded at runtime, and system-wide on 8.0.
- The manifest requests common controls v6, lists `supportedOS` entries for 8, 8.1 and 10, and runs `asInvoker`.
- Text lives in `i18n` as one `const Strings` per language, a struct with a field per message. A language added later that misses a field fails to compile. Messages with numbers are functions, which keeps the Russian plural forms файл, файла and файлов in one place. The presenter sends message keys and values, and only the view turns them into text.
- The preview card shows one thumbnail, a small JPEG picked from the probe's list and decoded by WIC. Windows 8 has no WebP codec.
- Settings cover save folders, the default profile, the height cap, parallel downloads from 1 to 4 with 2 as the default, a speed limit, language, browser login, clipboard pickup, the update channel and Copy diagnostics.

### First-run tutorial

**On first start, a panel at the bottom of the window walks him through one download in five steps, and every step has «Пропустить».** Each step outlines the control it talks about:

1. Copy a video link in the browser; pastegrab picks it up when its window gets focus.
2. Or paste the link into the field.
3. Pick what to keep: video, sound, or «Для анализа».
4. For one race out of a long replay, fill in «Фрагмент».
5. Press «Скачать». Files land in Видео, and «Отправить разработчику» sends you the details when something fails.

The steps are data in `app`, each a target control and a message key. The presenter tracks the current step, and the Win32 view draws the panel with native buttons, which Narrator reads and where Enter and Esc work. The panel and the outline windows exist only while the tutorial runs. `settings.json` stores the tutorial version he finished, and a later version with new steps shows once more. The «?» button replays it.

### Browser login

**pastegrab finds the browsers on the PC itself and reads their cookies only when a site refuses without them.** It takes the default browser from the `UserChoice` registry key and looks for Chrome, Edge, Firefox, Yandex Browser and Opera profiles. When a job fails with `NeedsLogin`, `BotCheck` or `BotWall`, it asks once, «Использовать вход из Chrome?», and retries with `--cookies-from-browser`. The answer is remembered per site.

Chrome 109, the last version for Windows 8, encrypts cookies with DPAPI, which yt-dlp reads. If Chrome holds its cookie file open, the message asks him to close Chrome and press «Повторить». yt-dlp has no Yandex Browser option, and whether its Chromium profile reads as `chromium` gets tested in M3.

## 9. Files and updates

```
%LOCALAPPDATA%\Programs\pastegrab\pastegrab.exe
%LOCALAPPDATA%\pastegrab\
  deploy.json               machine profile and installed tool versions, written by setup
  settings.json, queue.json
  logs\                     pastegrab.log, rotated at 1 MB, three files kept
  cache\yt-dlp\             yt-dlp's player and signature cache
  jobs\<id>\                probe output; deleted on success, swept after 7 days
  tmp\                      TEMP for the sidecars
  tools\state.json          active version of each tool
  tools\python\<version>\   tools\yt-dlp\<version>\   tools\ffmpeg\<version>\   tools\qjs\<version>\
```

Everything is per-user, and nothing needs admin rights. A `portable.txt` beside the exe moves the data folder next to it.

- JSON files go to a temp file, get `FlushFileBuffers`, and are swapped in with `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`. The write-through flag covers the rename only, so the flush comes first. A crash leaves either the old file or the new one. Queue saves follow state transitions and happen at most once per second, while progress never touches the disk. The history in `queue.json` keeps the last 200 finished jobs.
- yt-dlp works in `<destination>\.pastegrab-<job id>\`, which goes away whole once the file is in place. Finishing a file is a rename on the same volume, never a copy, and the finished file gets a `Zone.Identifier` stream with `ZoneId=3`, the mark Windows puts on every download.
- Before a fetch, free space must cover twice the expected size, because the video part, the audio part and the merged output exist together for a moment.
- yt-dlp updates run at most daily, in the background after startup, and right away after `EngineOutdated`. The updater downloads the release's `yt-dlp` file, 3 MB of pure Python with yt-dlp-ejs inside, and `SHA2-256SUMS`. It checks the hash, unpacks into a new version folder with zip-slip checks, compiles the bytecode with the bundled Python, runs `--version` from the new folder, and only then switches `state.json`. A release that needs a newer Python fails that check and never goes live. Running jobs finish on the old folder, and the next start deletes it. A nightly channel setting covers weeks when YouTube moves faster than stable releases.
- Python, ffmpeg and QuickJS are pinned and ship with app releases. Python is python-build-standalone's 3.11 install, pruned of Tcl/Tk, IDLE, pip, ensurepip, the test suite and the headers, which takes it from 95 MB to about 50 with yt-dlp's wheels. M0 loaded every `.dll` and `.pyd` in it on 8.0 and 8.1.
- App self-update ships in the first version he gets, because nobody can reach his PC to install a fix. A manifest signed with minisign lists the new build, with a version number that only grows and an expiry date, so a replayed old manifest can't roll him back to a build with a known hole. `pastegrab.exe` is a small launcher that starts `app\<version>\pastegrab-app.exe`, and the previous version stays on disk. A new version that twice fails to reach its main window gets rolled back, and the next report he sends tells you.
- Copy diagnostics puts app and tool versions, the Windows build, recent failures with the tail of their yt-dlp output, and the last 200 log lines on the clipboard. Cookies never reach the log, and neither does an info JSON.

### Reports

**A report goes straight to your Telegram through a bot that exists only for pastegrab.**

1. He presses «Отправить разработчику» and sees the exact text first: versions, the machine profile from `deploy.json`, the failed link with its failure kind, and the last 50 lines of the tool log.
2. pastegrab encrypts the text in age's file format to your X25519 public key, built from `ring`'s X25519, HKDF and ChaCha20-Poly1305, and calls Telegram's `sendDocument` over HTTPS with `report.age` and a caption that names only the app version and the failure kind. At most 5 reports an hour leave one PC.
3. If Telegram can't be reached, pastegrab saves the plain report next to its logs and copies it to the clipboard.

`age -d -i <identity>` opens a report, with the identity kept in your password manager. The public key is no secret and lives in the repository. The token and your chat ID sit in `secrets.toml` at the repository root, which git ignores. You paste them in, and CI writes it from repository secrets. `crates/secrets` reads it at build time, refuses a malformed token or chat ID, and compiles the token XOR-masked with the high bit set in every byte. That keeps it out of `strings` and stops nobody who tries harder. A build without the file keeps step 3 only. Whoever extracts it can post into your chat, rename the bot, and with your chat ID forward old messages and fetch their files, which is why the bodies are ciphertext. In that case `/revoke` in BotFather kills the token, and the next release carries a new one through self-update. Until he installs it, reports fall back to step 3.

## 10. Build and release

- `tools/build/rust` is a podman image with mingw-w64 and a pinned nightly with `rust-src`; `build.sh` runs cargo in it. The targets are `x86_64-win7-windows-gnu` for the app and `i686-win7-windows-gnu` for the installer, built with `-Zbuild-std=std,panic_unwind`. Our binaries import only DLLs that ship with Windows 8, `msvcrt.dll` included, and need neither the VC++ redistributable nor the UCRT. The M0 probe, built this way with rustls, ureq and `windows`, runs on 8.0 and 8.1.
- The release profile sets `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "unwind"` and `overflow-checks = true`. Unwinding lets a worker panic fail one job instead of killing every download. M2 measures `opt-level = "s"` against the size budget, since the hot work runs in the sidecars.
- No PGO, for the same reason.
- Icon, version info and manifest come from `embed-resource` in `build.rs`, through mingw's `windres`. The manifest lists Windows 8, 8.1 and 10 under `supportedOS`. Without the Windows 8 entry, Windows 8 refuses layered child windows and the fades fail, as the probe showed. `windows_subsystem = "windows"` keeps a console window from opening.
- The runtime dependencies are `url`, `linkify`, `serde`, `serde_json`, `ureq`, `rustls`, `webpki-roots`, `sha2`, `zip` with deflate only, `windows`, `log` and `thiserror`. Tests add `proptest`. Tokio and regex stay out, since error patterns are substrings and routes are path templates. No image crate either, because WIC decodes thumbnails.
- `cargo xtask imports` reads the PE import tables of `pastegrab.exe`, `ffmpeg.exe`, `ffprobe.exe`, `qjs.exe` and every DLL and `.pyd` in the Python folder. It fails on any DLL or function missing from `win8-exports.txt`, the exports both lab VMs share, which the same tool dumps on each VM with `--dump-exports`. Delay-loaded imports are checked. Runtime `GetProcAddress` lookups are not, and the VM checklist covers those. For `pastegrab-setup.exe`, which starts from the Downloads folder, every static import must also be on Windows 8's `KnownDLLs` list, which a planted DLL can't shadow. Until the xtask exists, `lab/m0` loads every module through `LoadLibraryEx` inside the VMs, which resolves the same imports.
- ffmpeg starts as gyan.dev's 8.1.1 essentials build, byte for byte the one ProSportsAnalyzer 1.8 runs on his PC. gyan lists Windows 10 as its floor; M0 ran it on 8.0 and 8.1, `h264_mf` included. It already carries libx264, libmp3lame, libopus, libvpx, libaom, Media Foundation, DXVA2 and D3D11VA decoding, gnutls for https and ffmpeg's native legacy decoders, and its 702 imports include none of the Windows 10 functions checked so far.
- Its weakness is size, about 97 MB per executable. A trimmed build of our own, made with mingw-w64 in a podman container and scripted in `tools/build/`, can replace it later behind the same `ToolStore` manifest, each removed component backed by the fixture matrix in section 11. QuickJS gets built that way from the start. At minimum a trimmed ffmpeg needs the demuxers mov, matroska, mpegts, hls, flv, asf, avi, mpegps, mp3, aac, ogg, flac and image2, the decoders h264, hevc, vp8, vp9, libdav1d, mpeg2video, mpeg4, h263, flv, vp6f, wmv1, wmv2, wmv3, vc1, aac, mp3, mp2, ac3, wmav2, opus, vorbis, mjpeg, png and webp, the encoders libx264, aac, libmp3lame, mjpeg, png and gif, the bwdif filter, and https through mbedTLS for the cases where yt-dlp hands ffmpeg a URL. The legacy decoders cover old broadcast archives. x264 makes either build GPL, and pastegrab only spawns it.
- The exe is unsigned, and SmartScreen asks once.

### Installer

**`pastegrab-setup.exe` learns the machine nobody can ask about and fits pastegrab to it. CI builds the binaries, and nothing compiles on his PC.** It targets `i686-win7-windows-gnu`, so it starts on any Windows 7, 8 or 8.1, 32- or 64-bit.

1. It reads the machine: the Windows version and build through `RtlGetVersion`, the native architecture through `GetNativeSystemInfo`, the CPU name and features, RAM, free space, whether the system disk has a seek penalty, the screen size, and whether `ucrtbase.dll` exists.
2. It checks the floor, 64-bit Windows 8.0 or newer. A 32-bit PC gets a plain Russian explanation instead of Windows' cryptic error about an invalid Win32 application. The release manifest has room for an x86 variant, which his PC doesn't need. Then it tunes defaults: parallel downloads from RAM, the x264 preset from the core count, the height cap from the screen.
3. It downloads that variant and its tools from our release over rustls and checks every SHA-256 against a manifest signed with minisign, whose private key never leaves your password manager. It installs per-user, with no admin rights and no UAC prompt.
4. If `ucrtbase.dll` is missing, setup runs Microsoft's VC++ redistributable behind a UAC prompt, because the Python under yt-dlp needs the UCRT. It installs the UCRT system-wide down to Windows 8.0; the lab's 8.0 VM got its UCRT that way. His PC already has it, by the ProSportsAnalyzer evidence.
5. It writes `deploy.json` with the machine profile and the path and version of every tool. The app reads that file at startup instead of probing hardware on every launch.
6. It creates the Start menu shortcut and an uninstall entry under `HKCU` that runs `pastegrab.exe --uninstall`. From M4 it also registers the `pastegrab:` protocol.
7. It runs a self-test: each tool's `--version`, a one-second ffmpeg encode of a test pattern, a QuickJS one-liner. The results and the machine profile go into a Russian report on the last screen.
8. Nothing leaves his PC on its own. The report screen, and every failure in the app, has «Отправить разработчику», which shows the exact text first. Section 9 describes the transport under Reports.

Steps 1 to 5 live in the `deploy` crate, and the app's yt-dlp updater calls the same code. Installs and updates share one tested path, and running setup again repairs or updates an install.

The installer settles the machine check, the tuned defaults, the prerequisites, where tools live, and the hardware report. Two things stay with the app. yt-dlp still updates itself between releases, because YouTube changes faster than you will ship installers. And every binary still has to prove it runs on Windows 8, through the import check and the lab VM.

## 11. Testing

- The link pipeline gets table tests from `tests/links.tsv`, and every link the person reports as misread becomes a row. Property tests check that cleaning is idempotent, that no input panics, and that every output re-parses to itself. `cargo fuzz` targets cover the pipeline and the progress parser.
- Planner tests assert exact format ids and stream actions over format lists shaped like YouTube's, an HLS-only broadcast and an old archive item, plus ffprobe results. Recorded yt-dlp output from real sites joins them with the yt-dlp adapter.
- `ScriptedRunner` replays recorded yt-dlp and ffmpeg transcripts, failures included, and adapter tests assert the emitted events and failure kinds.
- Each contract suite in `testkit` runs against the adapter and the fake.
- Live smoke tests are `#[ignore]` and run before each release, one short public video per site in the corpus.
- The corpus starts with the one olympics.com link you have and the `_TESTS` URLs that yt-dlp keeps in each extractor for olympics.com, YouTube, archive.org, VK, OK.ru, Rutube, Dailymotion and smotrim. Links from his reports join it as they arrive.
- Before each release the lab VM checklist runs: install with `pastegrab-setup.exe`, read its report, paste the corpus, kill pastegrab mid-download and confirm Task Manager shows no orphans, pull the network, fill the disk, check a Russian locale with an emoji title.
- A new Python, yt-dlp, QuickJS or ffmpeg version goes through `lab/m0` on both VMs before it ships, and a new Rust toolchain date goes through `lab/probe`.
- Every release file goes through Defender in both VMs and through VirusTotal, as `docs/release.md` lays out.

### Lab VM

**Two VMs cover what we can't rule out about his PC: `pastegrab-w81`, 64-bit Windows 8.1 Pro, and `pastegrab-w80`, 64-bit Windows 8, both in Russian.** Their files are in `lab/win81-x64/`, `lab/win80-x64/` and `lab/common/`.

- Both ISOs are Microsoft's MSDN images, and Microsoft no longer serves 8.x, so a copy counts only when its SHA-1 matches: `ru_windows_8.1_pro_vl_with_update_x64_dvd_6050899.iso` is `c1987affd4c2545b770cd0c79bfa081dbeea4d04`, `ru_windows_8_x64_dvd_915419.iso` is `b0df41ea9b2f47a205f0c088bd6105b0f3c67d6b`. Each value appears in two independent MSDN hash lists. The 8.1 VL media installs without a key, and the 8.0 media takes Microsoft's generic setup key for core Windows 8. Neither VM is activated.
- Drivers come from virtio-win 0.1.215. Later releases still ship Windows 8 and 8.1 folders, but by 0.1.229 their catalogs carry Red Hat's development signer, which Windows 8.x refuses to load; without a storage driver Setup sees no disk. 0.1.215 has Red Hat's production signature.
- The VMs are paravirtual: virtio-scsi, virtio-net, the guest agent channel, and Hyper-V enlightenments without synic and stimer. SeaBIOS keeps the install unattended and lets internal snapshots work.
- Networking is passt in user mode, which needs no bridge, no firewall rule and no root on the host.
- `autounattend.xml` installs Russian Windows with a local `tester` account and automatic logon, and `firstlogon.ps1` adds the remaining drivers, the QEMU guest agent and no-sleep power settings. Tests drive the VM through `virsh qemu-agent-command`.
- A snapshot named `clean` after setup lets every run start from the same state.
- The UCRT then goes in, and a second snapshot, `ucrt`, mirrors his PC. The Microsoft Update Catalog no longer lists KB2999226 for Windows 8.x, so 8.1 gets its successor KB3118401, and 8.0, which has neither, gets the UCRT from Microsoft's current VC++ redistributable. `clean` keeps testing the path where setup installs it.
- The install ISOs are deleted after setup, with the install drive emptied in each snapshot's definition. Rebuilding a VM needs its ISO back, identified by the SHA-1 above.
- Once per release both VMs boot with QEMU's `Penryn` CPU model instead of host-passthrough. It has no SSE4.2 or AVX, and any binary built for a newer CPU dies there with an illegal instruction before it can do so on his PC.

## 12. Budgets

Targets, not measurements. M0 and M2 measure them on a VM sized like the person's PC, and a release that misses one needs a stated reason.

| What | Budget |
|---|---|
| `pastegrab-app.exe` size | ≤ 4 MB |
| `pastegrab.exe`, the launcher, size | ≤ 512 KB |
| Launch to usable window, warm, launcher included | ≤ 150 ms |
| yt-dlp start to its first line, warm | ≤ 1 s |
| Private memory, idle | ≤ 15 MB |
| Private memory, 3 jobs running, pastegrab alone | ≤ 30 MB |
| Idle CPU | no timers, no wake-ups |
| Link analysis per paste | ≤ 1 ms |
| UI repaints while downloading | ≤ 4 per second |
| Allocations per progress line | none after the first line |

Work the design avoids:

- a yt-dlp launch for a direct media URL;
- a second extraction between probe and fetch;
- a transcode where a remux or a codec-aware pick gives the same output;
- a copy across volumes;
- a whole broadcast downloaded to keep a 30-second race;
- a faststart rewrite of a finished MP4;
- polling of any kind;
- large payloads in pastegrab's memory, since the info JSON stays on disk, downloads stream through one buffer and the history list is virtual;
- parsing human-readable log text for progress;
- sidecar processes alive while idle;
- more than one CPU-heavy job at a time, or one at normal priority.

### Profiling

**Every optimization lands with a before and after number from these, measured on an idle machine on its performance power profile.**

- `cargo bench -p pastegrab-domain` times link analysis, planning and naming with `divan`; the progress-line parser joins when the yt-dlp adapter exists.
- A test with a counting global allocator fails when a progress line allocates after the first.
- `just size` builds the Windows binaries in the toolchain container and prints their sizes, with a per-crate breakdown of the app.
- The lab measures start time and private memory on both VMs, warm and with a cold file cache on an HDD-speed disk.
- Compiler options compete on those numbers: `opt-level` 3, `"s"` and `"z"`, `-Zlocation-detail=none`, `-Zfmt-debug=shallow` and `-Zbuild-std-features=optimize_for_size`. Each binary keeps what wins for it; the launcher and setup lean towards size.

### Per-CPU builds

**Nothing gets a per-CPU build, because the one CPU-heavy part without runtime dispatch, QuickJS, measured no faster for one.** On a captured YouTube challenge, in interleaved runs on one core, `-O3`, LTO and x86-64-v2 or v3 ran no faster than the shipped `-O2`, and PGO gained about 4%. `docs/platform.md` has the numbers.

- pastegrab's own binaries stay baseline x86-64. A UI that waits on pipes gains nothing from AVX2 either.
- ffmpeg and x264 choose their SIMD code at runtime, and python-build-standalone's Windows build is already PGO-optimized.
- The release manifest can still list variants by CPU level, and setup reads CPUID for the machine profile anyway, so a future component that does benefit needs no new mechanism.
- The lever for YouTube's challenge is how often it runs, not how fast: yt-dlp keeps nothing between runs, so every probe pays it in full. Whether one yt-dlp process reuses a solved player across several links gets measured with the yt-dlp adapter, and pastegrab batches probes if it does.

## 13. Milestones

### M0 platform check

**M0 passed on both lab VMs, and `docs/platform.md` has the measurements.** Three yt-dlp packages, QuickJS, QuickJS-NG and gyan's ffmpeg went through YouTube, archive.org, a DASH merge, conversions, Cyrillic install paths and emoji titles. Every module of each package loaded, and `lab/probe` checked a window with a layered child, rustls, a job object and child processes.

Still open:

1. olympics.com from a home connection, in section 15.
2. Start times with the disk throttled to HDD speed.
3. Both VMs on QEMU's `Penryn` CPU model.
4. `xtask imports` with the export lists from both VMs, replacing the load test.

### M1 core on Linux

`domain`, `app`, `ytdlp`, `ffmpeg`, `http`, `store` and the CLI. Done when the CLI downloads and converts every corpus link on Linux and every contract suite passes.

Everything but `store` exists. `pgcli` downloads and converts from YouTube, VK, Rutube, OK.ru and archive.org on Linux, and `pgcli.exe` does the same on the 8.1 VM with the bundled tools, as `docs/platform.md` records. `pgcli --plan` probes a link and prints the planner's pick without downloading. Contract suites exist for `Files` and `ProcessRunner`.

### M2 Windows UI

The `win32` adapter, `pastegrab-setup.exe`, the tutorial, app self-update with rollback, Telegram reports, the VM checklist. Done when his footage sources work in both lab VMs within budget, setup's report arrives in your Telegram, and an update rolls out and back in the lab.

### M3 hardening

Tool updates, failure messages, resume after restart, diagnostics, local file conversion, fragments, the analysis profile, playlists with item selection.

### M4 extras

URL protocol and bookmarklet, drag from the browser, live recording, GIF export, an English string table.

## 14. Risks

| Risk | Plan |
|---|---|
| YouTube's challenge is slow on his CPU | It took 4.3 s per video with QuickJS on the lab's CPU. The fetch reuses the probe's info JSON, so each video pays once. Setup's report shows his CPU, and QuickJS-NG gets measured again if his numbers hurt. |
| yt-dlp drops Python 3.11, most likely when 3.11 reaches its end of life in October 2027 | The updater's smoke test keeps the last release that runs. 8.1 can move to Python 3.12, which yt-dlp keeps a year longer. For 8.0 the fallback is a `Resolver` and `Fetcher` pair that runs yt-dlp on your server and relays the bytes; YouTube ties stream URLs to the requesting IP, which forces the server to proxy. Setup's report says which Windows he has long before then. |
| The Tier 3 target breaks on a newer nightly | Stay on the pinned date in `tools/build/rust`. The MSVC win7 target is the second route. |
| Sites rate-limit the home IP | Browser cookies, per-site backoff, slower playlist pacing. |
| The Python under yt-dlp needs the UCRT, KB2999226 | His PC has it, by the ProSportsAnalyzer evidence, and setup installs it through the VC++ redistributable where it's missing. Our own binaries need only `msvcrt.dll`. |
| The lab's traffic doesn't come from a home connection | YouTube asks shared addresses to sign in far more often than home connections. A YouTube failure in the lab proves nothing until it repeats on a direct connection or on his PC. |
| olympics.com refuses yt-dlp from a home connection too | His main source would need another route. The Olympics YouTube channel carries much of the same footage, and pastegrab handles YouTube. |
| A fragment from a site that needs impersonation fails, because ffmpeg downloads sections | The job falls back to the whole stream and cuts it locally, as section 6 says. |
| Someone extracts the bot token | Spam and bot renames in your chat, and old reports as ciphertext. Revoke it in BotFather and ship a new one in the next release. |
| Unsigned exe | SmartScreen asks once. |
| Site terms | Personal use only. DRM stays off the table. |

## 15. Open questions

His hardware stays unknown by design, and setup's report answers it once he installs pastegrab. Two things need you:

1. olympics.com from a home connection, with the release the lab used: `yt-dlp_linux --ignore-config --impersonate chrome --simulate --print title "<link>"`, where `yt-dlp_linux` is the 2026.10.26 file from yt-dlp's release page. A printed title means his home connection most likely works too. A 403 means his main source needs another route.
2. Before M2, a bot from BotFather and your chat ID with it, pasted into `secrets.toml`. Also an age identity from `age-keygen` in your password manager, whose public line goes into the repository.

## Sources

- The [yt-dlp README's release files table](https://github.com/yt-dlp/yt-dlp#release-files) labels the Windows builds Win10+ from 2026.10.26 on, and [yt-dlp issue #16917](https://github.com/yt-dlp/yt-dlp/issues/16917) explains the move to Python 3.14 and which Python versions Windows 8 and 8.1 run.
- [yt-dlp's EJS wiki page](https://github.com/yt-dlp/yt-dlp/wiki/EJS) lists the supported QuickJS versions, warns that QuickJS before 2025-04-26 can take minutes, and says the official executables and the pure-Python release bundle yt-dlp-ejs.
- `ffmpeg -h protocol=tls` of gyan's 8.1.1 shows `tls_verify` defaulting to false.
- [yt-dlp issue #15012](https://github.com/yt-dlp/yt-dlp/issues/15012) announces the JS runtime requirement of 2025.11.12, with the supported runtimes and their minimum versions.
- The rustc platform pages for [win7-windows-msvc](https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html) and [win7-windows-gnu](https://doc.rust-lang.org/rustc/platform-support/win7-windows-gnu.html), and the [Rust blog post on the Windows baseline](https://blog.rust-lang.org/2024/02/26/Windows-7/).
- [Node.js commit a5decd4c8d](https://github.com/nodejs/node/commit/a5decd4c8d) removes Windows 8.1 and adds the Windows 10 startup check.
- [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) and [gyan.dev](https://gyan.dev/ffmpeg/builds) state their Windows 10 floors.
- `yt-dlp --help` and `--list-extractors` of version 2026.10.26 back every flag and the extractor count above.
- The ffmpeg and UCRT facts about his PC come from unpacking the ProSportsAnalyzer 1.8 release asset and reading the import tables of its `python38.dll` and `ffmpeg.exe`.
- A [Microsoft Q&A answer](https://learn.microsoft.com/en-us/answers/questions/5779246/como-descargar-el-iso-de-w8-1) confirms there is no official Windows 8.1 download page anymore.
- The lab's SHA-1 values match [rg-adguard's MSDN listing](https://files.rg-adguard.net/file/6973d60b-2acd-d230-aa00-fd6d1ec6d6e1) and a [pastebin hash table](https://pastebin.com/nZx0UtAB) for 8.1, and [Habr's 2012 MSDN list](https://habr.com/ru/articles/149670/) and [dowsoft.ru](https://dowsoft.ru/msdn-windows/windows-8/) for 8.0.
- A [Proxmox forum thread](https://forum.proxmox.com/threads/lastest-vertio-version-for-windows-server-2012.172228/) reports 0.1.215 working on 8.1 and later releases failing as unsigned. The signer inside each release's `vioscsi.cat` confirms it.
