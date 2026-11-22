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
