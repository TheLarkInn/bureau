# Agent Instructions

## ⛔ ENFORCED RULES — READ BEFORE ANY CODE CHANGE ⛔

These rules are **deny-level**: CI fails and the change is rejected if any is
violated. They are enforced by `bash scripts/lint.sh` (repository policy +
`cargo fmt --check` + clippy + the vendored dylint lints in `lints/rust-lints`,
configured by `dylint.toml`).

**Before finishing ANY change, run all gates and fix every finding:**

```sh
cargo fmt --all
bash scripts/lint.sh
cargo test --offline
```

### Hard limits (deny-level)

| Limit | Value | Enforced by |
|---|---|---|
| Function length | 25 lines | clippy `too_many_lines` |
| Cognitive complexity | 4 — each `assert!` and `.await` costs 1 | clippy `cognitive_complexity` |
| File length | 300 lines | `scripts/check-rust-policy.sh` |
| `#[allow]`/`#[expect]` in source | **zero** | policy script + clippy `allow_attributes` |
| `unsafe` | **zero** | `unsafe_code = "forbid"` |
| Compiler warnings | **zero** | `warnings = "deny"` |

Clippy groups at deny: `all`, `cargo`, `complexity`, `correctness`, `nursery`,
`pedantic`, `perf`, `style`, `suspicious`.

Two deliberate lint adjustments (root `Cargo.toml` comments carry the reasons):
`allow_attributes` is `deny`, not `forbid` — `forbid` breaks spec-approved
derive macros (clap); `multiple_crate_versions` is `allow` — reqwest's own tree
requires both syn 2 and syn 3.

### dylint module architecture (dylint.toml) — MANDATORY

`dylint.toml` is the workspace module architecture and is **exhaustive**:

- Every top-level module of every crate must be listed under
  `[module_dependencies.allow]`.
- Every cross-module dependency must be declared there as a
  `"<crate>:<module>" = [...]` edge.
- **Adding a module or a cross-module `use` means updating `dylint.toml` in
  the same change — the lint fails otherwise.** The lint flags both undeclared
  edges and, within a crate, declared edges no longer present in code.
- Never add a dependency edge that is not already declared without editing
  `dylint.toml` in the same commit.

### Spec rules (DESIGN.md) — enforced in review

1. Naming law (§2): no invented nouns, no mascots, no Kubernetes vocabulary.
2. Non-goals (§3) are hard: do not build anything on that list.
3. Every layer ships with offline tests: no network, no model calls.

### Writing tests under these limits

A test function fits at most ~3 assert-family calls. So:

1. Table-drive: one assert inside a loop over cases.
2. Or aggregate values into a tuple/`Vec` and assert once.
3. `.expect()` and `?` are free — use them for setup.

Pattern to copy: `crates/bureau/tests/process_contract.rs`.

---

Before you finish any change, run all gates and fix every finding:

```sh
cargo fmt --all
bash scripts/lint.sh
cargo test --offline
```

`scripts/lint.sh` is the single lint entry point: repository policy,
`cargo fmt --check`, clippy, and the dylint custom lints
(`lints/rust-lints`, configured by `dylint.toml`) — the same command CI
runs. It needs `cargo-dylint`, `dylint-link`, and the
`nightly-2026-01-22` toolchain with `rustc-dev` (install lines are in the
script header).

`dylint.toml` is the workspace module architecture: every top-level module
must be listed under `[module_dependencies.allow]`, and every cross-module
dependency must be declared there. Adding a module or a cross-module `use`
means updating `dylint.toml` in the same change — the lint fails otherwise.

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
