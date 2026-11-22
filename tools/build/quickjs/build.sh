#!/usr/bin/env bash
# Builds qjs.exe from Bellard's QuickJS and qjs-ng.exe from QuickJS-NG with mingw-w64 in podman.
# Usage: build.sh OUT_DIR
set -euo pipefail

QJS_VERSION=2026-06-04
QJS_SHA256=b376e839b322978313d929fd20663b11ba58b75df5a46c126dd19ea2fa70ad2a
QJS_NG_VERSION=0.18.0
QJS_NG_SHA256=28bcd89aa3683fd2a946537e5ce3f11cc46b5cbf3383c7561aaef3df5ff9f1dd

here=$(cd "$(dirname "$0")" && pwd)
out=$(mkdir -p "$1" && cd "$1" && pwd)
cache=${XDG_CACHE_HOME:-$HOME/.cache}/pastegrab/src
mkdir -p "$cache"

fetch() {
    local url=$1 file=$2 sum=$3
    if [ ! -f "$cache/$file" ]; then
        curl -fsSL -o "$cache/$file.part" "$url"
        mv "$cache/$file.part" "$cache/$file"
    fi
    echo "$sum  $cache/$file" | sha256sum -c --quiet
}
fetch "https://bellard.org/quickjs/quickjs-$QJS_VERSION.tar.xz" "quickjs-$QJS_VERSION.tar.xz" "$QJS_SHA256"
fetch "https://github.com/quickjs-ng/quickjs/archive/refs/tags/v$QJS_NG_VERSION.tar.gz" "quickjs-ng-$QJS_NG_VERSION.tar.gz" "$QJS_NG_SHA256"

image=pastegrab-quickjs-build
podman build -q -t "$image" "$here" >/dev/null
podman run --rm \
    -e QJS_VERSION="$QJS_VERSION" -e QJS_NG_VERSION="$QJS_NG_VERSION" \
    -v "$cache:/src:ro" -v "$out:/out" -v "$here/inner.sh:/inner.sh:ro" -v "$here:/res:ro" \
    "$image" bash /inner.sh
