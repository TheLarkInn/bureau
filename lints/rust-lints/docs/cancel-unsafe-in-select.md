# `cancel_unsafe_in_select`

**Level:** `warn`

Flags cancel-unsafe async calls inside `tokio::select!`, `futures::select!`, and `futures::select_biased!` arms. Losing branches are dropped mid-poll; a cancel-unsafe call in a losing branch loses progress.

## Why

`tokio::select!` polls every branch concurrently and keeps the first to complete. The losing futures are dropped wherever their last poll left them. If a losing future had consumed input, advanced a queue, or partially produced output, that progress is lost — the operation is neither completed nor reverted.

Cancel-unsafe operations fail in one of two ways:

- **Lost bytes or queue position** — the operation consumed input or advanced internal state that cannot be recovered from a dropped future.
- **Partial output** — the operation wrote some bytes to a peer before being dropped, leaving an inconsistent view.

The fix depends on the operation:

- **Reads** — use the cancel-safe `AsyncReadExt::read` and reassemble messages manually.
- **Writes** — run the write in `tokio::spawn` and select on its `JoinHandle`.
- **Locks and queues** — acquire before the `select!`, or restructure so the wait is in a dedicated task.

The lint inspects only the future expression (left of `=>`). The handler body (right of `=>`) runs after the future completes and is never dropped mid-poll.

## Cancel-unsafe paths

### `tokio::io::AsyncReadExt`

| Path | Risk |
|---|---|
| `tokio::io::AsyncReadExt::read_exact` | Partial fill — consumed bytes are lost |
| `tokio::io::AsyncReadExt::read_to_end` | Partial fill of destination vector |
| `tokio::io::AsyncReadExt::read_to_string` | Partial fill of destination string |
| `tokio::io::AsyncReadExt::read_{u,i}{16,32,64,128}` | Partial multi-byte read — bytes lost |
| `tokio::io::AsyncReadExt::read_{f32,f64}` | Partial multi-byte read — bytes lost |

`read` and `read_u8` are cancel-safe (single-byte or single-syscall reads) and not flagged. Little-endian variants (`read_u16_le`, etc.) share the risk of their big-endian counterparts above.

### `tokio::io::AsyncBufReadExt`

| Path | Risk |
|---|---|
| `tokio::io::AsyncBufReadExt::read_line` | Partial line pulled into internal buffer — next read skips or duplicates content |

### `tokio::io::AsyncWriteExt`

| Path | Risk |
|---|---|
| `tokio::io::AsyncWriteExt::write_all` | Partial write — peer sees truncated message |

### `tokio::sync` — queue-loss primitives

Cancelling the wait drops the future's place in the queue; cancellation loops can starve indefinitely under contention.

| Path | Risk |
|---|---|
| `tokio::sync::Mutex::lock` | Queue loss — starvation under contention |
| `tokio::sync::Mutex::lock_owned` | Queue loss |
| `tokio::sync::RwLock::read` | Queue loss |
| `tokio::sync::RwLock::write` | Queue loss |
| `tokio::sync::Semaphore::acquire` | Queue loss |
| `tokio::sync::Notify::notified` | Queue loss — wake-ups may be missed |

## Examples

### Triggers

```rust
tokio::select! {
    //~^ WARNING: cancel-unsafe call `read_exact` in `select!` arm
    res = socket.read_exact(&mut buf) => handle(res)?,
    _ = shutdown.recv() => break,
}
```

```rust
tokio::select! {
    //~^ WARNING: cancel-unsafe call `write_all` in `select!` arm
    _ = socket.write_all(&response) => {}
    _ = timeout => return Err(Timeout),
}
```

```rust
tokio::select! {
    //~^ WARNING: cancel-unsafe call `Mutex::lock` in `select!` arm
    guard = mutex.lock() => use_guard(guard),
    _ = shutdown.recv() => break,
}
```

### Does not trigger

```rust
// `read` (single syscall) is cancel-safe.
tokio::select! {
    res = socket.read(&mut buf) => handle(res)?,
    _ = shutdown.recv() => break,
}
```

```rust
// Channel `recv` is cancel-safe.
tokio::select! {
    msg = rx.recv() => process(msg),
    _ = shutdown.recv() => break,
}
```

```rust
// `tokio::time::sleep` / `timeout` / `Interval::tick` are cancel-safe.
tokio::select! {
    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
    _ = shutdown.recv() => break,
}
```

```rust
// Cancel-unsafe work isolated in a spawned task; the JoinHandle is cancel-safe.
let task = tokio::spawn(async move { socket.write_all(&buf).await });
tokio::select! {
    res = &mut task => res??,
    _ = shutdown.recv() => {}
}
```

## Configuration

```toml
[cancel_unsafe_in_select]
additional_paths = [
    "my_crate::protocol::read_frame",
    "my_crate::storage::commit",
]
```

| Field | Type | Default | Description |
|---|---|---|---|
| `additional_paths` | `Vec<String>` | `[]` | Extra cancel-unsafe paths to flag |
| `paths` | `Option<Vec<String>>` | `None` | If set, replaces the built-in defaults entirely |

## `futures::select!` nuance

When a future is passed as an expression (`fut = foo() => ...`), `futures::select!` pins it on the stack and drops it on loss — same semantics as `tokio::select!`. When a future is passed by mutable reference (`fut = &mut foo => ...`), the macro polls by reference and the future persists across iterations, making cancellation safe for that arm. The lint flags both cases on the conservative assumption that the common pattern is expression-form. Suppress with `#[expect(cancel_unsafe_in_select)]` on by-reference arms.

## Relation to Clippy

Clippy has no cancel-safety lint.

- `await_holding_invalid_type` concerns values held across `.await`, not futures polled inside a `select!` arm.
- `await_holding_lock` and `await_holding_refcell_ref` cover specific guard types held across `.await`.

## Why not `disallowed_methods`

`read_exact`, `write_all`, and `Mutex::lock` are correct outside a `select!` arm. `disallowed_methods` cannot scope to "inside a `select!` arm", so blanket-disallowing them would flag mostly legitimate uses.

## Relation to other lints

| Lint | Catches |
|---|---|
| `cancel_unsafe_in_select` | Futures that corrupt state when dropped mid-poll in a `select!` arm |
| `await_holding_unsendable` | Guard types held alive across `.await` |
| `blocking_in_async` | Synchronous calls that stall the executor inside async code |
| `realtime_in_async_test` | Tokio timer and clock misuse in async tests |
