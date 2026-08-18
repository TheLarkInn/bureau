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

node --test .github/extensions/bureau-canvas/test/*.test.mjs
./scripts/check-rust-policy.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

toolchain=nightly-2026-01-22
LIBRARY_PATH="$(rustc +"$toolchain" --print sysroot)/lib" \
    RUSTFLAGS="-Dwarnings" \
    cargo dylint --all
