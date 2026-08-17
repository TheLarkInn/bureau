# `realtime_in_async_test`

**Level:** `warn`

Flags two correctness issues in async tests using Tokio time:

1. `tokio::time::sleep`, `sleep_until`, `timeout`, `timeout_at`, `interval`, `interval_at` inside async test functions that don't have the Tokio clock paused
2. `std::time::Instant::now()` inside async tests that have a paused Tokio clock (`start_paused = true`)

## Why

Async tests that call `tokio::time::sleep` or similar functions wait on real time by default, slowing CI and causing flakiness under load. `start_paused = true` solves this: the clock starts frozen and auto-advances when the runtime would otherwise wait for a timer.

The second failure mode is mixing `std::time::Instant` with Tokio time control. `tokio::time::pause()` and `#[tokio::test(start_paused = true)]` affect `tokio::time::Instant`, not `std::time::Instant`.

## Examples

### Triggers

```rust
#[tokio::test]
async fn test_retry_backoff() {
    //~^ WARNING: real-time wait in async test without paused clock
    tokio::time::sleep(Duration::from_secs(5)).await;
}
```

```rust
#[tokio::test]
async fn test_request_timeout() {
    let _ = tokio::time::timeout( //~ WARNING: real-time wait in async test without paused clock
        Duration::from_secs(5),
        fetch_data(),
    ).await;
}
```

`std::time::Instant::now()` in a paused-clock test:

```rust
#[tokio::test(start_paused = true)]
async fn test_retry_budget() {
    let start = std::time::Instant::now();
    //~^ WARNING: `std::time::Instant::now()` does not respect Tokio's paused clock
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(start.elapsed() >= Duration::from_secs(30));
}
```

### Does not trigger

```rust
// Clock is paused — time calls are instant.
#[tokio::test(start_paused = true)]
async fn test_retry_backoff() {
    tokio::time::sleep(Duration::from_secs(5)).await; // OK
}

// Manual runtime with start_paused(true) is also recognized.
#[test]
fn test_with_manual_runtime() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::sleep(Duration::from_secs(60)).await; // OK
        });
}

// Not a test function — production code is not flagged.
async fn wait_for_ready() {
    tokio::time::sleep(Duration::from_secs(1)).await; // OK
}

// tokio::time::advance is the solution, not the problem.
#[tokio::test(start_paused = true)]
async fn test_advance() {
    tokio::time::advance(Duration::from_secs(60)).await; // OK
}

// Consistent with Tokio's logical clock.
#[tokio::test(start_paused = true)]
async fn test_deadline() {
    let start = tokio::time::Instant::now(); // OK
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(tokio::time::Instant::now() >= start);
}

// Real-time measurement in a plain test is outside this lint's scope.
#[test]
fn test_actual_elapsed_time() {
    let start = std::time::Instant::now(); // OK
    std::thread::sleep(Duration::from_millis(10));
    assert!(start.elapsed() >= Duration::from_millis(10));
}
```

## Configuration

```toml
[realtime_in_async_test]
additional_paths = ["my_crate::time::sleep"]
```

| Field | Type | Default | Description |
|---|---|---|---|
| `additional_paths` | `[string]` | `[]` | Additional fully-qualified paths to treat as Tokio-style time calls (appended to the defaults) |
| `paths` | `[string] \| null` | `null` | If set, replaces the built-in Tokio time call list entirely |

## Relation to `blocking_in_async`

This lint is about Tokio clock correctness in tests.

- `tokio::time::sleep(...)` without `start_paused = true` is a test clock problem (this lint).
- `std::time::Instant::now()` in a Tokio-timed test is a clock mismatch (this lint, planned).
- `std::thread::sleep(...)` inside async code is a blocking problem (`blocking_in_async`).
