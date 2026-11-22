#!/usr/bin/env bash
# package.sh TOOLS_DIR: builds the launcher, the window and pgcli for Windows, the setup around them,
# and the app update package with its unsigned update.json, all in target/dist/<version>/.
# needs: cargo tar zstd sha256sum jq podman
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
tools=$1
pkgid=$(cd "$root" && cargo pkgid -p pastegrab-window)
version=${pkgid##*@}
win=$root/target/win7/x86_64-win7-windows-gnu
out=$root/target/dist/$version
"$root/tools/build/rust/build.sh" "$root" x86_64-win7-windows-gnu -p pastegrab-window -p pgcli
PASTEGRAB_PROFILE=size "$root/tools/build/rust/build.sh" "$root" x86_64-win7-windows-gnu -p pastegrab-launcher
"$root/tools/release/build-setup.sh" "$tools" "$win/size/pastegrab.exe" \
    "$win/release/pastegrab-app.exe" "$win/release/pgcli.exe"
rm -rf "$out"
mkdir -p "$out"
cp "$root/target/win7/i686-win7-windows-gnu/release/pastegrab-setup.exe" "$out/"
# The package unpacks into app\<version>\, so its files sit at the top of the tar.
package=pastegrab-app-$version-x86_64.tar.zst
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 --format=gnu \
    -C "$win/release" -cf - pastegrab-app.exe pgcli.exe | zstd -q -19 -o "$out/$package"
jq -n --arg version "$version" \
    --argjson expires "$(($(date +%s) + 365 * 24 * 3600))" \
    --arg url "https://github.com/PatAmigo0/pastegrab/releases/download/v$version/$package" \
    --arg sha256 "$(sha256sum "$out/$package" | cut -d' ' -f1)" \
    --argjson size "$(stat -c %s "$out/$package")" \
    '{version: $version, expires: $expires,
      packages: [{target: "x86_64-windows", url: $url, sha256: $sha256, size: $size}]}' \
    >"$out/update.json"
ls -l "$out"
echo "next: just sign $version, then just publish $version"
