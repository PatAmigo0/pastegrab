#!/usr/bin/env bash
# fetch-redist.sh: puts Microsoft's VC++ redistributable, which installs the UCRT where Windows
# lacks it, in target/redist/ and checks it against the pinned hash; prints its path.
# needs: curl sha256sum
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
url=https://aka.ms/vs/17/release/vc_redist.x64.exe
# 14.44.35211.0. When Microsoft replaces the file, this fails until the pin moves on purpose.
sha256=cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b
file=$root/target/redist/vc_redist.x64.exe
mkdir -p "$(dirname "$file")"
if ! echo "$sha256  $file" | sha256sum -c --quiet >/dev/null 2>&1; then
    curl -sSL --fail -o "$file.part" "$url"
    mv "$file.part" "$file"
    echo "$sha256  $file" | sha256sum -c --quiet
fi
echo "$file"
