# pastegrab

**A desktop app for Windows 8.x that takes any pasted text with a link in it and saves the video, or only its audio, as a file that machine can play.** One non-technical person uses it, mostly for old Olympic and other sports footage that he studies in ProSportsAnalyzer. You maintain it from a distance.

Nothing is built yet. Section 1 lists the choices, section 13 says what M0 has to prove before any app code exists, and section 15 holds the open questions. The test VM lives in `lab/`, described in section 11.

## 1. Decisions

| Area | Choice | Reason |
|---|---|---|
| Site support | yt-dlp as a child process | 1,755 extractors in 2026.10.26 and frequent releases that track YouTube's changes. Rust extractors would mean owning that churn forever. |
| Direct media links | Downloaded in-process | A plain `.mp4` URL needs no Python start. |
| Merging and conversion | ffmpeg as a child process: gyan.dev's 8.1.1 essentials build | The exact build ProSportsAnalyzer 1.8 already runs on his PC. gyan lists Windows 10 as its floor, which the lab checks on 8.0 and 8.1. |
| YouTube's JavaScript challenge | QuickJS, our own build | Since 2025.11.12 yt-dlp needs a JS runtime for YouTube. Deno and Node embed V8, and current Node refuses to start below Windows 10. |
| UI | Native Win32 through the `windows` crate | No GPU or WebView dependency, native look and screen reader support, a few MB of memory. WebView2 stopped at version 109 on 8.1, which rules out Tauri. |
| UI language | Russian only, every string in one table per language | A language added later is one new file, and the compiler rejects it while a string is missing. |
| Installation | `pastegrab-setup.exe`, a 32-bit Rust bootstrapper | It runs on any Windows 8.x, measures the machine nobody can ask about, installs the 64-bit build, tunes settings to the hardware and handles prerequisites. |
| Concurrency | `std::thread` and channels, no async runtime | A handful of child processes and sockets at most. Blocking reads on pipes fit that directly. |
| Toolchain | Pinned nightly, Tier 3 `x86_64-win7-windows-msvc`, `-Zbuild-std`, static CRT | Tier 1 Windows targets need Windows 10 since Rust 1.78. The win7 targets keep std's fallbacks for 7, 8 and 8.1. |
| Windows 8 guarantee | CI checks every PE import against a Windows 8.1 export list | A Windows 10 import becomes a failed build instead of a crash on the person's PC. |
| TLS in our code | rustls with `ring` and bundled Mozilla roots | SChannel on 8.1 has no TLS 1.3, and the root store of an unpatched PC goes stale. |
| Serialization | `serde_json` for yt-dlp output, settings and the queue | One parser in the binary. |
| Reports | A Telegram bot, token compiled into the exe | No server to run. A leaked token costs a revoke and a release, and reports wait in a local file meanwhile. |
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
| BtbN's FFmpeg builds start at Windows 10 22H2, gyan.dev's at Windows 10 | gyan's 8.1.1 essentials, proven on his PC, confirmed in the lab; our own build only if the lab says no |
| yt-dlp labels `yt-dlp.exe`, `yt-dlp_x86.exe` and `yt-dlp_win.zip` as Win8+ | Official yt-dlp builds, confirmed on a VM in M0 |
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
  lab/           test VMs: libvirt XML, unattend files
  xtask/         import check, sidecar builds, packaging
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
| Liskov substitution | Every port has a contract suite in `testkit`, and every implementation, fakes included, runs it. A resolver must return `Unsupported` for links outside its business, because the resolver chain advances only on that kind. | `cargo test` |
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
- `FetchPlan` holds the exact format ids, the merge container, the section and the expected size. `Convert` holds a copy, encode or drop decision per stream and the exact cut.
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
6. The plan names exact format ids such as `137+140`. A selector string would let yt-dlp choose again at fetch time, and the plan would stop being testable.

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

## 7. Engines

### yt-dlp

**Each item costs two yt-dlp runs and one site extraction, because the probe saves the full info JSON and the fetch loads it back.**

Probe:

```
yt-dlp <fixed flags> --no-simulate --skip-download --write-info-json
       -o "<job>\probe" --print-to-file "<summary template>" "<job>\summary.json"
       -- "<url>"
```

For YouTube the full info JSON is large, since it lists every format URL and caption track. It stays on disk. pastegrab reads only `summary.json`, a few kilobytes built with a `%(...)j` template over the fields the planner needs. Playlists add `--flat-playlist` and get one summary line per entry. Entries are probed one at a time as their turn comes, and a 300-item playlist never starts 300 probes.

Fetch:

