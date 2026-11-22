#!/usr/bin/env bash
# build-setup.sh TOOLS_DIR LAUNCHER APP_FILE...: packs the launcher as pastegrab.exe, the app files
# into app/<version>/ and the python, qjs and ffmpeg folders of TOOLS_DIR into one compressed tar,
# then builds pastegrab-setup.exe around it and the VC++ redistributable.
# needs: cargo tar zstd podman
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
tools=$(cd "$1" && pwd)
launcher=$2
shift 2
pkgid=$(cd "$root" && cargo pkgid -p pastegrab-window)
version=${pkgid##*@}
out=$root/target/payload
rm -rf "$out/tree"
mkdir -p "$out/tree/app/$version" "$out/tree/tools"
cp "$launcher" "$out/tree/pastegrab.exe"
for tool in python qjs ffmpeg; do cp -r "$tools/$tool" "$out/tree/tools/$tool"; done
for file in "$@"; do cp "$file" "$out/tree/app/$version/"; done
# Sorted names without owners or times make the same inputs give the same bytes.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 --format=gnu \
    -C "$out/tree" -cf "$out/payload.tar" pastegrab.exe app tools
rm -rf "$out/tree"
size=$(stat -c %s "$out/payload.tar")
# A 128 MiB window lets ffmpeg.exe and ffprobe.exe share their common code; the setup's decoder
# accepts that much and no more.
nice -n 10 zstd -q -f -19 --long=27 -T2 "$out/payload.tar" -o "$out/payload.tar.zst"
rm "$out/payload.tar"
"$root/tools/release/fetch-redist.sh" >/dev/null
PASTEGRAB_PAYLOAD=/src/target/payload/payload.tar.zst PASTEGRAB_PAYLOAD_SIZE=$size \
    PASTEGRAB_REDIST=/src/target/redist/vc_redist.x64.exe \
    "$root/tools/build/rust/build.sh" "$root" i686-win7-windows-gnu -p pastegrab-setup
setup=$root/target/win7/i686-win7-windows-gnu/release/pastegrab-setup.exe
echo "$setup: $(stat -c %s "$setup") bytes, $size unpacked"
