# Platform check

**Every tool pastegrab needs runs on Windows 8.0 and 8.1, with yt-dlp on our own Python 3.11 package.** yt-dlp's official Windows builds require Windows 10 from release 2026.10.26 on, when they moved to Python 3.14. yt-dlp's maintainers report that Windows 8 runs Python up to 3.11 and Windows 8.1 up to 3.12, and current yt-dlp needs at least 3.11. That leaves Python 3.11 as the one version both systems run.

## Lab machines

| VM | Windows | CPU | RAM | Disk |
|---|---|---|---|---|
| `pastegrab-w81` | 8.1 Pro 6.3.9600, Russian, ANSI code page 1251 | host's Core Ultra 5 125U, 2 vCPUs | 4 GiB | qcow2 on NVMe, virtio-scsi |
| `pastegrab-w80` | 8 6.2.9200, Russian, ANSI code page 1251 | same | 4 GiB | same |

Both ran from their `ucrt` snapshot. The UCRT is 10.0.10586.9 on 8.1 from KB3118401 and 10.0.10240.16390 on 8.0 from the VC++ redistributable. Each VM reached the internet through passt and the host's own route.

## Candidates

| Name | Contents | Python | OpenSSL | curl_cffi |
|---|---|---|---|---|
| official | `yt-dlp_win.zip` 2026.10.21, the last official build for Windows 8 | 3.10.11 | 1.1.1t | 0.16.3 |
| embeddable | python.org's 3.11.9 embeddable package, yt-dlp 2026.10.26's pure-Python release file, the wheels yt-dlp pins | 3.11.9 | 3.0.13 | 0.17.0 |
| standalone | python-build-standalone 3.11.18, the same release file and wheels | 3.11.18 | 3.5.9 | 0.17.0 |

QuickJS 2026-06-04 and QuickJS-NG 0.18.0 came from `tools/build/quickjs`, built with mingw-w64. Both import only `KERNEL32.dll` and `msvcrt.dll`. ffmpeg is gyan.dev's 8.1.1 essentials build.

## Results

Every check passed on both VMs, for all three packages.

| Check | 8.1 | 8.0 |
|---|---|---|
| yt-dlp starts, curl_cffi loads, `--list-impersonate-targets` lists Chrome and Safari | pass | pass |
| YouTube probe of `jNQXAC9IVRw` with QuickJS: all 25 formats | pass | pass |
| The same probe with `--impersonate chrome` | pass | pass |
| VP9 242 plus Opus 251 merged into MKV by ffmpeg | pass | pass |
| archive.org MPEG-1 original, 10.4 MB | pass | pass |
| Install folder and TEMP under `Иван Петров`, QuickJS reading its script from there | pass | pass |
| File name with Cyrillic and an emoji | pass | pass |
| ffmpeg `h264_mf` encode, VP9 to H.264, MPEG-1 to H.264, `bwdif` | pass | pass |
| Every `.dll` and `.pyd` of each package loads through `LoadLibraryEx`: 116, 77 and 91 modules | pass | pass |
| `lab/probe`: `RtlGetVersion`, threads, a Unicode file name, child processes, a kill-on-close job, rustls to github.com, a window with a layered child | pass | pass |

The probe's layered child failed on 8.0 until its manifest listed Windows 8 under `supportedOS`. Windows 8 creates layered child windows only for programs that declare it.

## YouTube's JS check

**QuickJS 2026-06-04 solves YouTube's challenge in about 4.3 s on these VMs, QuickJS-NG in about 5.2 s.** The 8.0 runs shared the host with the 8.1 VM and took up to 5.7 and 7.8 s. yt-dlp kept nothing between runs, and a second probe of the same video paid the full time again. pastegrab ships Bellard's QuickJS and keeps the probe's info JSON for the fetch, which solves the challenge once per video.

## Memory and startup

| Package | `--version`, first run on 8.1 | Peak private memory, YouTube probe |
|---|---|---|
| official | 0.32 s | 94 MB |
| embeddable | 1.40 s | 105 MB |
| standalone | 2.10 s | 105 MB |

