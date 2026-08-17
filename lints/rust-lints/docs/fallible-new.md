# `fallible_new`

**Level:** `deny`

Warns when a `fn new()` constructor contains operations that can panic, suggesting it return `Result` or be renamed to convey fallibility.

## Why

Rust convention is that `fn new()` is an infallible constructor. Callers assume it will not panic:

- **Surprising panics** — a constructor that calls `.unwrap()` or `.expect()` can abort the program in cases the caller has no opportunity to handle.
- **Impossible to recover** — unlike a `Result`, a panic in `new()` cannot be caught with `?` or matched on. The only option is `catch_unwind`, which is not idiomatic.
- **Breaks composability** — library consumers cannot wrap fallible construction in their own error handling without risking a panic in their process.

Return `Result<Self, E>` (and optionally rename to `try_new` / `try_new_*` for variants), or move the fallible work out of the constructor.

### Relation to Clippy

Clippy has `fallible_impl_from` (nursery) which catches `unwrap`/`panic!` inside `impl From`, but nothing for `fn new()`. Clippy also has blanket `unwrap_used` / `expect_used` / `panic` restriction lints, but those fire everywhere and are not constructor-specific. This lint targets the specific convention violation of a panicking `new()`.

## Flagged expressions

The lint fires when the body of `fn new(...)` (or `fn new_*()` variants) contains any of:

| Expression | Notes |
|---|---|
| `.unwrap()` | On `Result` or `Option` |
| `.expect("...")` | On `Result` or `Option` |
| `panic!(...)` | Direct panic |
| `unreachable!(...)` | Logically equivalent to panic |

`todo!()` and `unimplemented!()` are intentionally omitted. rustc's `todo` and `unimplemented` lints (typically `deny`) already flag them; this lint focuses on surprising runtime failures in otherwise infallible constructors.

## Examples

### Triggers

```rust
impl Config {
    pub fn new(path: &str) -> Self {
        //~^ ERROR: constructor `new` can panic
        let contents = std::fs::read_to_string(path).unwrap();
        toml::from_str(&contents).expect("invalid config")
    }
}
```

```rust
impl DbPool {
    pub fn new(url: &str) -> Self {
        //~^ ERROR: constructor `new` can panic
        let conn = Connection::connect(url).unwrap();
        Self { conn }
    }
}
```

### Does not trigger

```rust
// Returns Result — callers can handle failure
impl Config {
    pub fn new(path: &str) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }
}

// No fallible operations
impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

// Named `try_new` — the name signals fallibility
impl Server {
    pub fn try_new(addr: &str) -> Result<Self, io::Error> {
        let listener = TcpListener::bind(addr)?;
        Ok(Self { listener })
    }
}

// Private constructor — opt out with #[expect]
struct Inner;
#[expect(fallible_new)]
impl Inner {
    fn new() -> Self {
        // unwrap here is an internal invariant; #[expect] documents the intent
        let val = GLOBAL.lock().unwrap();
        Self
    }
}
```

## Configuration

```toml
[fallible_new]
# Also check `new_*` variant constructors (e.g. `new_with_capacity`)
check_new_variants = true
```

| Field | Type | Default | Description |
|---|---|---|---|
| `check_new_variants` | `bool` | `true` | Also lint `fn new_*()` methods, not just `fn new()` |

