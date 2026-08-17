# `panic_in_drop`

**Level:** `deny`

Flags `Drop` implementations that contain operations which can panic, since panicking during unwinding causes an immediate abort.

## Why

If a panic occurs while the runtime is already unwinding from a previous panic, Rust aborts the process immediately — no destructors run, no cleanup happens:

- **Double-panic abort** — a `.unwrap()` inside `Drop` works fine in normal code paths, but if the value is dropped during stack unwinding from an unrelated panic, the process aborts with no backtrace or error message.
- **Silent data loss** — destructors that flush buffers, close connections, or write checkpoints silently fail to complete. The abort prevents any subsequent destructors from running too.
- **Extremely hard to debug** — the abort happens at the OS level. You get a signal (SIGABRT) with no indication of which `Drop` impl caused it or what the original panic was about.
- **Contagious** — one bad `Drop` impl can take down an entire server. Libraries that panic in `Drop` are unsafe to use in any context that catches panics (e.g. `catch_unwind`, thread pool workers, test harnesses).

Handle errors in `Drop` gracefully: log them, ignore them, or store them for later retrieval. If cleanup is critical and fallible, provide an explicit `close()` / `flush()` method that callers invoke before dropping.

### Relation to Clippy

Clippy does not have a lint for this. The general restriction lints `unwrap_used`, `expect_used`, and `panic` fire everywhere and are not `Drop`-specific. There is no lint that specifically targets the dangerous combination of panicking code inside a destructor.

## Flagged expressions

The lint fires when the body of a `Drop::drop` implementation contains any of:

| Expression | Notes |
|---|---|
| `.unwrap()` | On `Result` or `Option` |
| `.expect("...")` | On `Result` or `Option` |
| `panic!(...)` | Direct panic |
| `unreachable!(...)` | Logically equivalent to panic |
| `assert!(...)` | Panics on failure |
| `assert_eq!(...)` / `assert_ne!(...)` | Panics on failure |

`todo!()` and `unimplemented!()` are intentionally omitted. rustc's `todo` and `unimplemented` lints (typically `deny`) already flag them; this lint focuses on surprising runtime failures in `Drop` impls.

## Examples

### Triggers

```rust
impl Drop for TempFile {
    fn drop(&mut self) {
        //~^ ERROR: panic-able expression in `Drop` impl — this will abort during unwinding
        std::fs::remove_file(&self.path).unwrap();
    }
}
```

```rust
impl Drop for Connection {
    fn drop(&mut self) {
        //~^ ERROR: panic-able expression in `Drop` impl — this will abort during unwinding
        self.socket.shutdown(Shutdown::Both).expect("shutdown failed");
    }
}
```

```rust
impl Drop for Checkpoint {
    fn drop(&mut self) {
        //~^ ERROR: panic-able expression in `Drop` impl — this will abort during unwinding
        assert!(self.flushed, "dropped without flushing");
    }
}
```

### Does not trigger

```rust
// Errors are silently ignored — safe during unwinding
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

// Errors are logged but not propagated
impl Drop for Connection {
    fn drop(&mut self) {
        if let Err(e) = self.socket.shutdown(Shutdown::Both) {
            eprintln!("warning: shutdown failed: {e}");
        }
    }
}

// Uses if-let instead of unwrap
impl Drop for Pool {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            handle.join().ok();
        }
    }
}

// Normal code — not inside Drop
impl Processor {
    fn process(&self) {
        let data = self.load().unwrap(); // fine here
    }
}

// Guarded by std::thread::panicking() — safe
impl Drop for GuardedDrop {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.val.unwrap(); // only runs when not already unwinding
        }
    }
}

// Panic inside a closure — doesn't run during drop itself
impl Drop for Deferred {
    fn drop(&mut self) {
        // Closures and async blocks are not traversed — a panic stored
        // in a closure won't execute as part of the drop call.
        let _ = self.callback.take();
    }
}
```

## Exceptions

The lint does not fire on:

- **`std::thread::panicking()` guards** — if the body of `drop()` checks `std::thread::panicking()` (or its negation), both branches are suppressed. The author has already handled the double-panic scenario.
- **Closures and async blocks** — panic-able expressions inside a closure or async block within `drop()` are not flagged, because they don't execute as part of the drop call itself.

## Configuration

No additional configuration.

## Relation to other lints

This pairs well with `fallible_new` — together they cover the two most dangerous places to panic: constructors (surprising callers) and destructors (aborting the process).
