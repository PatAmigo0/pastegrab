# pastegrab

A video downloader for Windows 8.x: paste any text with a link in it, get the video or its sound as a file that plays on that PC. Rust with native Win32 controls on top of yt-dlp and ffmpeg.

Early stage. `SPEC.md` holds the design, the decisions and the milestones; the link pipeline in `crates/domain` is the first part that runs.

| Path | What |
|---|---|
| `SPEC.md` | design, decisions, milestones |
| `crates/domain` | pure core: finds, unwraps, cleans and classifies links |
| `lab/` | Windows 8 and 8.1 test VMs for libvirt |

On Linux, `just check` runs rustfmt, clippy at pedantic level with warnings as errors, and the tests. Windows builds need a nightly toolchain with the Tier 3 `x86_64-win7-windows-msvc` target, as section 10 of the spec describes.
