#!/usr/bin/env bash
# Downloads every M0 candidate into its own folder under $M0_DIR/dl and checks each hash.
# needs: gh curl python3 sha256sum
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dl=${M0_DIR:-/mnt/storage/pastegrab-m0}/dl
mkdir -p "$dl"

gh_rel() { # REPO TAG DIR ASSET...
    local repo=$1 tag=$2 dir=$3
    shift 3
    mkdir -p "$dl/$dir"
    for a in "$@"; do
        [ -f "$dl/$dir/$a" ] || gh release download "$tag" -R "$repo" -p "$a" -D "$dl/$dir" --clobber
    done
}

echo "== yt-dlp 2026.10.21, the last official build for Windows 8"
gh_rel yt-dlp/yt-dlp 2026.10.21 ytdlp-20261021 yt-dlp_win.zip yt-dlp.exe SHA2-256SUMS
(cd "$dl/ytdlp-20261021" && grep -E ' (yt-dlp_win\.zip|yt-dlp\.exe)$' SHA2-256SUMS | sha256sum -c)

echo "== yt-dlp 2026.10.26, the pure-Python release and the Linux control"
gh_rel yt-dlp/yt-dlp 2026.10.26 ytdlp-20261026 yt-dlp yt-dlp_linux SHA2-256SUMS
(cd "$dl/ytdlp-20261026" && grep -E ' (yt-dlp|yt-dlp_linux)$' SHA2-256SUMS | sha256sum -c)

echo "== python-build-standalone 3.11.18"
pbs=cpython-3.11.18+20261110-x86_64-pc-windows-msvc-install_only_stripped.tar.gz
gh_rel astral-sh/python-build-standalone 20261110 pbs-3.11.18 "$pbs" SHA256SUMS
(cd "$dl/pbs-3.11.18" && grep -F " $pbs" SHA256SUMS | sha256sum -c)

echo "== the same Python for Linux, which compiles the Windows build's bytecode"
pbsl=cpython-3.11.18+20261110-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz
gh_rel astral-sh/python-build-standalone 20261110 pbs-3.11.18-linux "$pbsl"
(cd "$dl/pbs-3.11.18" && grep -F " $pbsl" SHA256SUMS | sed "s#  #  ../pbs-3.11.18-linux/#" | sha256sum -c)
[ -x "${M0_DIR:-/mnt/storage/pastegrab-m0}/pylinux/python/bin/python3.11" ] || {
    mkdir -p "${M0_DIR:-/mnt/storage/pastegrab-m0}/pylinux"
    tar -xzf "$dl/pbs-3.11.18-linux/$pbsl" -C "${M0_DIR:-/mnt/storage/pastegrab-m0}/pylinux"
}

echo "== python.org 3.11.9 embeddable"
mkdir -p "$dl/python-3.11.9"
embed=$dl/python-3.11.9/python-3.11.9-embed-amd64.zip
[ -f "$embed" ] || curl -fsSL -o "$embed" https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip
echo "009d6bf7e3b2ddca3d784fa09f90fe54336d5b60f0e0f305c37f400bf83cfd3b  $embed" | sha256sum -c

echo "== wheels, pinned as in yt-dlp 2026.10.26"
mkdir -p "$dl/wheels"
if [ -z "$(ls -A "$dl/wheels")" ]; then
    python3 -I "$here/wheels.py" "$dl/wheels" \
        brotli==1.2.1 certifi==2026.10.23 charset-normalizer==3.5.2 idna==3.20 mutagen==1.48.1 \
        pycryptodomex==3.24.1 requests==2.34.2 urllib3==2.8.0 websockets==17.3 yt-dlp-ejs==0.8.0 \
        cffi==2.1.1 curl-cffi==0.17.0 pycparser==3.11
fi

echo "== gyan ffmpeg 8.1.1 essentials"
gh_rel GyanD/codexffmpeg 8.1.1 ffmpeg-8.1.1 ffmpeg-8.1.1-essentials_build.zip
echo "6f58ce889f59c311410f7d2b18895b33c03456463486f3b1ebc93d97a0f54541  $dl/ffmpeg-8.1.1/ffmpeg-8.1.1-essentials_build.zip" | sha256sum -c
