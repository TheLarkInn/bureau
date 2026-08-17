# `unbounded_channel`

**Level:** `deny`

Flags creation of unbounded channels, which can cause memory exhaustion under backpressure. Suggests using bounded channels instead.

## Why

Unbounded channels have no backpressure. When producers outrun consumers:

- **Unbounded memory growth** — messages queue indefinitely, consuming memory until OOM. Unlike bounded channels, once memory is allocated during a backpressure spike, it's never freed even after the spike ends.
- **Hard to debug** — OOM crashes occur far away from the channel creation site, making root cause analysis difficult.
- **No flow control** — a runaway producer can take down the entire system without warning. With bounded channels, at least the `.send()` call would fail or block, giving callers an opportunity to notice and throttle.
- **Usually a mistake** — most unbounded channels are created "for now" and forgotten. In production under load, they become a vector for memory exhaustion attacks (even internal ones from misbehaving subsystems).

Use a bounded channel with an explicit capacity. Backpressure may require refactoring, but discovering that at design time beats discovering it at 3am in production.

## Flagged calls

Built-in flagged constructors:

### `std::sync::mpsc`

| Path | Notes |
|---|---|
| `std::sync::mpsc::channel` | Creates unbounded MPSC channel (returns `(Sender, Receiver)`) |

Note: `std::sync::mpsc` is deprecated in favor of async channels. If your channel is unbounded, this is likely a sign that you should be using `tokio::sync::mpsc` with an explicit bound instead.

### `tokio::sync::mpsc`

| Path | Notes |
|---|---|
| `tokio::sync::mpsc::unbounded_channel` | Explicitly unbounded async MPSC channel |

### `flume`

| Path | Notes |
|---|---|
| `flume::unbounded` | Create unbounded multi-producer channel |

### `crossbeam::channel`

| Path | Notes |
|---|---|
| `crossbeam_channel::unbounded` | Create unbounded crossbeam channel (standalone crate) |
| `crossbeam::channel::unbounded` | Create unbounded crossbeam channel (umbrella re-export) |

### Examples

#### Triggers

```rust
use tokio::sync::mpsc;

async fn setup_logger() {
    //~^ ERROR: `tokio::sync::mpsc::unbounded_channel()` creates an unbounded channel — no backpressure
    let (tx, mut rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("{}", msg);
        }
    });
}
```

```rust
use std::sync::mpsc;

fn start_worker() {
    //~^ ERROR: `std::sync::mpsc::channel()` creates an unbounded channel — no backpressure
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        while let Ok(item) = rx.recv() {
            process(item);
        }
    });
}
```

```rust
use crossbeam::channel;

fn main() {
    //~^ ERROR: `crossbeam::channel::unbounded()` creates an unbounded channel — no backpressure
    let (tx, rx) = channel::unbounded();

    // ...
}
```

#### Does not trigger

```rust
use tokio::sync::mpsc;

async fn setup_logger_safe() {
    // Bounded to 1000 messages — backpressure kicks in after that
    let (tx, mut rx) = mpsc::channel(1000);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            println!("{}", msg);
        }
    });
}
```

```rust
use std::sync::mpsc;

fn start_worker_safe() {
    // std::sync::mpsc::channel with 100 message buffer
    let (tx, rx) = mpsc::sync_channel(100);

    std::thread::spawn(move || {
        while let Ok(item) = rx.recv() {
            process(item);
        }
    });
}
```

```rust
use flume;

fn main() {
    // Bounded to 500 messages
    let (tx, rx) = flume::bounded(500);

    // ...
}
```

## Suppression zones

The lint is automatically suppressed in:

- **`fn main()`** — the composition root, where wiring up real dependencies (including unbounded channels) is expected.
- **Test code** — `#[test]` functions, `#[tokio::test]`, `#[cfg(test)]` modules, and test crates.
- **`#[allow(unbounded_channel)]`** — explicit per-site opt-out.

## Configuration

```toml
[unbounded_channel]
# Additional paths to flag beyond the built-in defaults.
additional_paths = [
    "my_app::channels::create_unbounded",
]

# Or override the defaults entirely.
# paths = [
#     "tokio::sync::mpsc::unbounded_channel",
#     "crossbeam::channel::unbounded",
# ]
```

| Field | Type | Default | Description |
|---|---|---|---|
| `additional_paths` | `Vec<String>` | `[]` | Extra unbounded channel paths to flag |
| `paths` | `Option<Vec<String>>` | `None` | If set, replaces built-in defaults entirely |

## Choosing a Bounded Capacity

- **Logging channels:** 1,000–10,000 messages (logs rarely get backed up)
- **Work queues:** 10–100 tasks (tune based on memory budget and expected queue depth)
- **Event buses:** 100–1,000 events (depends on burst size)
- **RPC response channels:** 1–10 (keep it tight; if responses are queuing, something is wrong upstream)

When in doubt, start small (e.g., 100) and increase if you observe legitimate backpressure. It's better to discover a design issue (producer too fast) early than to silently exhaust memory in production.

## Relation to other lints

| Related lint | Catches |
|---|---|
| `large_struct` / `large_future` (Clippy) | Oversized allocations → stack overflow / memory pressure |
