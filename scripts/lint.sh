#!/usr/bin/env bash
# Every lint gate CI enforces, in one command: canvas extension tests,
# repository policy, formatting, clippy, and the dylint custom lints
# (lints/rust-lints, configured by dylint.toml). Mirrors
# .github/workflows/rust-lints.yml — CI runs this same script, so the two
# never drift.
#
# Requires: cargo-dylint, dylint-link, and the nightly-2026-01-22 toolchain
# with the rustc-dev component:
#
#   cargo install cargo-dylint --version 5.0.0 --locked
#   cargo install dylint-link --version 5.0.0 --locked
#   rustup toolchain install nightly-2026-01-22 --component rustc-dev
set -euo pipefail

# `node --test` exits 0 even when tests fail on the Node in use here, so the
# result is read from TAP rather than from the exit code — otherwise this gate
# passes silently while tests are red. Also fails when nothing ran, which is
# what a mistyped glob looks like.
canvas_tests() {
    local output
    output="$(node --test --test-reporter=tap .github/extensions/bureau-canvas/test/*.test.mjs)" || true
    printf '%s\n' "$output"
    if grep -qE '^not ok ' <<<"$output"; then
        echo "canvas tests failed" >&2
        return 1
    fi
    if ! grep -qE '^ok 1 ' <<<"$output"; then
        echo "canvas tests did not run" >&2
        return 1
    fi
}

canvas_tests
./scripts/check-rust-policy.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

toolchain=nightly-2026-01-22
LIBRARY_PATH="$(rustc +"$toolchain" --print sysroot)/lib" \
    RUSTFLAGS="-Dwarnings" \
    cargo dylint --all
