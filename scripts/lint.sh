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

# `node --test` exits 0 even when tests fail on the Node in use here, so each
# result is read from TAP rather than from the exit code — otherwise this gate
# passes silently while tests are red. Also fails when nothing ran, which is
# what a mistyped glob looks like.
node_tests() {
    local label="$1"
    shift
    local output
    output="$(node --test --test-reporter=tap "$@")" || true
    printf '%s\n' "$output"
    if grep -qE '^not ok ' <<<"$output"; then
        echo "$label failed" >&2
        return 1
    fi
    if ! grep -qE '^ok 1 ' <<<"$output"; then
        echo "$label did not run" >&2
        return 1
    fi
}

# Browser tests for the assignment card controls. Skipped with a notice when
# the browser is not installed, so a fresh clone still runs every other gate;
# CI installs it, so there the suite always runs.
canvas_browser_tests() {
    local dir=.github/extensions/bureau-canvas/e2e/playwright
    if [ ! -d "$dir/node_modules" ]; then
        echo "skipping canvas browser tests: run 'npm ci && npx playwright install --with-deps chromium' in $dir" >&2
        return 0
    fi
    (cd "$dir" && npm run test:pr)
}

node_tests "canvas tests" .github/extensions/bureau-canvas/test/*.test.mjs
node_tests "script tests" scripts/*.test.mjs
canvas_browser_tests
bash ./scripts/check-rust-policy.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

toolchain=nightly-2026-01-22
LIBRARY_PATH="$(rustc +"$toolchain" --print sysroot)/lib" \
    RUSTFLAGS="-Dwarnings" \
    cargo dylint --all