**yt-dlp's release file holds 1,046 `.py` files and no bytecode,** and Python compiles every module it imports on each launch. The official build ships compiled bytecode. Unpacking the release and compiling it once takes 2.5 to 2.7 s on 8.1. After that, `--version` takes 0.32 to 0.41 s on either Python, the official build's time, and private memory at start drops from 52 to 37 MB.

## Startup on a slow disk

**With the disk capped at 80 operations and 80 MB a second, about a 5400 rpm laptop drive, a start with an empty file cache takes 9 to 38 s, and the number of files read decides it.** Each start below followed a drop of Windows' file cache and 20 s of settling, three runs per package, on 8.1.

| Package | Cold | Warm |
|---|---|---|
| official, single `yt-dlp.exe` | 11.5 to 20.8 s | 2.2 s |
| official, unpacked | 8.7 to 16.2 s | 0.27 s |
| embeddable, release file | 12.5 to 20.2 s | 0.66 s |
| standalone, release file | 16.7 to 38 s | 0.64 to 1.1 s |
| embeddable, compiled | 22 to 24 s | 0.29 to 1.1 s |
| standalone, compiled | 20.7 to 28 s | 0.29 to 0.41 s |

Compiling wins every warm start and loses every cold one. A thousand loose `.pyc` files cost a thousand seeks, where the zipped release costs one long read. The official build keeps its bytecode in one archive, which is why it starts fastest from a cold disk. The single-file `yt-dlp.exe` unpacks itself on every start, warm or cold.

## QuickJS builds

**Compiler flags don't make QuickJS faster at YouTube's challenge; PGO gains about 4%.** The input was a challenge script captured from yt-dlp 2026.10.26, 3.3 MB of solver and player code. Builds came from `tools/build/quickjs`'s container with gcc 14, as Linux binaries on the host, each pinned to the same efficient core and run in turns, three rounds:

| Build | Runs |
|---|---|
| `-O2`, what ships | 4.88, 4.94, 4.95 s |
| `-O3`, LTO, x86-64-v2 | 5.17, 5.27, 5.41 s |
| PGO, LTO, x86-64 | 4.68, 4.72, 4.78 s |
| PGO, LTO, x86-64-v3 | 4.76, 4.79, 5.22 s |

A first pass with hyperfine ran each build's five runs back to back and showed LTO and PGO two to three times faster. In that pass `-O2` took 17 s, against 4.9 s interleaved, so the host's changing load moved the numbers more than any flag did. Interleaving spreads that load across all builds alike.

yt-dlp also keeps no solved challenge between runs: two probes of one video with a shared cache folder both handed QuickJS the full 3.3 MB script.

## Core benchmarks

`cargo bench -p pastegrab-domain` on the host, loaded with other work and on its power-saving profile, so the numbers say only how far each sits from its budget:

| Bench | Median |
|---|---|
| link analysis, a paste with two links | 7.0 µs |
| link analysis, a chat paste with 100 links | 368 µs |
| format plan for a YouTube-sized list | 1.06 µs |
| file name for a Russian title with an emoji | 1.57 µs |

The budget for link analysis is 1 ms per paste.

## Text encoding

**On a Russian Windows yt-dlp writes cp1251 into a pipe and drops every character outside it.** A title with 🏃 came through as two spaces. `--encoding utf-8` delivered the exact title from all three packages, and so did `-X utf8` on the Python 3.11 packages. A `%(title)j` print template produced an ASCII JSON string with `🏃` escapes. File names on disk were exact in every case.

QuickJS reads its script path in the ANSI code page. A path that cp1251 can't represent would break the challenge.

## olympics.com

**From the lab's network, olympics.com returned HTTP 403 to yt-dlp 2026.10.26, with and without `--impersonate chrome`.** That network isn't a home connection, and M0 didn't establish whether Akamai blocks its address or the request itself. The VMs didn't test the site.

## Not covered

- Vimeo needs a login for every test video in yt-dlp's extractor, so archive.org took its place.
- The import check ran as a load test inside the VMs, not yet as `cargo xtask imports` against export lists.
- QEMU's `Penryn` CPU model, the stand-in for a CPU without SSE4.2.
