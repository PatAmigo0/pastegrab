# Recipes for this machine only; git ignores local/.
import? 'local/local.just'

# What CI runs, cheapest failure first.
check: fmt-check clippy test

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# Lints the Windows-only code in the toolchain container: everything for 64-bit, and the
# 32-bit setup.
clippy-windows:
    podman build -q -t pastegrab-rust-build tools/build/rust >/dev/null
    podman run --rm -v "$PWD:/src" -v pastegrab-cargo-registry:/opt/cargo/registry \
        -v pastegrab-cargo-git:/opt/cargo/git -e CARGO_TARGET_DIR=/src/target/win7 -w /src \
        pastegrab-rust-build sh -c 'cargo clippy -q -Zbuild-std=std,panic_unwind \
        --target x86_64-win7-windows-gnu --workspace --all-targets -- -D warnings && \
        cargo clippy -q -Zbuild-std=std,panic_unwind --target i686-win7-windows-gnu \
        -p pastegrab-setup --all-targets -- -D warnings'

# Renders the exe icon from its SVG, each size a PNG inside the .ico; needs rsvg-convert.
icon:
    #!/usr/bin/env bash
    set -euo pipefail
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    for n in 16 24 32 48 256; do rsvg-convert -w $n -h $n crates/winres/pastegrab.svg -o "$tmp/$n.png"; done
    python3 - "$tmp" crates/winres/pastegrab.ico <<'PY'
    import struct, sys
    sizes = [16, 24, 32, 48, 256]
    images = [open(f"{sys.argv[1]}/{n}.png", "rb").read() for n in sizes]
    offset = 6 + 16 * len(images)
    out = struct.pack("<HHH", 0, 1, len(images))
    for n, png in zip(sizes, images):
        out += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(png), offset)
        offset += len(png)
    open(sys.argv[2], "wb").write(out + b"".join(images))
    PY

# Builds every Windows file of a release into target/dist/<version>/; TOOLS is the unpacked tools.
package TOOLS:
    tools/release/package.sh {{TOOLS}}

# Signs a packaged release's update.json with the release key; minisign asks for its password.
sign VERSION:
    minisign -Sm target/dist/{{VERSION}}/update.json -s ~/.minisign/minisign.key

# Publishes a signed release on GitHub, where every install's update check finds it; with
# --prerelease only a PC told its address finds it.
publish VERSION *FLAGS:
    gh release create v{{VERSION}} target/dist/{{VERSION}}/* --title "pastegrab {{VERSION}}" --notes "" {{FLAGS}}
