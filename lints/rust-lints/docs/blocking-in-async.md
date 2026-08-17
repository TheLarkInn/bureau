# `blocking_in_async`

**Level:** `deny`

Flags known-blocking operations inside `async fn` or `async {}` blocks. Suggests using async-aware alternatives or `spawn_blocking` instead.

## Why

Calling blocking operations inside async code blocks the executor thread, causing:

- **Executor starvation** — the thread is blocked while other tasks are waiting for that thread to progress. A single blocking call can starve hundreds of tasks on a single-threaded executor.
- **Unpredictable latency** — callers of your async function don't expect it to block the executor. Adding a single `fs::read()` can turn a `1µs` task into a `10ms` task, cascading delays across the entire application.
- **Deadlocks in thread pools** — if all worker threads are blocked on I/O, no task can make progress. This is a classic cause of async deadlocks.
- **Defeats the point of async** — async is for I/O multiplexing. Blocking inside async wastes the entire benefit.

Use async-aware alternatives (`tokio::fs`) or offload blocking work with `tokio::task::spawn_blocking`.

This lint is about executor blocking, not test clock semantics. A Tokio test using `tokio::time::sleep(...)` without `start_paused = true` has a clock problem, not a blocking problem. That case belongs to `realtime_in_async_test`.

## Flagged calls

Built-in flagged paths:

### `std::fs`

| Path | Notes |
|---|---|
| `std::fs::read` | Synchronous file read |
| `std::fs::read_to_string` | Synchronous file read to string |
| `std::fs::write` | Synchronous file write |
| `std::fs::read_dir` | Synchronous directory listing |
| `std::fs::metadata` | Synchronous metadata lookup |
| `std::fs::canonicalize` | Synchronous path resolution |
| `std::fs::copy` | Synchronous file copy |
| `std::fs::create_dir` | Synchronous directory creation |
| `std::fs::create_dir_all` | Synchronous recursive directory creation |
| `std::fs::remove_file` | Synchronous file removal |
| `std::fs::remove_dir` | Synchronous directory removal |
| `std::fs::remove_dir_all` | Synchronous recursive directory removal |
| `std::fs::rename` | Synchronous file rename |

### `std::io`

| Path | Notes |
|---|---|
| `std::io::Stdin::read_line` | Blocks on keyboard input |
| `std::io::Stdin::read` | Blocks on stdin |

### `std::net`

| Path | Notes |
|---|---|
| `std::net::TcpStream::connect` | Synchronous TCP connection |
| `std::net::UdpSocket::bind` | Synchronous socket bind |
| `std::net::TcpListener::bind` | Synchronous listener creation |

### `std::thread`

| Path | Notes |
|---|---|
| `std::thread::sleep` | Blocks the thread; unlike `tokio::time::sleep`, this stalls the executor itself |
| `std::thread::spawn` | Bypasses the executor; use `tokio::task::spawn` instead |

### `std::sync`

| Path | Notes |
|---|---|
| `std::sync::Mutex::lock` | Blocks on contention (use `tokio::sync::Mutex` instead) |
| `std::sync::RwLock::read` | Blocks on contention |
| `std::sync::RwLock::write` | Blocks on contention |

### `parking_lot`

| Path | Notes |
|---|---|
| `parking_lot::Mutex::lock` | Synchronous lock (use async mutex) |
| `parking_lot::RwLock::read` | Synchronous read lock |
| `parking_lot::RwLock::write` | Synchronous write lock |

### `tokio::task`

| Path | Notes |
|---|---|
| `tokio::task::block_in_place` | Risky on single-threaded executors; prefer `spawn_blocking` |

### Examples

#### Triggers

```rust
async fn fetch_user_config(user_id: u32) -> Config {
    //~^ ERROR: blocking call to `std::fs::read_to_string()` inside async context
    let path = format!("/data/{}.toml", user_id);
    let contents = std::fs::read_to_string(&path).unwrap();
    Config::parse(&contents)
}
```

```rust
async fn connect() -> Connection {
    //~^ ERROR: blocking call to `std::net::TcpStream::connect()` inside async context
    let stream = std::net::TcpStream::connect("127.0.0.1:5432").unwrap();
    Connection::new(stream)
}
```

```rust
async fn process() {
    //~^ ERROR: blocking call to `std::thread::sleep()` inside async context
    std::thread::sleep(Duration::from_secs(1));
}
```

```rust
async fn safe_acquire(mtx: &std::sync::Mutex<Data>) -> Data {
    //~^ ERROR: blocking call to `std::sync::Mutex::lock()` inside async context
    let guard = mtx.lock().unwrap();
    guard.clone()
}
```

#### Does not trigger

```rust
// Using async-aware alternative
async fn fetch_user_config(user_id: u32) -> Config {
    let path = format!("/data/{}.toml", user_id);
    let contents = tokio::fs::read_to_string(&path).await.unwrap();
    Config::parse(&contents)
}

// Using spawn_blocking for CPU-bound work or unavoidable blocking
async fn process() {
    tokio::task::spawn_blocking(|| {
        std::thread::sleep(Duration::from_secs(1));
    }).await.unwrap();
}

// Using tokio::net for async I/O
async fn connect() -> Connection {
    let stream = tokio::net::TcpStream::connect("127.0.0.1:5432").await.unwrap();
    Connection::new(stream)
}

// Using tokio::sync::Mutex for async-safe locks
async fn safe_acquire(mtx: &tokio::sync::Mutex<Data>) -> Data {
    let guard = mtx.lock().await;
    guard.clone()
}

// Synchronous code (not inside async)
fn fetch_config(user_id: u32) -> Config {
    let path = format!("/data/{}.toml", user_id);
    let contents = std::fs::read_to_string(&path).unwrap(); // ok
    Config::parse(&contents)
}

// Inside #[test] or #[tokio::test] — blocking in tests is acceptable
#[tokio::test]
async fn test_config_load() {
    let contents = std::fs::read_to_string("config.toml").unwrap(); // ok
}
```

## Configuration

```toml
[blocking_in_async]
# Additional paths to flag beyond the built-in defaults.
additional_paths = [
    "my_lib::database::connect_blocking",
]

# Or override the defaults entirely.
# paths = [
#     "std::fs::read",
#     "std::net::TcpStream::connect",
# ]
```

| Field | Type | Default | Description |
|---|---|---|---|
| `additional_paths` | `Vec<String>` | `[]` | Extra blocking function paths to flag |
| `paths` | `Option<Vec<String>>` | `None` | If set, replaces built-in defaults entirely |

## Relation to other lints

This lint complements `await_holding_lock` (Clippy), `global_side_effect::time`, and `realtime_in_async_test`:

| Lint | Catches |
|---|---|
| `blocking_in_async` | Blocking operations starving the executor |
| `await_holding_lock` (Clippy) | `std::sync::Mutex` held across `.await` |
| `global_side_effect::time` | Non-testable direct time dependencies in production code |
| `realtime_in_async_test` | Tokio timer misuse and `std::time::Instant` mismatches in async tests |
