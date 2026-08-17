# `result_result`

**Level:** `warn`

Flags `Result<Result<T, E1>, E2>` types in function signatures and type aliases, which are almost always a mistake or can be simplified.

## Why

A nested `Result<Result<T, E1>, E2>` forces callers into awkward double-matching:

- **Double unwrap** — callers must handle two layers of error: `match result { Ok(Ok(v)) => ..., Ok(Err(e1)) => ..., Err(e2) => ... }`. This is tedious and easy to get wrong.
- **Usually a bug** — the nested `Result` often comes from accidentally calling `.map()` with a fallible closure instead of `.and_then()`, producing an unintended extra layer.
- **Hides error flow** — readers have to reason about which layer an error came from. A unified error enum or `and_then` chain is clearer.

Fixes:

1. Replace `.map(fallible_fn)` with `.and_then(fallible_fn)` to flatten the result.
2. Unify the two error types into a single enum and return `Result<T, UnifiedError>`.
3. Use `?` to propagate the inner error.

### Relation to Clippy

Clippy has `option_option` (pedantic) which catches `Option<Option<T>>`, but there is **no equivalent for nested `Result`**. This lint fills that gap.

## Examples

### Triggers

```rust
// Nested Result in return type
fn parse_and_validate(input: &str) -> Result<Result<Config, ValidationError>, ParseError> {
    //~^ WARNING: nested `Result<Result<_, _>, _>` — consider flattening into a single Result
    let parsed = parse(input)?;
    Ok(validate(parsed))
}
```

```rust
// Nested Result in type alias
type LoadResult = Result<Result<Data, DecodeError>, io::Error>;
    //~^ WARNING: nested `Result<Result<_, _>, _>` — consider flattening into a single Result
```

```rust
// Produced by .map() with a fallible closure
fn load(path: &str) -> Result<Result<Config, toml::de::Error>, io::Error> {
    //~^ WARNING: nested `Result<Result<_, _>, _>` — consider flattening into a single Result
    std::fs::read_to_string(path).map(|s| toml::from_str(&s))
}
```

### Does not trigger

```rust
// Flat Result with unified error
fn parse_and_validate(input: &str) -> Result<Config, AppError> {
    let parsed = parse(input)?;
    let config = validate(parsed)?;
    Ok(config)
}

// Using .and_then() flattens the Result
fn load(path: &str) -> Result<Config, Box<dyn Error>> {
    std::fs::read_to_string(path)
        .map_err(|e| e.into())
        .and_then(|s| toml::from_str(&s).map_err(|e| e.into()))
}

// Result with a non-Result Ok type
fn fetch(url: &str) -> Result<String, reqwest::Error> {
    // ...
}

// Deeply generic code where T happens to be a Result at some call site
// is not flagged — only explicit Result<Result<_, _>, _> in signatures
fn wrap<T>(value: T) -> Result<T, Error> {
    Ok(value)
}
```

## Exceptions

The lint does not fire on:

- **Trait impl methods** — the return type is dictated by the trait definition; the implementor cannot change it.
- **Trait definition method signatures** — flagging the trait would be redundant since every implementor must match.
- **Closures** — closure return types are inferred and not part of a public API.
- **Macro-generated code** — skipped to avoid false positives from macros the user did not write.

## Configuration

No additional configuration. The lint fires on `Result<Result<_, _>, _>` in function signatures and type aliases, subject to the exceptions above.
