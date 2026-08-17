# rust-lints

Custom Rust lints via the [dylint](https://github.com/trailofbits/dylint) ecosystem.

## Lints

| Lint | Level | Description |
|------|-------|-------------|
| [`acyclic_modules`](#acyclic_modules) | deny | Cyclic dependencies between sibling modules at any depth |
| [`await_holding_unsendable`](#await_holding_unsendable) | deny | Guards, pool connections, and span handles held across `.await` points |
| [`blocking_in_async`](#blocking_in_async) | deny | Blocking operations inside `async fn` or `async {}` blocks |
| [`debug_remnants`](#debug_remnants) | warn | Debug macros (`println!`, `eprintln!`, `dbg!`) in non-test code |
| [`fallible_new`](#fallible_new) | deny | `fn new()` constructors that can panic |
| [`global_side_effect::env`](#global_side_effect) | warn | Direct calls to `std::env::var` and similar outside `main()` |
| [`global_side_effect::logging_init`](#global_side_effect) | deny | Global tracing subscriber initialization outside `main()` |
| [`global_side_effect::randomness`](#global_side_effect) | warn | Direct calls to random number generators outside `main()` and tests |
| [`global_side_effect::time`](#global_side_effect) | warn | Direct calls to wall-clock or monotonic time outside `main()` and tests |
| [`map_init_then_insert`](#map_init_then_insert) | warn | `HashMap`/`BTreeMap`/`IndexMap` created empty then immediately populated with `insert()` |
| [`module_dependencies`](#module_dependencies) | deny | Cross-module dependencies not declared in the allowlist |
| [`needless_builder`](#needless_builder) | warn | Structs with ≤ 2 named fields that unnecessarily derive `bon::Builder` |
| [`panic_in_drop`](#panic_in_drop) | deny | Panic-able expressions inside `Drop` implementations |
| [`proper_error_type`](#proper_error_type) | warn | Incomplete or unstructured error types in `pub`/`pub(crate)` APIs |
| [`realtime_in_async_test`](#realtime_in_async_test) | warn | Tokio time calls in async tests without `start_paused = true` |
| [`result_result`](#result_result) | warn | Nested `Result<Result<T, E1>, E2>` in function signatures |
| [`suggest_builder`](#suggest_builder) | warn | Structs with ≥ 6 named fields that could use a `#[builder]` constructor |
| [`topological_ordering`](#topological_ordering) | warn | Items within a module not ordered by their dependency graph |
| [`unbounded_channel`](#unbounded_channel) | deny | Creation of unbounded channels that can exhaust memory |
| [`unclear_exports`](#unclear_exports) | deny | Glob imports (`use foo::*`) and renamed imports (`use foo::Bar as Baz`) |
| [`unsafe_send_missing_drop`](#unsafe_send_missing_drop) | warn | `unsafe impl Send` on types with `!Send` fields and no `Drop` impl |
| [`unstructured_log_fields`](#unstructured_log_fields) | warn | `tracing` macros using format args instead of structured fields |

---

### `acyclic_modules`

Flags cyclic dependencies between sibling modules at any depth of the module hierarchy. Builds a sibling dependency graph at every level and reports any cycle it finds.

```
error: cyclic dependency between sibling modules under `crate`:
       `payments` → `server` → `payments`
  --> src/payments/checkout.rs:5:5
   |
5  |     use crate::server::auth::verify;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `payments` → `server`
   |
  ::: src/server/auth.rs:12:9
   |
12 |     crate::payments::billing::create_invoice();
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `server` → `payments`
   |
   = help: break this cycle by moving shared items to a module that both
           `payments` and `server` can depend on, or restructure so the
           dependency flows in one direction
```

Tracks path expressions, use statements, type annotations, and method calls. Parent-child references are excluded by construction (only siblings are compared). Does not fire inside `#[cfg(test)]` code or on macro-expanded spans.

No configuration required. Use `#[expect(acyclic_modules, reason = "...")]` for per-site opt-out. Complementary to `module_dependencies` — see [docs/acyclic-modules.md](docs/acyclic-modules.md) for the full design.

### `await_holding_unsendable`

Flags guards, pool connections, and span handles held across `.await` points. Complements Clippy's `await_holding_lock` and `await_holding_refcell_ref` by covering types those lints don't know about.

```
error: `Entered` held across `.await` — corrupted span nesting — events on other tasks attributed to wrong span
  --> src/handler.rs:20:9
   |
20 |     let _entered = span.enter();
   |         ^^^^^^^^
   |
   = help: scope the guard so it is dropped before the `.await`, or use an async-aware alternative
note: the value is held across these await points
  --> src/handler.rs:21:15
   |
21 |     do_work().await;
   |               ^^^^^
```

Flagged by default: `parking_lot` mutex and rwlock guards (including `Mapped*` and `Arc*` variants), `tracing::span::Entered` / `EnteredSpan`, `crossbeam_epoch::Guard`, `rusqlite::Transaction` / `Savepoint`, `r2d2::PooledConnection`, `diesel::r2d2::PooledConnection`.

Does not fire inside `#[test]` / `#[tokio::test]` or `#[cfg(test)]` modules. Extend via `additional_types` in `dylint.toml`, or disable the defaults with `skip_default_types = true`.

### `blocking_in_async`

Flags known-blocking operations inside `async fn` or `async {}` blocks. Suggests using async-aware alternatives or `spawn_blocking` instead.

```
warning: blocking call to `std::fs::read_to_string()` inside async function
  --> src/loader.rs:12:13
   |
12 |     let data = std::fs::read_to_string(path)?;
   |               ^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: use `tokio::fs::read_to_string()` instead
           or wrap the blocking call in `tokio::task::spawn_blocking()`
```

Flagged by default: `std::fs::read/write/read_dir/metadata/canonicalize`, `std::io::stdin().read*`, `std::net::TcpStream::connect`, `std::thread::sleep`, `std::thread::spawn`, `std::sync::Mutex::lock`, `std::sync::RwLock::read/write`, `parking_lot::Mutex::lock`, `parking_lot::RwLock::read/write`, `tokio::task::block_in_place`.

Does not fire inside `#[test]` / `#[tokio::test]` or `tokio::task::spawn_blocking`.

### `debug_remnants`

Flags `println!`, `print!`, `eprintln!`, and `dbg!` outside test code. Suggests structured logging replacements (`tracing` or `log`).

```
warning: debug remnant in committed code
  --> src/api.rs:42:5
   |
42 |     println!("request: {:?}", req);
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: replace with `tracing::info!(?req, "incoming request")`
```

Does not fire inside `#[test]` functions or `#[cfg(test)]` modules. Supersedes `clippy::dbg_macro`, `clippy::print_stdout`, and `clippy::print_stderr` with actionable replacements and unified configuration.

### `fallible_new`

Warns when a `fn new()` constructor contains `.unwrap()`, `.expect()`, `panic!`, or `unreachable!`. These can abort the program in cases the caller cannot handle.

```
warning: constructor `new` can panic — consider returning `Result` or renaming to `try_new`
  --> src/config.rs:8:5
   |
 8 |     pub fn new(path: &str) -> Self {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: `.unwrap()` at src/config.rs:9:55 can panic — use `?` with a `Result` return type instead
```

Does not fire when the return type is already `Result` or inside trait impls. For private constructors that intentionally panic on invariant violations, use `#[expect(fallible_new)]`.

### `global_side_effect`

Four lints that flag direct calls to non-deterministic or environment-coupled functions. The fix for `time`, `randomness`, and `env` is to accept the dependency as a parameter. `logging_init` is `deny` by default because it mutates process-global state; the fix is to move initialization to `main()`.

```
warning[global_side_effect.time]: direct call to `chrono::Utc::now()`
  --> src/billing.rs:42:15
   |
42 |     let now = Utc::now();
   |               ^^^^^^^^^^
   |
   = help: accept a time parameter or use a clock trait so callers can
           control the time source in tests
```

None of the four lints fire inside `#[test]` functions, `#[cfg(test)]` modules, or `fn main()`.

**`global_side_effect::time`** — flags: `std::time::SystemTime::now`, `std::time::Instant::now`, `chrono::Utc::now`, `chrono::Local::now`, `time::OffsetDateTime::now_utc`, `jiff::Zoned::now`, `tokio::time::Instant::now`, and more.

**`global_side_effect::randomness`** — flags: `rand::thread_rng`, `rand::random`, `rand::rngs::OsRng::new`, `fastrand::Rng::new`, and more.

**`global_side_effect::env`** — flags: `std::env::var`, `std::env::vars`, `std::env::args`, `dotenvy::var`, `dotenvy::vars`, `dotenv::var`.

**`global_side_effect::logging_init`** — `deny` by default; flags: `tracing_subscriber::fmt::init`, `tracing_subscriber::fmt::try_init`, `tracing_subscriber::fmt::SubscriberBuilder::{init, try_init}`, `tracing_subscriber::util::SubscriberInitExt::{init, try_init}`, and `tracing::subscriber::set_global_default`.

### `map_init_then_insert`

Warns when a `HashMap`, `BTreeMap`, or `IndexMap` is created empty and then immediately populated with two or more sequential `.insert()` calls. Suggests `::from([...])` instead.

```
warning: immediately inserting into a newly created map — consider using `HashMap::from([..])`
  --> src/config.rs:12:5
   |
12 | /   let mut m = HashMap::new();
13 | |   m.insert("a", 1);
14 | |   m.insert("b", 2);
15 | |   m.insert("c", 3);
   | |________________________^
   |
   = help: use `let m = HashMap::from([..])` to initialize the map inline
```

Does not fire when there is intervening control flow, reads, or borrows between creation and the insert sequence, or when there is only one insert. Complements Clippy's `vec_init_then_push`.

### `module_dependencies`

Enforces an allowlist of permitted cross-module dependencies within a crate. Each top-level module declares which other top-level modules it may depend on. Any undeclared dependency is a compile-time error.

```
error[module_dependencies]: `payments` depends on `server`, which is not in its allowlist
  --> src/payments/checkout.rs:12:5
   |
12 |     use crate::server::SessionInfo;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: if this dependency is intentional, add "server" to the
           `payments` allowlist in module_dependencies.toml
   = help: if not, move `SessionInfo` to a module that `payments`
           is allowed to depend on (currently: types, errors, utils)
```

Configuration is via a `[module_dependencies]` section declaring the allowed dependency edges per module. In exhaustive mode (default), every top-level module must appear in the config. Dead edges (declared but unused dependencies) produce a warning. Does not fire inside `#[cfg(test)]` code.

### `needless_builder`

Warns when `bon::Builder` is derived on a struct with very few fields.

```
warning: struct `Point` has only 2 fields; `bon::Builder` may be unnecessary
  --> src/lib.rs:5:1
   = help: consider using a plain constructor or struct literal instead
```

### `panic_in_drop`

Flags `.unwrap()`, `.expect()`, `panic!`, `unreachable!`, `assert!`, `assert_eq!`, and `assert_ne!` inside `Drop` implementations. Panicking during unwinding causes an immediate process abort with no cleanup.

```
warning: `.unwrap()` in `Drop` impl — this will abort if called during unwinding
  --> src/tempfile.rs:12:9
   |
12 |         std::fs::remove_file(&self.path).unwrap();
   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: handle the error with `if let Err(e) = ...` or ignore it with `let _ = ...`
   = note: panicking in `drop()` while already unwinding causes an immediate process abort
```

Does not fire on macro-generated `Drop` impls or inside `if !std::thread::panicking()` guards.

### `proper_error_type`

Flags error types exposed in `pub` or `pub(crate)` APIs that are incomplete, unstructured, or missing error chain information. Fires in five cases:

1. Functions returning `Result<T, String>`, `Result<T, &str>`, `Result<T, Box<dyn Error>>`, or `anyhow::Error`/`miette::Report` on effectively-public surfaces.
2. Manual `impl Error` blocks missing `source()` when the type wraps other errors.
3. `Display` impls that render an inner error also returned by `source()` (double-printing).
4. Types with both manual `impl Display` and `impl Error` (use `thiserror` instead).
5. Types named `*Error` or `*Err` that don't implement `std::error::Error`.

```
warning: public function returns `Result<_, String>` — use a type that implements `Error`
  --> src/config.rs:5:40
   = help: define an error enum with `#[derive(thiserror::Error)]`
```

### `result_result`

Flags `Result<Result<T, E1>, E2>` in function signatures and type aliases. Nested results force callers into awkward double-matching and usually indicate `.map()` where `.and_then()` was intended.

```
warning: nested `Result<Result<_, _>, _>` — consider flattening into a single Result
  --> src/loader.rs:5:34
   |
 5 | fn load(path: &str) -> Result<Result<Config, toml::de::Error>, io::Error> {
   |                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: use `.and_then()` to chain fallible operations, or unify the error
           types into a single enum
```

Complements Clippy's `option_option` (pedantic), which catches `Option<Option<T>>`.

### `realtime_in_async_test`

Flags two clock-correctness issues in async tests:

1. `tokio::time::sleep`, `sleep_until`, `timeout`, `timeout_at`, `interval`, `interval_at` inside async tests without a paused clock. Real wall-clock waits are slow and flaky; `start_paused = true` makes the clock auto-advance instantly.
2. `std::time::Instant::now()` inside tests that *do* set `start_paused = true`. The std clock ignores Tokio's paused clock, so measurements drift from the simulated time. Use `tokio::time::Instant::now()` instead.

```
warning: real-time wait in async test without paused clock
  --> src/jobs/retry_test.rs:12:5
   |
12 |     tokio::time::sleep(Duration::from_secs(5)).await;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: add `start_paused = true` to `#[tokio::test]` so the clock auto-advances and tests run instantly:
           `#[tokio::test(start_paused = true)]`
```

Walks local helper functions transitively, so tokio time calls hidden inside a helper called from the test are still flagged. Does not fire outside test functions or on `tokio::time::advance` (the correct tool for stepping a paused clock).

### `suggest_builder`

Suggests using a function-builder constructor for structs with many named fields.

```
warning: struct `Config` has 5 fields and may be a good candidate for a `#[builder]` constructor
  --> src/lib.rs:10:1
   = help: prefer `#[bon] impl` with `#[builder] fn new(...) -> Self` to enable the builder pattern
```

Does not fire on structs that already have a bon builder, have no constructor (no inherent `fn` returning `Self`, `Result<Self, _>`, or `Box<Self>`), derive any trait in `skip_derives` (default: `Default`, `Queryable`, `Insertable`, `Selectable`), are named `*Builder`, have lifetime parameters, are `#[repr(C)]`, or are generated by macros. `PhantomData` fields are not counted toward the threshold.

### `topological_ordering`

Flags items within a module that violate topological order based on the reference graph. An item must appear before any item that references it (callee-first) — leaf functions at the top, composition roots at the bottom.

```
warning: items are not in topological order in this module
  --> src/lib.rs:5:22
   |
5  |     fn process(_cfg: Config) {}
   |                      ^^^^^^ `fn process` references `struct Config` but appears before it
   |
   = help: reorder items so referenced items appear before their referencing items
```

A struct/enum and its inherent `impl` blocks are treated as one unit; separating them with unrelated items triggers a diagnostic. Trait impl blocks are ordered independently.

Mutual recursion is handled via strongly connected components — items in a cycle are unconstrained relative to each other, but the cycle as a whole is ordered relative to outside items.

Does not fire inside `#[cfg(test)]` modules or on macro-expanded items. Suppress with `#[allow(topological_ordering)]` on specific items or modules. See [docs/topological-ordering.md](docs/topological-ordering.md) for the full design.

### `unbounded_channel`

Flags creation of unbounded channels, which can cause memory exhaustion under backpressure.

```
warning: unbounded channel created — can exhaust memory under backpressure
  --> src/logger.rs:42:29
   |
42 |     let (tx, rx) = mpsc::unbounded_channel();
   |                         ^^^^^^^^^^^^^^^^^^
   |
   = help: use `mpsc::channel(capacity)` instead with an explicit bound
           (e.g., `channel(1000)`) to enable backpressure
```

Flagged by default: `std::sync::mpsc::channel`, `tokio::sync::mpsc::unbounded_channel`, `flume::unbounded`, `crossbeam::channel::unbounded`.

Does not fire inside `#[test]` / `#[tokio::test]`, `#[cfg(test)]` modules, or `fn main()`.

### `unclear_exports`

Flags glob imports (`use foo::*`) and renamed imports (`use foo::Bar as Baz`). Every imported name must be listed explicitly under its original name so the module's API surface is intentional and traceable.

```
error: glob imports (`use foo::*`) are banned — list each imported name explicitly
  --> src/lib.rs:3:1
   |
3  |     use utils::*;
   |     ^^^^^^^^^^^^^
   |
   = help: replace `use foo::*` with an explicit list: `use foo::{Bar, Baz}`
```

```
error: renamed imports (`use foo::Bar as Baz`) are banned — use the original name
  --> src/lib.rs:4:1
   |
4  |     use utils::Bar as Baz;
   |     ^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: import the item under its original name, or create a type alias
           if a new name is truly needed
```

Does not fire on underscore imports (`use foo::Bar as _`) or macro-expanded spans.

### `unsafe_send_missing_drop`

Warns when a type has `unsafe impl Send` but contains `!Send` fields and no `Drop` implementation. The implicit destructor will drop those `!Send` fields on whichever thread drops the owning struct, which is unsound when the fields have thread-affinity requirements (e.g. ObjC pointers that must be released on a specific dispatch queue).

```
warning: `Handle` has `unsafe impl Send` but contains `!Send` fields and no `Drop` impl
  --> src/handle.rs:19:1
   |
19 | struct Handle {
   | ^^^^^^^^^^^^^
   |
   = help: the implicit destructor drops `!Send` fields on the caller's thread;
           implement `Drop` to ensure `!Send` fields are destroyed in the correct context
```

`PhantomData<T>` and `ManuallyDrop<T>` fields are excluded — the former is zero-sized, the latter opts out of the implicit destructor. Unbounded generic fields (`T` without a `T: Send` bound) count as `!Send`, since the `unsafe impl` claims `Send` for all instantiations.

### `unstructured_log_fields`

Flags `tracing` macro invocations where all captured values are positional format arguments and none are structured key-value fields. Structured fields enable filtering, indexing, and machine-readable logs.

```
warning: `tracing::info!` uses format args instead of structured fields
  --> src/handler.rs:15:5
   |
15 |     tracing::info!("user {} hit {}", user_id, path);
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: use structured fields: `tracing::info!(user_id, path, "message")`
           instead of `tracing::info!("user {} path {}", user_id, path)`
```

Does not fire when at least one structured field is present, when the format string has no capture placeholders, or on non-tracing macros (e.g. `log::info!`).

---

## Usage

Choose one install path:

### Git source

Add to your workspace `Cargo.toml`:

```toml
[workspace.metadata.dylint]
libraries = [
    { git = "https://github.com/li-kai/rust-lints" },
]
```

This makes `cargo-dylint` clone and build the lints from source.

### Nix package

If you use Nix, prefer the flake package instead of the Git source. CI builds
`packages.default` for Linux and macOS and pushes the result to
`li-kai.cachix.org`. That package contains the prebuilt lint library and the
matching `dylint-driver`, so consumers do not need `rustup` or the pinned
nightly toolchain installed separately.

In your `flake.nix`:

```nix
{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-lints.url = "github:li-kai/rust-lints";
  };

  outputs = { self, flake-utils, nixpkgs, rust-lints, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        devShells.default = rust-lints.lib.mkDevShell {
          inherit pkgs;
          extraRustComponents = [ "rust-src" ];
          extraRustTargets = [ "wasm32-unknown-unknown" ];
          packages = [ pkgs.just ];
        };
      });
}
```

Then run:

```sh
cargo dylint --all
```

`rust-lints.lib.mkDevShell` is the supported Nix consumer interface. It wires in
the matching `cargo-dylint`, toolchain, `rustup` shim, `DYLINT_LIBRARY_PATH`,
and `DYLINT_DRIVER_PATH` so downstream repos do not need to reproduce this
repository's internal shell logic. Entering that shell means using the pinned
nightly Rust toolchain and matching nightly `rust-analyzer`.

`extraRustComponents` adds host-toolchain components on top of the required
baseline for `cargo`, `clippy`, `rustfmt`, and the Dylint runtime.
`extraRustTargets` adds target stdlibs such as `wasm32-unknown-unknown`.
`cargo-dylint` remains managed by the shell hook; it is not a Rust component.

`rust-lints.lib.dylintVersion` remains stable for advanced consumers that need
the raw CLI compatibility version. More detailed system-specific metadata is
available from `rust-lints.lib.dylint.forSystem system`.

See [docs/nix-packaging.md](docs/nix-packaging.md) for the package layout and
[docs/nix-cachix.md](docs/nix-cachix.md) for the CI publishing flow.

Configure thresholds and options in `dylint.toml`:

```toml
[suggest_builder]
threshold = 6
skip_derives = ["Default", "Queryable", "Insertable", "Selectable"]

[needless_builder]
threshold = 2

[fallible_new]
check_new_variants = true

[debug_remnants]
suggested_strategy = "tracing"  # or "log" for libraries
allow_in_tests = true
allow_in_test_modules = true

[unbounded_channel]
# additional_paths = ["my_app::channels::create_unbounded"]

[blocking_in_async]
# additional_paths = ["my_lib::database::connect_blocking"]

[await_holding_unsendable]
# additional_types = ["my_crate::MyGuard"]
# skip_default_types = false

[global_side_effect.time]
# additional_paths = ["my_crate::util::current_time"]

[global_side_effect.randomness]
# additional_paths = []

[global_side_effect.env]
# additional_paths = []

[global_side_effect.logging_init]
# additional_paths = []

[module_dependencies]
exhaustive = true

[module_dependencies.allow]
# types = []
# errors = ["types"]
# utils = ["types", "errors"]
# payments = ["types", "errors", "utils"]

[realtime_in_async_test]
# allowed_paths = ["my_crate::time::sleep"]
```

## Editor Setup

### Zed

Copy the relevant settings from `.zed/settings.json` into your project's Zed
settings. Key options:

- `check.overrideCommand` — runs clippy + dylint via `scripts/ra-check.sh`
- `rustc.source: "discover"` — resolves `rustc_private` crates from the sysroot
  (requires the `rustc-dev` component in your toolchain)

### Claude Code

This repo ships a `rust-analyzer` LSP plugin that disables `checkOnSave`
(navigation only — diagnostics come from hooks). This avoids conflicts with the
custom check command used by dylint.

```sh
# Register the marketplace (once per machine)
claude plugin marketplace add github:li-kai/rust-lints

# Install the plugin
claude plugin install rust-analyzer@rust-lints
```

Then enable it in your repo's `.claude/settings.json`:

```json
{
  "enabledPlugins": {
    "rust-analyzer@rust-lints": true
  }
}
```

## Development

Requires `dylint-link`:

```sh
cargo install dylint-link
```

Build and test:

```sh
just check-all
```