```
yt-dlp <fixed flags> --load-info-json "<job>\probe.info.json" -f 137+140
       --merge-output-format mp4 -P "home:<dest>" -P "temp:<dest>\.pastegrab"
       -o "%(title)s.%(ext)s" --print-to-file after_move:filepath "<job>\result.txt"
```

Fixed flags, every one present in `yt-dlp --help` of 2026.10.26:

```
--ignore-config --no-plugin-dirs --no-remote-components
--no-js-runtimes --js-runtimes "quickjs:<tools>\qjs\<version>\qjs.exe"
--ffmpeg-location "<tools>\ffmpeg\<version>" --cache-dir "<data>\cache\yt-dlp"
--quiet --progress --newline --progress-delta 0.5 --color never
--progress-template "download:@pg|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"
--progress-template "postprocess:@pp|%(progress.postprocessor)s|%(progress.status)s"
--encoding utf-8 --windows-filenames --trim-filenames 120
-N 4 --retries 10 --fragment-retries 10
```

Adapter rules:

- Use the unpacked `yt-dlp_win.zip` build, not `yt-dlp.exe`. The single-file exe is a PyInstaller bundle that extracts its Python runtime to `%TEMP%` on every launch, twice per item here, on an HDD where Defender's real-time scan reads each extracted file. The zip build can't run `-U`, and `ToolStore` updates it instead, as section 9 describes.
- `--` comes before every URL. Input starting with `-` can never turn into an option; the parser rejects non-http input first, and this is the second lock.
- Nothing parsed depends on the console code page. Python writes to a pipe in the ANSI code page, cp1251 on a Russian Windows. Progress lines are ASCII by construction, the summary and the final path come from files yt-dlp writes in UTF-8, and error text is decoded lossily for display only.
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

**olympics.com, his main source, sits behind Akamai's bot filter.** It answers yt-dlp's plain client with 403 and curl with an HTTP/2 stream reset, and lets browser-like TLS through. That makes `--impersonate` mandatory for him, which in turn depends on `curl_cffi` inside the official yt-dlp build running on Windows 8, so M0 checks it first. If it doesn't run, the fallback to test is `--cookies-from-browser chrome`, since his Chrome already holds Akamai's cookies from real visits.

### Direct HTTP

- ureq with rustls on `ring` and `webpki-roots`, one 256 KiB buffer per download.
- Writes `<name>.part` in the destination folder and renames it when done.
- Resumes with `Range` and `If-Range` when the server sent `ETag` or `Last-Modified`.
- Reserves the expected size up front through `FileAllocationInfo` when the server reports it, which keeps the file in fewer fragments on an HDD.
- Socket read timeouts; the cancel flag is checked between chunks.

### ffmpeg

- `ffmpeg.exe` and `ffprobe.exe` come from gyan.dev's essentials build, pinned at 8.1.1, as section 10 describes.
- Encoder capabilities are probed once per ffmpeg version and cached in settings.
- Arguments come from a pure function of the planner's `Convert`, covered by golden tests.

### Process control

- Spawn suspended with `CREATE_NO_WINDOW`, assign to a new Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resume. Every process yt-dlp starts later, ffmpeg included, inherits the job.
- Cancel calls `TerminateJobObject`. When pastegrab dies, its job handles close and Windows kills every child.
- Windows 8 added nested jobs, and this works even when a launcher already put pastegrab inside a job.
- `SetErrorMode(SEM_FAILCRITICALERRORS)` before the first spawn. Children inherit it, and a missing DLL returns an exit code instead of a system dialog in front of the person.
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
  tools\state.json          active version of each tool
  tools\yt-dlp\<version>\   tools\ffmpeg\<version>\   tools\qjs\<version>\
