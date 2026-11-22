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

# Lints the Windows-only code in the toolchain container.
clippy-windows:
    podman build -q -t pastegrab-rust-build tools/build/rust >/dev/null
    podman run --rm -v "$PWD:/src" -v pastegrab-cargo-registry:/opt/cargo/registry \
        -v pastegrab-cargo-git:/opt/cargo/git -e CARGO_TARGET_DIR=/src/target/win7 -w /src \
        pastegrab-rust-build cargo clippy -q -Zbuild-std=std,panic_unwind \
        --target x86_64-win7-windows-gnu --workspace --all-targets -- -D warnings

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
