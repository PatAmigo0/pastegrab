#!/usr/bin/env bash
# Builds a crate for a win7 GNU target in the pinned toolchain container.
# Usage: [PASTEGRAB_PROFILE=size] build.sh CRATE_DIR TARGET [CARGO_ARGS...]; the profile defaults to release.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
crate=$(cd "$1" && pwd)
target=$2
shift 2

image=pastegrab-rust-build
podman build -q -t "$image" "$here" >/dev/null
# Named volumes keep the crate index and sources between runs.
# The setup's payload comes in through these, when set.
forward=()
for name in PASTEGRAB_PAYLOAD PASTEGRAB_PAYLOAD_SIZE; do
    if [ -n "${!name:-}" ]; then forward+=(-e "$name=${!name}"); fi
done
# A target folder of its own keeps host and container builds from rebuilding each other.
podman run --rm "${forward[@]}" \
    -v "$crate:/src" -v pastegrab-cargo-registry:/opt/cargo/registry -v pastegrab-cargo-git:/opt/cargo/git \
    -e CARGO_TARGET_DIR=/src/target/win7 -w /src "$image" \
    cargo build --profile "${PASTEGRAB_PROFILE:-release}" -Zbuild-std=std,panic_unwind --target "$target" "$@"
