#!/usr/bin/env bash
# Assembles the M0 packages from $M0_DIR/dl and burns them, with m0.ps1, to the PGM0 disc.
# QuickJS comes from tools/build/quickjs/build.sh "$M0_DIR/qjs".
# needs: bsdtar tar xorriso
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
base=${M0_DIR:-/mnt/storage/pastegrab-m0}
iso=${M0_ISO:-/mnt/storage/ISO/pastegrab-m0-tools.iso}
dl=$base/dl
st=$base/stage
work=$base/work
rm -rf "$st" "$work"
mkdir -p "$st" "$work/deps"

# Both Python builds put this one folder of unpacked wheels on sys.path.
for w in "$dl"/wheels/*.whl; do bsdtar -xf "$w" -C "$work/deps"; done

# p1: the last official Windows 8 build as published, p1x its single-file twin.
cp "$dl/ytdlp-20261021/yt-dlp_win.zip" "$st/p1.zip"
mkdir -p "$st/p1x"
cp "$dl/ytdlp-20261021/yt-dlp.exe" "$st/p1x/"

# p2: python.org's 3.11.9 embeddable, whose ._pth file is the whole sys.path.
mkdir -p "$work/p2/python"
bsdtar -xf "$dl/python-3.11.9/python-3.11.9-embed-amd64.zip" -C "$work/p2/python"
cp -r "$work/deps" "$work/p2/python/deps"
printf 'python311.zip\r\n.\r\ndeps\r\n' > "$work/p2/python/python311._pth"
cp "$dl/ytdlp-20261026/yt-dlp" "$work/p2/yt-dlp"
(cd "$work/p2" && bsdtar -a -cf "$st/p2.zip" python yt-dlp)

# p3: python-build-standalone 3.11.18 with the same wheels in site-packages.
mkdir -p "$work/p3"
tar -xzf "$dl"/pbs-3.11.18/cpython-3.11.18+*-install_only_stripped.tar.gz -C "$work/p3"
mkdir -p "$work/p3/python/Lib/site-packages"
cp -r "$work/deps/." "$work/p3/python/Lib/site-packages/"
cp "$dl/ytdlp-20261026/yt-dlp" "$work/p3/yt-dlp"
(cd "$work/p3" && bsdtar -a -cf "$st/p3.zip" python yt-dlp)

# p4: the same Python with its library, the wheels and yt-dlp as bytecode in a few zip files.
"$base/pylinux/python/bin/python3.11" -I "$here/pack-python.py" "$work/p3/python" "$work/deps" \
    "$dl/ytdlp-20261026/yt-dlp" "$work/p4" >/dev/null
(cd "$work/p4" && bsdtar -a -cf "$st/p4.zip" python)

mkdir -p "$st/qjs" "$st/qjs-ng" "$st/ffmpeg"
cp "$base/qjs/qjs.exe" "$st/qjs/qjs.exe"
cp "$base/qjs/qjs-ng.exe" "$st/qjs-ng/qjs.exe"
bsdtar -xf "$dl/ffmpeg-8.1.1/ffmpeg-8.1.1-essentials_build.zip" -C "$work" '*/bin/ffmpeg.exe' '*/bin/ffprobe.exe'
cp "$work"/ffmpeg-8.1.1-essentials_build/bin/ff{mpeg,probe}.exe "$st/ffmpeg/"

# PowerShell 3 reads a script without a BOM in the ANSI code page.
{ printf '\xef\xbb\xbf'; cat "$here/m0.ps1"; } > "$st/m0.ps1"

rm -f "$iso"
xorriso -as mkisofs -quiet -J -joliet-long -R -V PGM0 -o "$iso" "$st"
ls -la "$iso"
