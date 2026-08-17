# `map_init_then_insert`

**Level:** `warn`

Warns when a `HashMap`, `BTreeMap`, `IndexMap`, `FxHashMap`, `AHashMap`, or similar map is created empty and then immediately populated with sequential `.insert()` calls, suggesting `::from([...])` instead.

## Why

Sequential `.insert()` calls after construction are verbose and miss optimizations:

- **Readability** — a `from` literal makes the intended contents visible at a glance, similar to `vec![...]` vs repeated `.push()`.
- **Missed capacity** — `HashMap::new()` followed by *n* inserts causes repeated resizing. `HashMap::from([(k, v), ...])` can allocate once.
- **Redundant mutability** — the sequential pattern requires `let mut`, while the `from` pattern yields an immutable binding.

### Relation to Clippy

Clippy has `vec_init_then_push` (perf) which catches the analogous pattern for `Vec::new()` followed by `.push()` calls. There is no equivalent for map types. Clippy's `map_entry` lint is different — it catches redundant `contains_key` + `insert` sequences, not init-then-insert patterns.

## Flagged patterns

The lint fires when all of the following hold:

1. A `HashMap`, `BTreeMap`, `IndexMap`, `FxHashMap`, `AHashMap`, or other recognized map is created via `::new()`, `::default()`, or `::with_capacity(n)`
2. The immediately following statements are all `.insert(k, v)` calls on the same binding
3. There are no intervening reads, borrows, or control flow between creation and the insert sequence

| Constructor | Suggested replacement |
|---|---|
| `HashMap::new()` + inserts | `HashMap::from([...])` |
| `BTreeMap::new()` + inserts | `BTreeMap::from([...])` |
| `IndexMap::new()` + inserts | `IndexMap::from([...])` |
| `FxHashMap::default()` + inserts | `FxHashMap::from([...])` |
| `AHashMap::new()` + inserts | `AHashMap::from([...])` |
| `::with_capacity(n)` + inserts | `::from([...])` (capacity is implicit) |

**Note:** The suggestion always uses the name the user wrote. `FxHashMap::from([...])` and `AHashMap::from([...])` are valid because both are type aliases of `HashMap` and inherit its `From<[(K, V); N]>` impl. If you want to disallow the `indexmap!` macro entirely, use the `disallowed_macros` lint.

## Examples

### Triggers

```rust
let mut m = HashMap::new();
//~^ WARNING: immediately inserting into a newly created map
m.insert("a", 1);
m.insert("b", 2);
m.insert("c", 3);
// suggest: let m = HashMap::from([("a", 1), ("b", 2), ("c", 3)]);
```

```rust
let mut m = BTreeMap::new();
//~^ WARNING: immediately inserting into a newly created map
m.insert(1, "one");
m.insert(2, "two");
// suggest: let m = BTreeMap::from([(1, "one"), (2, "two")]);
```

### Does not trigger

```rust
// Only one insert — not worth linting
let mut m = HashMap::new();
m.insert("a", 1);
```

```rust
// Intervening statement breaks the sequence
let mut m = HashMap::new();
m.insert("a", 1);
println!("inserted a");
m.insert("b", 2);
```

```rust
// Control flow between creation and inserts
let mut m = HashMap::new();
if condition {
    m.insert("a", 1);
}
```

```rust
// Map is read between inserts
let mut m = HashMap::new();
m.insert("a", 1);
let v = m.get("a");
m.insert("b", 2);
```

```rust
// Already constructed with `from`
let m = HashMap::from([("a", 1), ("b", 2)]);
```

## Configuration

None. Use `#[allow(map_init_then_insert)]` to suppress on a case-by-case basis, matching Clippy's `vec_init_then_push` approach.

## Scope: which map types?

| Type | Detected via | Included? |
|---|---|---|
| `std::collections::HashMap` | `is_type_diagnostic_item(sym::HashMap)` | ✅ |
| `std::collections::BTreeMap` | `is_type_diagnostic_item(sym::BTreeMap)` | ✅ |
| `indexmap::IndexMap` | Type path matching | ✅ |
| `rustc_hash::FxHashMap` | Type alias of `HashMap` — caught by diagnostic item | ✅ |
| `ahash::AHashMap` | Explicit crate + type name matching | ✅ |
| `hashbrown::HashMap` | — | ❌ — diagnostic item is on std's `HashMap` only; direct hashbrown usage is rare |
| `dashmap::DashMap` | — | ❌ — concurrent map, different semantics |
| `FxHashSet` / `AHashSet` / `HashSet` | — | ❌ — sets, not maps; fit a `set_init_then_insert` lint |

