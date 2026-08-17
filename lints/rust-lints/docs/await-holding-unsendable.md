# `await_holding_unsendable`

**Level:** `deny`

Flags values of specific types that are held alive across `.await` points. Complements Clippy's `await_holding_lock` and `await_holding_refcell_ref` by covering types Clippy doesn't know about: tracing span guards, parking_lot locks, connection pool handles, crossbeam epoch guards, and any custom types via configuration.

## Why

- **Deadlocks** — a `MutexGuard` held across `.await` keeps the lock while the task is suspended. If another task on the same executor needs the lock to make progress, the executor deadlocks. Unlike blocking code, the holder isn't actively running toward the unlock.
- **Broken span nesting** — a `tracing::span::Entered` guard held across `.await` keeps the span active while the task is suspended. When another task runs on the same thread, its events are attributed to the wrong span.
- **RefCell panics** — a `Ref` or `RefMut` held across `.await` keeps the borrow active across an unbounded suspension. Any attempt to borrow the same `RefCell` will panic.
- **Cancellation unsafety** — if the future is dropped at the `.await` point, the guard's `Drop` runs in an arbitrary context. For mutex guards this unlocks from a potentially unexpected state; for span guards this corrupts the thread-local span stack.

The fix is to scope the guard so it is dropped before the `.await`:

```rust
// Before (deadlock risk):
async fn bad(mtx: &Mutex<Data>) {
    let guard = mtx.lock();
    send(guard.clone()).await;  // guard held across await
}

// After:
async fn good(mtx: &Mutex<Data>) {
    let data = {
        let guard = mtx.lock();
        guard.clone()
    };
    send(data).await;  // guard already dropped
}
```

## What it checks

The lint fires when a value of a flagged type is live across an `.await` expression. A value is "live across `.await`" when it is created before the `.await` and used or dropped after it, or when its scope encloses the `.await`.

## Default types

Standard library guards (`MutexGuard`, `RwLockReadGuard`, `RwLockWriteGuard`) and `RefCell` refs (`Ref`, `RefMut`) are intentionally **not** included — Clippy's `await_holding_lock` and `await_holding_refcell_ref` already cover them with diagnostic-item-based matching that is more robust than path-string matching. Keep those Clippy lints enabled alongside this one.

### `parking_lot` — alternative lock guards

| Type | Risk |
|---|---|
| `parking_lot::MutexGuard` | Deadlock |
| `parking_lot::FairMutexGuard` | Deadlock |
| `parking_lot::RwLockReadGuard` | Deadlock |
| `parking_lot::RwLockWriteGuard` | Deadlock |
| `parking_lot::RwLockUpgradableReadGuard` | Deadlock |
| `parking_lot::MappedMutexGuard` | Deadlock |
| `parking_lot::MappedFairMutexGuard` | Deadlock |
| `parking_lot::MappedRwLockReadGuard` | Deadlock |
| `parking_lot::MappedRwLockWriteGuard` | Deadlock |
| `parking_lot::ArcMutexGuard` | Deadlock |
| `parking_lot::ArcRwLockReadGuard` | Deadlock |
| `parking_lot::ArcRwLockWriteGuard` | Deadlock |
| `parking_lot::ArcRwLockUpgradableReadGuard` | Deadlock |

### `tracing` — span guards

| Type | Risk |
|---|---|
| `tracing::span::Entered` | Corrupted span nesting — events on other tasks attributed to wrong span |
| `tracing::span::EnteredSpan` | Corrupted span nesting — events on other tasks attributed to wrong span |

### `crossbeam` — epoch-based reclamation

| Type | Risk |
|---|---|
| `crossbeam_epoch::Guard` | Delays memory reclamation while the task is suspended, causing unbounded memory growth |

### `rusqlite` — database transactions

| Type | Risk |
|---|---|
| `rusqlite::Transaction` | Holds exclusive connection lock — blocks all other queries while suspended |
| `rusqlite::Savepoint` | Holds exclusive connection lock — blocks all other queries while suspended |

