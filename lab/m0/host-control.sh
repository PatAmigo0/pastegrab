#!/usr/bin/env bash
# host-control.sh [olympics|youtube|archive]: the same yt-dlp release from this machine, through the
# network the VMs share, which separates a Windows 8 failure from a network one.
# needs: date
set -uo pipefail
base=${M0_DIR:-/mnt/storage/pastegrab-m0}
y=$base/dl/ytdlp-20261026/yt-dlp_linux
qjs=$base/qjs/qjs-linux
out=$base/host
mkdir -p "$out/cache"
yd() { "$y" --ignore-config --cache-dir "$out/cache" "$@"; }
timed() {
    local t0 rc
    t0=$(date +%s%N)
    "$@"
    rc=$?
    echo "elapsed $(( ($(date +%s%N) - t0) / 1000000 )) ms, rc=$rc"
}

case ${1:-all} in
olympics | all)
    echo "== olympics.com, impersonating chrome"
    timed yd --impersonate chrome --simulate --print '%(title)s | %(duration)s s' \
        'https://olympics.com/ru/video/slavomir-knazovicky-canoe-sprint-c1-500m-silver-medal-atlanta-1996' 2>&1 | tail -3
    ;;&
youtube | all)
    echo "== YouTube jNQXAC9IVRw with QuickJS"
    rm -rf "$out/cache"
    timed yd --no-js-runtimes --js-runtimes "quickjs:$qjs" --simulate \
        --print '%(title)s | %(formats.:.format_id)s' 'https://www.youtube.com/watch?v=jNQXAC9IVRw' 2>&1 | tail -3
    ;;&
archive | all)
    echo "== archive.org MPEG-1 original"
    timed yd --simulate --print '%(title)s | %(formats.:.format_id)s' \
        'https://archive.org/details/Election_Ads/Commercial-Nixon1960ElectionAdToughonDefense.mpg' 2>&1 | tail -3
    ;;
esac