```

Everything is per-user, and nothing needs admin rights. A `portable.txt` beside the exe moves the data folder next to it.

- JSON files go to a temp file first and get swapped in with `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`. A crash leaves either the old file or the new one. Queue saves follow state transitions and happen at most once per second, while progress never touches the disk.
- yt-dlp's temp folder is `<destination>\.pastegrab\`. Finishing a file is a rename on the same volume, never a copy.
- Before a fetch, free space must cover twice the expected size, because the video part, the audio part and the merged output exist together for a moment.
- yt-dlp updates run at most daily, in the background after startup, and right away after `EngineOutdated`. The updater downloads `yt-dlp_win.zip` and `SHA2-256SUMS` from the latest release, checks the hash, unpacks into a new version folder with zip-slip checks, then switches `state.json`. Running jobs finish on the old folder, and the next start deletes it. A nightly channel setting covers weeks when YouTube moves faster than stable releases.
- ffmpeg and QuickJS are pinned and ship with app releases.
- App self-update ships in the first version he gets, because nobody can reach his PC to install a fix. A manifest signed with minisign lists the new build. `pastegrab.exe` is a small launcher that starts `app\<version>\pastegrab-app.exe`, and the previous version stays on disk. A new version that twice fails to reach its main window gets rolled back, and the next report he sends tells you.
- Copy diagnostics puts app and tool versions, the Windows build, recent failures with the tail of their yt-dlp output, and the last 200 log lines on the clipboard. Cookies never reach the log.

### Reports

**A report goes straight to your Telegram through a bot that exists only for pastegrab.**

1. He presses «Отправить разработчику» and sees the exact text first: versions, the machine profile from `deploy.json`, the failed link with its failure kind, and the last 50 lines of the tool log.
2. pastegrab calls Telegram's `sendDocument` over HTTPS with `report.txt` and a one-line caption, addressed to your chat with the bot. At most 5 reports an hour leave one PC.
3. If Telegram can't be reached, pastegrab saves the report next to its logs and copies it to the clipboard.

The token and your chat ID come from your password manager at build time, into environment variables that `option_env!` reads. They never land in git, and a build without them keeps step 3 only. Inside the exe the token is XOR-masked, which keeps it out of `strings` and stops nobody who tries harder. Whoever extracts it can post into your chat through the bot and rename it; the rest of your Telegram stays out of reach. In that case `/revoke` in BotFather kills the token, and the next release carries a new one through self-update. Until he installs it, reports fall back to step 3.

## 10. Build and release

- `rust-toolchain.toml` pins a nightly date with `rust-src`. The targets are `x86_64-win7-windows-msvc` for the app and `i686-win7-windows-msvc` for the installer, built with `-Zbuild-std=std,panic_unwind` and `+crt-static`. The PC needs neither the VC++ redistributable nor the UCRT update. `cargo-xwin` cross-builds from Linux. If that fails in M0, the fallbacks are a native build in a Windows VM, then `x86_64-win7-windows-gnu` with mingw-w64.
- The release profile sets `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "unwind"`. Unwinding lets a worker panic fail one job instead of killing every download.
- No PGO, since the hot work runs inside yt-dlp and ffmpeg.
- Icon, version info and manifest come from `embed-resource` in `build.rs`.
- The runtime dependencies are `url`, `linkify`, `serde`, `serde_json`, `ureq`, `rustls`, `webpki-roots`, `sha2`, `zip` with deflate only, `windows`, `log` and `thiserror`. Tests add `proptest`. Tokio and regex stay out, since error patterns are substrings and routes are path templates. No image crate either, because WIC decodes thumbnails.
- `cargo xtask imports` reads the PE import tables of `pastegrab.exe`, `ffmpeg.exe`, `ffprobe.exe`, `qjs.exe` and every DLL and `.pyd` in the yt-dlp folder. It fails on any DLL or function missing from `win81-exports.txt`, which the same tool generates once on a Windows 8.1 VM with `--dump-exports`. Delay-loaded imports are checked. Runtime `GetProcAddress` lookups are not, and the VM checklist covers those.
- ffmpeg starts as gyan.dev's 8.1.1 essentials build, byte for byte the one ProSportsAnalyzer 1.8 runs on his PC. gyan lists Windows 10 as its floor, and the lab proves it on 8.0 and 8.1 before pastegrab relies on it. It already carries libx264, libmp3lame, libopus, libvpx, libaom, Media Foundation, DXVA2 and D3D11VA decoding, gnutls for https and ffmpeg's native legacy decoders, and its 702 imports include none of the Windows 10 functions checked so far.
- Its weakness is size, about 97 MB per executable. A trimmed build of our own, made with mingw-w64 in a podman container and scripted in `tools/build/`, can replace it later behind the same `ToolStore` manifest, each removed component backed by the fixture matrix in section 11. QuickJS gets built that way from the start. At minimum a trimmed ffmpeg needs the demuxers mov, matroska, mpegts, hls, flv, asf, avi, mpegps, mp3, aac, ogg, flac and image2, the decoders h264, hevc, vp8, vp9, libdav1d, mpeg2video, mpeg4, h263, flv, vp6f, wmv1, wmv2, wmv3, vc1, aac, mp3, mp2, ac3, wmav2, opus, vorbis, mjpeg, png and webp, the encoders libx264, aac, libmp3lame, mjpeg, png and gif, the bwdif filter, and https through mbedTLS for the cases where yt-dlp hands ffmpeg a URL. The legacy decoders cover old broadcast archives. x264 makes either build GPL, and pastegrab only spawns it.
- The exe is unsigned, and SmartScreen asks once.

### Installer

**`pastegrab-setup.exe` learns the machine nobody can ask about and fits pastegrab to it. CI builds the binaries, and nothing compiles on his PC.** It targets `i686-win7-windows-msvc`, so it starts on any Windows 7, 8 or 8.1, 32- or 64-bit.

1. It reads the machine: the Windows version and build through `RtlGetVersion`, the native architecture through `GetNativeSystemInfo`, the CPU name and features, RAM, free space, whether the system disk has a seek penalty, the screen size, and whether `ucrtbase.dll` exists.
2. It checks the floor, 64-bit Windows 8.0 or newer. A 32-bit PC gets a plain Russian explanation instead of Windows' cryptic error about an invalid Win32 application. The release manifest has room for an x86 variant, which his PC doesn't need. Then it tunes defaults: parallel downloads from RAM, the x264 preset from the core count, the height cap from the screen.
3. It downloads that variant and its tools from our release over rustls and checks every SHA-256 against a manifest signed with minisign, whose private key never leaves your password manager. It installs per-user, with no admin rights and no UAC prompt.
4. If `ucrtbase.dll` is missing and yt-dlp's Python needs it, the UCRT DLLs go next to yt-dlp. Microsoft supports this app-local UCRT on Windows 7 to 8.1, and it needs no admin rights either.
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

### Lab VM

**Two VMs cover what we can't rule out about his PC: `pastegrab-w81`, 64-bit Windows 8.1 Pro, and `pastegrab-w80`, 64-bit Windows 8, both in Russian.** Their files are in `lab/win81-x64/`, `lab/win80-x64/` and `lab/common/`.

- Both ISOs are Microsoft's MSDN images, and Microsoft no longer serves 8.x, so a copy counts only when its SHA-1 matches: `ru_windows_8.1_pro_vl_with_update_x64_dvd_6050899.iso` is `c1987affd4c2545b770cd0c79bfa081dbeea4d04`, `ru_windows_8_x64_dvd_915419.iso` is `b0df41ea9b2f47a205f0c088bd6105b0f3c67d6b`. Each value appears in two independent MSDN hash lists. The 8.1 VL media installs without a key, and the 8.0 media takes Microsoft's generic setup key for core Windows 8. Neither VM is activated.
- Drivers come from virtio-win 0.1.240, the last release with Windows 8 and 8.1 folders.
- The VMs are paravirtual: virtio-scsi, virtio-net, the guest agent channel, and Hyper-V enlightenments without synic and stimer. SeaBIOS keeps the install unattended and lets internal snapshots work.
- Networking is passt in user mode, which needs no bridge, no firewall rule and no root on the host.
- `autounattend.xml` installs Russian Windows with a local `tester` account and automatic logon, and `firstlogon.ps1` adds the remaining drivers, the QEMU guest agent and no-sleep power settings. Tests drive the VM through `virsh qemu-agent-command`.
- A snapshot named `clean` after setup lets every run start from the same state.
- KB2999226 from the Microsoft Update Catalog then goes in, and a second snapshot, `ucrt`, mirrors his PC. `clean` keeps testing setup's app-local UCRT path.
- Once per release both VMs boot with QEMU's `Penryn` CPU model instead of host-passthrough. It has no SSE4.2 or AVX, and any binary built for a newer CPU dies there with an illegal instruction before it can do so on his PC.

## 12. Budgets

Targets, not measurements. M0 and M2 measure them on a VM sized like the person's PC, and a release that misses one needs a stated reason.

| What | Budget |
|---|---|
| `pastegrab.exe` size | ≤ 4 MB |
| Launch to usable window, warm | ≤ 150 ms |
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

## 13. Milestones

### M0 platform check

Go or no-go in both lab VMs, before any app code:

1. A win7-target hello world opens a window and does a rustls GET to github.com.
2. The `yt-dlp_win.zip` build prints `--version`, then downloads a short YouTube video and a short Vimeo video with QuickJS as the runtime. Time the YouTube challenge.
3. gyan's ffmpeg 8.1.1 merges a DASH pair, converts VP9 to H.264 and runs the `h264_mf` test.
4. `--impersonate chrome` gets the olympics.com test link past Akamai, which depends on curl_cffi inside the yt-dlp build.
5. Launch time and memory of `yt-dlp.exe` against the unpacked build, measured cold with the VM's disk throttled to HDD speed through libvirt `iotune`.
6. Cyrillic and emoji titles survive on a Russian-locale install.
7. `win81-exports.txt` exists, and the import check passes for every binary.

Results go to `docs/platform.md` with the VM's specs. A failure at step 2 switches to the fallbacks in section 14.

### M1 core on Linux

`domain`, `app`, `ytdlp`, `ffmpeg`, `http`, `store` and the CLI. Done when the CLI downloads and converts every corpus link on Linux and every contract suite passes.

### M2 Windows UI

The `win32` adapter, `pastegrab-setup.exe`, the tutorial, app self-update with rollback, Telegram reports, the VM checklist. Done when his footage sources work in both lab VMs within budget, setup's report arrives in your Telegram, and an update rolls out and back in the lab.

### M3 hardening

Tool updates, failure messages, resume after restart, diagnostics, local file conversion, fragments, the analysis profile, playlists with item selection.

### M4 extras

URL protocol and bookmarklet, drag from the browser, live recording, GIF export, an English string table.

## 14. Risks

| Risk | Plan |
|---|---|
| QuickJS is too slow for YouTube's challenge | Measure in M0, then try quickjs-ng. Without a runtime yt-dlp warns that YouTube formats may be missing; how many remain is part of the M0 measurement. |
| A later yt-dlp drops Windows 8 | Pin the last working release, then build the unpacked package from source with a Python that still runs on 8.1. |
| No yt-dlp build runs on 8.1 at all | A `Resolver` and `Fetcher` pair that runs yt-dlp on your server and relays the bytes. YouTube ties stream URLs to the requesting IP, which forces the server to proxy. A new adapter, not a rewrite. |
| The Tier 3 target breaks on a newer nightly | Stay on the pinned date. The GNU win7 target is the second route. |
| Sites rate-limit the home IP | Browser cookies, per-site backoff, slower playlist pacing. |
| The Python inside yt-dlp needs the UCRT, KB2999226 | His PC has it, by the ProSportsAnalyzer evidence. The lab VMs start without it and get it in a second snapshot that mirrors his PC. Our own binaries link the CRT statically. |
| The lab's traffic leaves through a VPN | YouTube asks VPN exits to sign in far more often than home connections. A YouTube failure in the lab proves nothing until it repeats on a direct connection or on his PC. |
| Someone extracts the bot token | Spam in your chat at worst. Revoke it in BotFather and ship a new one in the next release. |
| Unsigned exe | SmartScreen asks once. |
| Site terms | Personal use only. DRM stays off the table. |

## 15. Open questions

None right now. His hardware stays unknown by design, and setup's report answers it once he installs pastegrab. Before M2 you need a bot from BotFather, your chat ID with it, and both stored in your password manager.

## Sources

- The [yt-dlp README's release files table](https://github.com/yt-dlp/yt-dlp#release-files) labels `yt-dlp.exe`, `yt-dlp_x86.exe` and `yt-dlp_win.zip` as Win8+.
- [yt-dlp issue #15012](https://github.com/yt-dlp/yt-dlp/issues/15012) announces the JS runtime requirement of 2025.11.12, with the supported runtimes and their minimum versions.
- The rustc platform pages for [win7-windows-msvc](https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html) and [win7-windows-gnu](https://doc.rust-lang.org/rustc/platform-support/win7-windows-gnu.html), and the [Rust blog post on the Windows baseline](https://blog.rust-lang.org/2024/02/26/Windows-7/).
- [Node.js commit a5decd4c8d](https://github.com/nodejs/node/commit/a5decd4c8d) removes Windows 8.1 and adds the Windows 10 startup check.
- [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) and [gyan.dev](https://gyan.dev/ffmpeg/builds) state their Windows 10 floors.
- `yt-dlp --help` and `--list-extractors` of version 2026.10.26 back every flag and the extractor count above.
- The ffmpeg and UCRT facts about his PC come from unpacking the ProSportsAnalyzer 1.8 release asset and reading the import tables of its `python38.dll` and `ffmpeg.exe`.
- A [Microsoft Q&A answer](https://learn.microsoft.com/en-us/answers/questions/5779246/como-descargar-el-iso-de-w8-1) confirms there is no official Windows 8.1 download page anymore.
- The lab's SHA-1 values match [rg-adguard's MSDN listing](https://files.rg-adguard.net/file/6973d60b-2acd-d230-aa00-fd6d1ec6d6e1) and a [pastebin hash table](https://pastebin.com/nZx0UtAB) for 8.1, and [Habr's 2012 MSDN list](https://habr.com/ru/articles/149670/) and [dowsoft.ru](https://dowsoft.ru/msdn-windows/windows-8/) for 8.0.
- A [Proxmox forum thread](https://forum.proxmox.com/threads/lastest-vertio-version-for-windows-server-2012.172228/) points to virtio-win 0.1.240 as the last release for the 2012 R2 generation; the ISO listing confirms its `w8.1` folders.
