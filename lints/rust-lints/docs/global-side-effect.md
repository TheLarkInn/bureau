# Global Side-Effect Lints

**Lints:** `global_side_effect::time`, `global_side_effect::randomness`, `global_side_effect::env`, `global_side_effect::logging_init`
**Levels:** `warn` for `time`, `randomness`, and `env`; `deny` for `logging_init`

Four lints that flag direct calls to non-deterministic or environment-coupled functions. The first three target runtime dependencies — wall-clock time, random number generation, and environment variables — fix: **accept the dependency as a parameter**. The fourth targets global logger initialization — fix: **move it to `main()`**.

## Why

Direct calls to these functions are untestable (inputs cannot be controlled), non-deterministic (same inputs, different outputs), and hard to mock without thread-local hacks.

Global tracing subscriber initialization (`tracing_subscriber::fmt::init()`, etc.) shares the same structural problem: it mutates process-global state, it can only succeed once per process, and when called from library code or deep inside the call graph it causes silent failures in tests (the second `init()` panics or is silently ignored) and removes the application author's ability to choose their own logging configuration.

## Flagged calls

Built-in flagged paths by lint:

### `global_side_effect::time`

| Path | Notes |
|---|---|
| `std::time::SystemTime::now` | Wall-clock time |
| `std::time::Instant::now` | Monotonic clock |
| `chrono::Utc::now` | |
| `chrono::Local::now` | |
| `chrono::offset::Utc::now` | Re-export |
| `chrono::offset::Local::now` | Re-export |
| `time::OffsetDateTime::now_utc` | |
| `time::OffsetDateTime::now_local` | |
| `time::Instant::now` | |
| `jiff::Zoned::now` | |
| `jiff::Timestamp::now` | |
| `tokio::time::Instant::now` | |

### `global_side_effect::randomness`

| Path | Notes |
|---|---|
| `std::random::random` | Direct random value |
| `rand::thread_rng` | Thread-local RNG (rand 0.8) |
| `rand::rng` | Thread-local RNG (rand 0.9+) |
| `rand::random` | Wrapper around thread-local RNG |
| `rand::random_range` | Random value in range (rand 0.9+) |
| `rand::rngs::OsRng::new` | OS randomness |
| `rand::rngs::StdRng::from_os_rng` | Seeded from OS |
| `getrandom::getrandom` | Low-level OS randomness |
| `fastrand::bool` | Random bool |
| `fastrand::u8` .. `fastrand::u128` | Random unsigned integers |
| `fastrand::usize` | Random usize |
| `fastrand::i8` .. `fastrand::i128` | Random signed integers |
| `fastrand::isize` | Random isize |
| `fastrand::f32`, `fastrand::f64` | Random floats |
| `fastrand::char` | Random char |
| `fastrand::Rng::new` | RNG from OS seed |

### `global_side_effect::env`

| Path | Notes |
|---|---|
| `std::env::var` | Read a single env var |
| `std::env::var_os` | Read a single env var (OsString) |
| `std::env::vars` | Iterate all env vars |
| `std::env::vars_os` | Iterate all env vars (OsString) |
| `std::env::args` | Command-line arguments |
| `std::env::args_os` | Command-line arguments (OsString) |
| `dotenvy::dotenv` | Load `.env` file |
| `dotenvy::dotenv_override` | Load `.env` file, overriding existing |
| `dotenvy::from_filename` | Load from custom filename |
| `dotenvy::var` | Read from `.env` file |
| `dotenvy::vars` | Iterate `.env` vars |
| `dotenv::dotenv` | Load `.env` file (unmaintained) |
| `dotenv::var` | Read from `.env` file (unmaintained) |
| `dotenv::vars` | Iterate `.env` vars (unmaintained) |

### `global_side_effect::logging_init`

| Path | Notes |
|---|---|
| `tracing_subscriber::fmt::init` | Sets the global default subscriber |
| `tracing_subscriber::fmt::try_init` | Same, returns `Result` instead of panicking |
| `tracing_subscriber::fmt::SubscriberBuilder::init` | Builder-style init |
| `tracing_subscriber::fmt::SubscriberBuilder::try_init` | Builder-style try-init |
| `tracing_subscriber::util::SubscriberInitExt::init` | Extension trait `init()` |
| `tracing_subscriber::util::SubscriberInitExt::try_init` | Extension trait `try_init()` |
| `tracing::subscriber::set_global_default` | Low-level global subscriber registration |

## Examples

### `time`, `randomness`, `env`

The first three lints follow the same pattern. These examples use `global_side_effect::time`; substitute the relevant function for the other two.

#### Triggers

```rust
use std::time::Instant;

fn is_expired(&self) -> bool {
    //~^ WARNING: direct call to `std::time::Instant::now()`
    Instant::now() > self.deadline
}
```

#### Does not trigger

```rust
// Injected as a parameter
fn is_expired(&self, now: Instant) -> bool {
    now > self.deadline
}

// Trait-based injection
fn is_expired(&self, clock: &impl Clock) -> bool {
    clock.now() > self.deadline
}

// Inside #[test] or #[cfg(test)]
#[test]
fn test_something() {
    let start = Instant::now(); // ok
}

// In main() — the composition root
fn main() {
    let start = Instant::now(); // ok
    run(start);
}
```

### `logging_init`

#### Triggers

```rust
// Inside a library function
pub fn setup_app() {
    //~^ ERROR: direct call to `tracing_subscriber::fmt::init()`
    tracing_subscriber::fmt::init();
    // ...
}

// Inside a module initializer
pub fn configure_service(config: &Config) {
    //~^ ERROR: direct call to `tracing_subscriber::fmt::SubscriberBuilder::try_init()`
    tracing_subscriber::fmt().try_init();
    start_service(config);
}
```

#### Does not trigger

```rust
// In main() — the only place global logger init belongs
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init(); // ok — this is the composition root
    run();
}

// In a function that returns a subscriber without installing it globally
fn build_subscriber() -> impl tracing::Subscriber {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .finish() // ok — returns the subscriber, doesn't install it
}

// Inside #[test] or #[cfg(test)]
#[test]
fn test_with_logging() {
    let _ = tracing_subscriber::fmt().try_init(); // ok
}
```

## Suppression zones

The lint does not fire in these contexts:

| Context | Detection |
|---|---|
| Test crates | Integration test files (`tests/`), or the main crate compiled with `cargo test`. Detected via `--test` flag (`is_test_crate()`). Covers test helpers that don't carry `#[test]` themselves. |
| Test functions | Any function registered with the test harness (`#[test]`, `#[tokio::test]`, `#[async_std::test]`, `#[rstest]`, `#[test_case::test]`, `#[googletest::test]`, etc.) — detected via the compiler's `#[rustc_test_marker]` |
| `#[cfg(test)]` modules | Item is inside a `#[cfg(test)]` module |
| `fn main()` | Enclosing function is `main` |
| `#[allow(global_side_effect_time)]` (etc.) | Standard rustc attribute — use the per-lint name |

## Configuration

All four lints accept the same two fields. Use the lint name as the TOML section key.

```toml
[global_side_effect.time]
# Extra paths to flag, merged with built-in defaults.
additional_paths = ["my_crate::util::current_time"]

# Replace built-in defaults entirely.
# paths = ["std::time::Instant::now", "chrono::Utc::now"]
```

| Field | Type | Default | Description |
|---|---|---|---|
| `additional_paths` | `Vec<String>` | `[]` | Extra paths to flag, merged with defaults |
| `paths` | `Option<Vec<String>>` | `None` | If set, replaces built-in defaults entirely |
