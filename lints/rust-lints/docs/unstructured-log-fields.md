# `unstructured_log_fields`

**Level:** `warn`

Flags `tracing` macro invocations (`info!`, `warn!`, `debug!`, `error!`, `trace!`) where all captured values are positional format arguments and none are structured key-value fields.

## Why

Tracing's power comes from structured fields — they enable filtering, indexing, and machine-readable logs. When every value is interpolated into a format string (`tracing::info!("user {} path {}", user_id, path)`), that structure is lost: the values are baked into an opaque string, invisible to subscribers and query tools.

- **Unqueryable** — `user_id=42` in a format string can't be filtered by a `tracing` subscriber or log aggregator without regex.
- **Inconsistent** — mixed structured/unstructured logging in a codebase makes it unclear which fields are available for querying.
- **Easy to fix** — `tracing::info!(user_id, path, "user hit endpoint")` is the same number of tokens and strictly more useful.

### Relation to Clippy

No Clippy lint covers this. Clippy doesn't inspect tracing macro semantics.

## Examples

### Triggers

```rust
fn handle_request() {
    let user_id = 42;
    let path = "/api/v1";
    tracing::info!("user {} hit {}", user_id, path);
    //~^ WARNING: `tracing::info!` uses format args instead of structured fields
}
```

```rust
fn process() {
    let count = 10;
    tracing::warn!("processed {} items", count);
    //~^ WARNING: `tracing::warn!` uses format args instead of structured fields
}
```

```rust
fn debug_request() {
    let req = "GET /";
    tracing::debug!("request: {:?}", req);
    //~^ WARNING: `tracing::debug!` uses format args instead of structured fields
}
```

### Does not trigger

```rust
// Structured fields (bare identifier shorthand)
tracing::info!(user_id, path, "user hit endpoint");

// Debug field shorthand
tracing::debug!(?req, "incoming request");

// Display field shorthand
tracing::info!(%addr, "listening");

// Explicit key-value
tracing::info!(user_id = user_id, "processing");

// Mixed structured + positional — at least one structured field present,
// partial improvement is acceptable
tracing::info!(user_id, "user performed {}", action);

// Bare message with no captures — nothing to structure
tracing::info!("server started");

// Non-tracing macros (log crate has no structured field support)
log::info!("user {}", user_id);

// Suppressed with #[allow]
#[allow(unstructured_log_fields)]
fn allowed() {
    tracing::info!("user {}", 42);
}
```

