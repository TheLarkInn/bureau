# Agent Instructions

Before you finish any change, run all five gates and fix every finding:

```sh
cargo fmt --all
bash scripts/check-rust-policy.sh
cargo clippy --workspace --all-targets --all-features
cargo test --offline
LIBRARY_PATH="$(rustc +nightly-2026-01-22 --print sysroot)/lib" DYLINT_RUSTFLAGS="-Dwarnings -A module-dependencies-dead-edge" cargo dylint --all
```

The last gate — the custom dylint libraries (`li-kai/rust-lints`, pinned in
`[workspace.metadata.dylint]`) — is arguably the most important and the
easiest to forget: it does not run under plain `cargo clippy`. Never skip
it. If the pinned nightly is missing: `rustup toolchain install
nightly-2026-01-22 --component rustc-dev`.

The `-A module-dependencies-dead-edge` carve-out is deliberate: the
dead-edge check runs per crate, so in this lib+bin package every
lib-internal edge reads as dead in the bin crate. The allowlist in
`dylint.toml` stays fully enforced. Adding a module edge means editing
`dylint.toml` — that edit is the architectural decision reviewers check.
The flags ride in `DYLINT_RUSTFLAGS`, not `RUSTFLAGS`: `RUSTFLAGS` also
applies when building the dylint driver itself, where custom lint names
are not yet registered.

## The spec

`DESIGN.md` is the authoritative specification. Read it before writing
anything. Three rules are enforced in review:

1. Naming law (§2): no invented nouns, no mascots, no Kubernetes
   vocabulary.
2. Non-goals (§3) are hard: do not build anything on that list.
3. Every layer ships with offline tests: no network, no model calls.

All layers are built: process contract, fake adapter, step contract, run
log, config loader, engine, durable state, git worktrees, forge clients,
reconcile loop.

## Hard limits (deny-level; CI fails otherwise)

| Limit | Value | Tool |
|---|---|---|
| Function length | 25 lines | clippy `too_many_lines` |
| Cognitive complexity | 4 — each `assert!` and `.await` costs 1 | clippy `cognitive_complexity` |
| File length | 300 lines | `scripts/check-rust-policy.sh` |
| `#[allow]`/`#[expect]` in source | zero | policy script + clippy `allow_attributes` |
| `unsafe` | zero | `unsafe_code = "forbid"` |
| Compiler warnings | zero | `warnings = "deny"` |

Clippy groups at deny: `all`, `cargo`, `complexity`, `correctness`,
`nursery`, `pedantic`, `perf`, `style`, `suspicious`.

Two deliberate lint adjustments (root `Cargo.toml` comments carry the
reasons): `allow_attributes` is `deny`, not `forbid` — `forbid` breaks
spec-approved derive macros (clap); `multiple_crate_versions` is
`allow` — reqwest's own tree requires both syn 2 and syn 3.

## Writing tests under these limits

A test function fits at most ~3 assert-family calls. So:

1. Table-drive: one assert inside a loop over cases.
2. Or aggregate values into a tuple/`Vec` and assert once.
3. `.expect()` and `?` are free — use them for setup.

Pattern to copy: `crates/bureau/tests/process_contract.rs`.
