# `proper_error_type`

**Level:** `warn`

Flags error types in public APIs that are incomplete, unstructured, or missing error chain information.

## Why

Error handling is a contract between a function and its callers. In large crates, `pub(crate)` functions and types form an internal API surface with many consumers who face the same problems as external callers — they cannot match on variants, compose errors with `?`, or get useful error chains in logs. This lint therefore covers both `pub` and `pub(crate)` visibility (see [Skip conditions](#skip-conditions) for exceptions).

When that contract is incomplete:

- **Unstructured errors** — `String`, `&str`, and `Box<dyn Error>` discard type information. Callers cannot distinguish failure modes without parsing text, and any wording change silently breaks them.
- **Broken error chains** — manual `impl Error` often omits `source()`, preventing logging frameworks and `anyhow`/`eyre` reporters from walking the causal chain.
- **Duplicated sources** — when `Display` renders an inner error that `source()` also returns, error reporters print the same message twice. The [convention][std-error]: return it via `source()` *or* render it in `Display`, not both.
- **Misleading types** — a type named `FooError` that does not implement `std::error::Error` cannot be used with `Box<dyn Error>`, `?` conversion via `From`, or error reporters.
- **Avoidable boilerplate** — hand-written `Display` + `Error` impls drift out of sync with enum variants. `thiserror` eliminates this class of bug.

This lint does not enforce naming conventions (e.g., `config::Error` vs. `config::ConfigError`). See the [Rust API Guidelines][api-naming] on module-name stuttering.

[std-error]: https://doc.rust-lang.org/std/error/trait.Error.html
[api-naming]: https://rust-lang.github.io/api-guidelines/naming.html

### Relation to Clippy

No existing Clippy lint covers this space:

- `clippy::result_unit_err` — flags `Result<T, ()>`, not structural problems with the error type.
- `clippy::result_large_err` — flags error types that are large by size, not by correctness.
- `clippy::error_impl_error` — flags types *named* `Error` that implement `Error` (naming ambiguity).
- `clippy::map_err_ignore` — catches `.map_err(|_| ...)`, a related but distinct pattern.

## Steps

### Step 1 — Unstructured error types

Flags `pub` and `pub(crate)` functions returning `Result<T, E>` where `E` is `String`, `&str`, `Cow<'_, str>`, or `Box<dyn Error>` (including `Box<dyn Error + Send + Sync>`).

Also flags `anyhow::Error` and `miette::Report` in effectively public signatures — items reachable from the crate root per `tcx.effective_visibilities()`. These types are designed for application-internal use, so they are acceptable in `pub(crate)` and narrower functions but not on library API surfaces.

```rust
// Triggers
pub fn parse(input: &str) -> Result<Config, String> { .. }
//~^ WARNING: public function returns `Result<_, String>` — use a type that implements `Error`

// Triggers
pub fn run(cmd: &str) -> Result<(), Box<dyn Error>> { .. }
//~^ WARNING: public function returns `Result<_, Box<dyn Error>>` — use a type that implements `Error`

// Triggers — pub(crate) with unstructured error
pub(crate) fn load_config(path: &Path) -> Result<Config, String> { .. }
//~^ WARNING: `pub(crate)` function returns `Result<_, String>` — use a type that implements `Error`

// Triggers — anyhow/miette in an effectively public function
pub fn load(path: &Path) -> anyhow::Result<Config> { .. }
//~^ WARNING: effectively public function returns `anyhow::Error` — use a typed error
pub fn check(input: &str) -> miette::Result<()> { .. }
//~^ WARNING: effectively public function returns `miette::Report` — use a typed error
```

```rust
// OK — anyhow in a pub(crate) function (designed for application-internal use)
pub(crate) fn helper() -> anyhow::Result<()> { .. }

// OK — binary entry point
fn main() -> anyhow::Result<()> { .. }

// OK — typed error
pub fn parse(input: &str) -> Result<Config, ParseError> { .. }

// OK — pub(super) or narrower visibility
pub(super) fn helper() -> Result<(), String> { .. }

// OK — private function
fn helper() -> Result<(), String> { .. }
```

### Step 2 — Missing `source()`

Flags manual `impl Error` blocks that do not override `source()` when the type has fields that implement `Error`. Applies to both `pub` and `pub(crate)` types.

```rust
// Triggers
pub enum ConfigError { Io(io::Error) }
impl std::error::Error for ConfigError {}
//~^ WARNING: `ConfigError` wraps error types but does not implement `source()`

// Triggers — pub(crate) type
pub(crate) enum InternalError { Io(io::Error) }
impl std::error::Error for InternalError {}
//~^ WARNING: `InternalError` wraps error types but does not implement `source()`
```

```rust
// OK — source() implemented
impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self { Self::Io(e) => Some(e) }
    }
}

// OK — no fields that implement Error
pub enum ConfigError { MissingField(&'static str) }
impl std::error::Error for ConfigError {}

// OK — thiserror
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("io failed")]
    Io(#[from] io::Error),
}
```

### Step 3 — Duplicated source in `Display`

Flags `Display` impls that render an inner error also returned by `source()`. Error reporters already print each `source()` level, so duplicating it in `Display` produces double output.

```rust
// Triggers — Display renders `e`, source() also returns `e`
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self { Self::Io(e) => write!(f, "config error: {e}") }
        //~^ WARNING: `Display` renders inner error that is also returned by `source()`
    }
}
impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self { Self::Io(e) => Some(e) }
    }
}
```

```rust
// OK — Display describes this level only
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self { Self::Io(_) => write!(f, "failed to read config file") }
    }
}

// OK — thiserror
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("failed to read config file")]
    Io(#[source] io::Error),
}
```

### Step 4 — Manual `Error` + `Display` — use `thiserror`

Flags types where both `Error` and `Display` are implemented by hand.

```rust
// Triggers
pub enum ConfigError {
    Parse(toml::de::Error),
    Io(io::Error),
}
impl std::fmt::Display for ConfigError { .. }
impl std::error::Error for ConfigError { .. }
//~^ WARNING: manual `Error` + `Display` impl — use `#[derive(thiserror::Error)]`
```

```rust
// OK — thiserror
#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("parse error")]
    Parse(#[from] toml::de::Error),
    #[error("io error")]
    Io(#[from] io::Error),
}
```

### Step 5 — `*Error` types without `Error` impl

Flags `pub` and `pub(crate)` types whose name ends in `Error` or `Err` that do not implement `std::error::Error`.

```rust
// Triggers
pub enum ParseError {
    //~^ WARNING: `ParseError` is named as an error type but does not implement `std::error::Error`
    InvalidSyntax,
    UnexpectedEof,
}

// Triggers
pub struct ConnectionError { pub message: String, pub code: u32 }
//~^ WARNING: `ConnectionError` is named as an error type but does not implement `std::error::Error`

// Triggers — pub(crate) type
pub(crate) enum InternalError { Oops }
//~^ WARNING: `InternalError` is named as an error type but does not implement `std::error::Error`
```

```rust
// OK — implements Error (via thiserror or manually)
#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("invalid syntax")]
    InvalidSyntax,
}

// OK — not named *Error
pub enum ParseProblem { InvalidSyntax }

// OK — pub(super) or narrower visibility
pub(super) enum LocalError { Oops }

// OK — private type
enum InternalError { Oops }
```

## Skip conditions

| Condition | Reason |
|---|---|
| `span.from_expansion()` | Macro-generated code |
| `pub(super)` or narrower visibility (steps 1, 2, 5) | Local enough that callers can coordinate refactors directly |
| `pub(crate)` + `anyhow`/`miette` (step 1) | These types are designed for application-internal use |
| Not effectively public + `anyhow`/`miette` (step 1) | Acceptable in binaries and internal code |
| Trait impl methods | Signature dictated by the trait |
| `#[cfg(test)]` modules | Test helpers commonly use informal error types |
| `fn main()` | Entry points commonly use `anyhow::Result` |
| `#[derive(thiserror::Error)]` (steps 2–5) | thiserror handles correctness — proc-macro generated impls have expansion spans, so `span.from_expansion()` skips them |
| No fields implementing `Error` (step 2) | No source to chain |
| `#[error(transparent)]` (step 3) | Intentionally forwards both `Display` and `source()` — handled implicitly since generated impls are from expansion |

