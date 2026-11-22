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
