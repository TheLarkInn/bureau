# `suggest_builder`

**Level:** `warn`

Suggests adding a `#[builder]` constructor in a `#[bon] impl` for structs with many fields, enabling named setters at call sites.

## Why

Structs with many fields are easy to misuse at construction time:

- **Unreadable constructors** — `Config { host, port, timeout, retries, tls }` gives little help at the call site.
- **Easy to miss optional fields** — every field has to be supplied up front, even when some values have sensible defaults.
- **Painful to extend** — adding a field means updating every constructor site.

A `#[bon] impl` with a `#[builder]` constructor turns a struct into a builder-backed API, so callers can write `Config::builder().host("localhost").port(5432).tls(true).build()`.

## Examples

### Triggers

```rust
struct Config {
    host: String,
    port: u16,
    timeout: u64,
    retries: u32,
    tls: bool,
    name: String,
}
//~^ WARNING: struct `Config` has 6 fields; consider exposing a `#[builder]` constructor
```

### Does not trigger

```rust
// Already has `#[derive(bon::Builder)]`
#[derive(bon::Builder)]
struct Config {
    host: String,
    port: u16,
    timeout: u64,
    retries: u32,
    tls: bool,
    name: String,
}

// Below threshold (default 6)
struct Point {
    x: i32,
    y: i32,
}

// Named *Builder structs are skipped
struct ConfigBuilder {
    host: String,
    port: u16,
    timeout: u64,
    retries: u32,
    tls: bool,
    mode: String,
}
```

Structs with `Default` (derived or manual), `#[repr(C)]`, or lifetime parameters are also not considered. `PhantomData` fields are excluded from the field count since they are not real from a construction-ergonomics standpoint.

## Configuration

```toml
[suggest_builder]
threshold = 6
skip_derives = ["Default", "Queryable", "Insertable", "Selectable"]
```

| Field | Type | Default | Description |
|---|---|---|---|
| `threshold` | `usize` | `6` | Minimum field count to trigger the lint |
| `skip_derives` | `Vec<String>` | `["Default", "Queryable", "Insertable", "Selectable"]` | Derive names that exempt a struct from this lint |
