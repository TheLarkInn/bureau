# `debug_remnants`

**Level:** `warn`

Flags debugging macros (`println!()`, `print!()`, `eprintln!()`, `dbg!()`) and suggests structured logging replacements.

## Why

Debug macros should not persist in production code:

- **Unstructured output** — `println!` and `eprintln!` produce plain text with no machine-readable structure. In production with thousands of log lines per second, you cannot filter, aggregate, or correlate them across services.
- **Lost context** — raw print statements cannot capture span information, request IDs, or async task context. Debugging becomes a wall of unrelated text.
- **No severity levels** — `println!` has no concept of debug vs. info vs. error. You cannot control verbosity in production.
- **Development artifacts** — temporary debug output accidentally shipped to production, filling logs with noise.
- **Testing fragility** — programs that write to stderr may fail in test harnesses that capture output.

Use structured logging (`tracing` for applications, `log` for libraries) for runtime verbosity control and observability integration.

## Flagged Expressions

| Expression | Example | Reason |
|---|---|---|
| `println!(...)` | `println!("got here")` | Unstructured stdout output |
| `print!(...)` | `print!("debug: ")` | Unstructured stdout output (no newline variant) |
| `eprintln!(...)` | `eprintln!("ERROR: {}", err)` | Unstructured stderr output |
| `dbg!(...)` | `dbg!(value)` | Temporary debugging tool (prints to stderr) |

`eprint!` is intentionally excluded — it is typically used for progress indicators and prompts, not leftover debugging.

## Examples

### Triggers

```rust
fn process_payment(amount: u64) -> Result<Receipt> {
    //~^ WARNING: debug remnant: `println!` in committed code
    println!("Processing ${}", amount);
    let receipt = do_payment()?;
    Ok(receipt)
}
```

```rust
impl Handler {
    fn handle_request(&self, req: Request) -> Response {
        //~^ WARNING: debug remnant: `eprintln!` in committed code
        eprintln!("Got request: {:?}", req);
        self.process(req)
    }
}
```

```rust
fn validate(config: &Config) -> bool {
    //~^ WARNING: debug remnant: `dbg!` in committed code
    dbg!(config);
    config.is_valid()
}
```

```rust
fn main() {
    //~^ WARNING: debug remnant: `println!` in committed code
    println!("Starting server");
    run()
}
```

### Does not trigger

```rust
// Structured logging (tracing)
fn process_payment(amount: u64) -> Result<Receipt> {
    tracing::info!(amount, "processing payment");
    let receipt = do_payment()?;
    tracing::debug!(?receipt, "payment completed");
    Ok(receipt)
}

// Structured logging (log crate in libraries)
pub fn parse_config(input: &str) -> Result<Config> {
    log::info!("parsing config");
    let config = toml::from_str(input)?;
    Ok(config)
}

// Inside a #[test] function
#[test]
fn test_payment() {
    println!("Testing payment logic");  // ok
    assert!(process_payment(100).is_ok());
}

// Inside #[cfg(test)] modules
#[cfg(test)]
mod tests {
    fn setup() {
        eprintln!("Setting up test environment");  // ok
    }
}

// Explicit override with reason
fn cli_entrypoint() {
    #[allow(debug_remnants, reason = "CLI tool prints to stdout as its interface")]
    println!("Usage: tool <command>");
}
```

## Suppression zones

The lint does not fire in these contexts:

| Context | Detection |
|---|---|
| Test crates | Integration test files (`tests/`), or the main crate compiled with `cargo test`. Detected via `is_test_crate()`. |
| Test functions | Any function registered with the test harness (`#[test]`, `#[tokio::test]`, `#[rstest]`, etc.) — detected via `#[rustc_test_marker]` |
| `#[cfg(test)]` modules | Item is inside a `#[cfg(test)]` module |
| `#[allow(debug_remnants)]` | Standard rustc suppression attribute |

No implicit exemptions for `fn main()` or binary crates. Use `#[allow(debug_remnants, reason = "...")]` for intentional print output in CLI tools or entry points.

## Configuration

```toml
[debug_remnants]
# Which logging framework to suggest: "tracing" (default) or "log"
suggested_framework = "tracing"
```

| Field | Type | Default | Description |
|---|---|---|---|
| `suggested_framework` | `"tracing"` \| `"log"` | `"tracing"` | Which logging framework to suggest in diagnostics |

Test suppression (test crates, `#[test]` functions, `#[cfg(test)]` modules) is always active and not configurable.

### Strategy guide

- **`suggested_framework = "tracing"`** (default) — Suggest `tracing::info!()`, `tracing::debug!()`, etc. Use for applications and async code.
- **`suggested_framework = "log"`** — Suggest `log::info!()`, `log::debug!()`, etc. Use for libraries where `tracing` is not a dependency.

## Suggested Fixes by Strategy

### For Applications (tracing strategy)

**Before:**
```rust
fn fetch_user(id: u64) -> Result<User> {
    println!("Fetching user {}", id);
    let user = db.query(id)?;
    Ok(user)
}
```

**After:**
```rust
fn fetch_user(id: u64) -> Result<User> {
    tracing::info!(id, "fetching user");
    let user = db.query(id)?;
    Ok(user)
}
```

Or with `#[tracing::instrument]`:
```rust
#[tracing::instrument(skip(db))]
fn fetch_user(id: u64) -> Result<User> {
    let user = db.query(id)?;
    Ok(user)
}
```

### For Libraries (log strategy)

**Before:**
```rust
pub fn parse(input: &str) -> Result<Data> {
    eprintln!("Parsing input");
    // ...
}
```

**After:**
```rust
pub fn parse(input: &str) -> Result<Data> {
    log::info!("parsing input");
    // ...
}
```

## Relation to Clippy

Disable these Clippy lints when `debug_remnants` is active to avoid duplicate warnings:

```toml
# Cargo.toml — turn off clippy lints that debug_remnants replaces
[workspace.lints.clippy]
dbg_macro    = "allow"  # superseded by debug_remnants
print_stdout = "allow"  # superseded by debug_remnants
print_stderr = "allow"  # superseded by debug_remnants
```

| Clippy Lint | What `debug_remnants` adds |
|---|---|
| `clippy::dbg_macro` | Suggests `tracing::debug!()` replacement; suppresses in tests |
| `clippy::print_stdout` | Suggests `tracing::info!()` replacement; covers `print!` and `println!` together |
| `clippy::print_stderr` | Suggests `tracing::warn!()` replacement; unified configuration |

Why supersede rather than wrap:

- **Actionable diagnostics** — Clippy says "don't use this." This lint suggests a concrete replacement.
- **Unified config** — one `suggested_framework` knob instead of three separate lints.
- **Test-aware** — suppresses in `#[test]` and `#[cfg(test)]` without per-lint `allow` attributes.
- **`print!` coverage** — Clippy requires two separate lints (`print_stdout`, `print_stderr`); this lint catches all four macros in one pass.