### Connection pools — checked-out connections

| Type | Risk |
|---|---|
| `r2d2::PooledConnection` | Pool starvation — connection checked out across `.await` is unavailable to other tasks while suspended; with small pools this deadlocks the application |
| `diesel::r2d2::PooledConnection` | Same as `r2d2::PooledConnection` (re-export) |

## Examples

### Fires

```rust
async fn traced() {
    let span = tracing::info_span!("op");
    let _entered = span.enter();
    //~^ ERROR: `Entered` held across `.await` — corrupted span nesting
    do_work().await;
}
```

```rust
async fn pool(pool: &r2d2::Pool<MyManager>) {
    let conn = pool.get().unwrap();
    //~^ ERROR: `PooledConnection` held across `.await` — pool starvation
    fetch(&conn).await;
}
```

### Does not fire

```rust
// Tracing's async-aware instrument pattern — no Entered guard.
async fn good() {
    do_work()
        .instrument(tracing::info_span!("op"))
        .await; // OK — no Entered guard
}
```

```rust
// Entered scoped before .await.
async fn good() {
    let span = tracing::info_span!("op");
    {
        let _entered = span.enter();
        // sync work under the span
    }
    do_work().await; // OK — entered already dropped
}
```

```rust
// Synchronous code — no async context.
fn sync_fn() {
    let span = tracing::info_span!("op");
    let _entered = span.enter(); // OK — not async
}
```

```rust
// Inside #[cfg(test)] — test code is excluded.
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_span() {
        let span = tracing::info_span!("test");
        let _entered = span.enter();
        do_work().await; // OK — test code
    }
}
```

## Configuration

```toml
[await_holding_unsendable]
# Additional fully-qualified type paths to flag beyond the built-in defaults.
# additional_types = ["my_crate::ConnectionGuard"]

# If true, disables the built-in default types and uses only `additional_types`.
# skip_default_types = false
```

| Field | Type | Default | Description |
|---|---|---|---|
| `additional_types` | `Vec<String>` | `[]` | Extra type paths to flag when held across `.await` |
| `skip_default_types` | `bool` | `false` | If `true`, only `additional_types` are checked |

## Relationship to Clippy lints

This lint **complements** Clippy — keep both enabled, no duplicates:

| Clippy lint | Covers | This lint adds |
|---|---|---|
| `await_holding_lock` | `std::sync` guards + parking_lot (via diagnostic items) | — (defers to Clippy) |
| `await_holding_refcell_ref` | `std::cell::Ref` / `RefMut` | — (defers to Clippy) |
| `await_holding_invalid_type` | User-configured types (requires clippy.toml) | parking_lot, tracing, crossbeam, rusqlite, r2d2 out of the box; configurable via dylint.toml |

Clippy uses diagnostic-item matching (robust, zero-config). This lint uses `def_path_str` matching (less robust but covers third-party types Clippy has no diagnostic items for).

## Relation to nightly `#[must_not_suspend]`

Rust nightly has an experimental `#[must_not_suspend]` attribute (RFC 3014, tracking issue #87521). When stabilized, library authors will annotate their types directly and the compiler will enforce the constraint. Until then — and it has been unstable since 2021 — this lint provides the same protection on stable Rust with a configurable type list. Migration is straightforward: as upstream libraries adopt the attribute, remove those types from `additional_types`.

## Relation to other lints

| Lint | Catches |
|---|---|
| `await_holding_unsendable` | Third-party guard types held across `.await` — complements Clippy's `await_holding_*` |
| `blocking_in_async` | Blocking calls (not guards) that stall the executor |
| `unsafe_send_missing_drop` | `unsafe impl Send` without `Drop` — unsound cross-thread destruction |
| `panic_in_drop` | Panicking inside `Drop` — abort during unwinding |
