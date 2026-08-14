# Agent Instructions

## Linting expectations

This repository is a Rust workspace with strict lint configuration. All code must pass these lints before being considered complete.

### Rust lints (from `[workspace.lints.rust]` in root `Cargo.toml`)

- `unsafe_code = "forbid"` — never use `unsafe` blocks.
- `warnings = "deny"` — all compiler warnings are errors; fix them, don't ignore them.

### Clippy lints (from `[workspace.lints.clippy]`)

All of the following Clippy lint groups are set to **deny**:

- `all` (priority -1)
- `cargo`
- `complexity`
- `correctness`
- `nursery`
- `pedantic`
- `perf`
- `style`
- `suspicious`

Additionally:

- `allow_attributes = "forbid"` — do not use `#[allow(...)]` to suppress lints.
- `cognitive_complexity = "deny"` — keep functions simple; break up complex logic.
- `too_many_lines = "deny"` — keep functions short; extract helpers when needed.

### Additional tooling

- Root `clippy.toml` and `dylint.toml` exist — dylint runs extra lint libraries (see `[workspace.metadata.dylint]` in root `Cargo.toml`).

### How to verify

Run clippy across the workspace and fix every finding:

```sh
cargo clippy --workspace --all-targets
```

Do not add `#[allow]` attributes or otherwise suppress lints; refactor the code to satisfy them instead.
