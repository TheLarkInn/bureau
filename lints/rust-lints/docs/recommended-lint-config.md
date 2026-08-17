# Recommended Lint Configuration

Clippy and dylint setup for production Rust codebases. Copy the [Setup](#setup) configurations, then use the [Lint Reference](#lint-reference) to calibrate levels for your team.

## Setup

**1. Add workspace lints to `Cargo.toml`:**

```toml
[workspace.lints.rust]
unsafe_code     = "forbid"  # or "deny" if your project has legitimate unsafe blocks
unused_must_use = "deny"
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(dylint_lib, values("rust_lints"))'] }

[workspace.lints.clippy]
# Enable opt-in lint groups — individual entries below override these.
pedantic = { level = "warn", priority = -1 }
nursery  = { level = "warn", priority = -1 }

# --- Deny: always wrong in production ---
await_holding_lock            = "deny"
await_holding_refcell_ref     = "deny"
let_underscore_future         = "deny"
rc_mutex                      = "deny"
exit                          = "deny"
todo                          = "deny"
unimplemented                 = "deny"
undocumented_unsafe_blocks    = "deny"
multiple_unsafe_ops_per_block = "deny"

# --- Warn: usually wrong; suppression requires a documented reason ---

# Error handling discipline
unwrap_used               = "warn"
expect_used               = "warn"
panic_in_result_fn        = "warn"
unwrap_in_result          = "warn"
map_err_ignore            = "warn"

# Type safety & correctness
indexing_slicing          = "warn"
panic                     = "warn"
option_option             = "warn"
cast_possible_truncation  = "warn"
cast_sign_loss            = "warn"
cast_possible_wrap        = "warn"
float_cmp                 = "warn"

# Async & concurrency
large_futures             = "warn"
mutex_atomic              = "warn"

# Code quality & maintainability
wildcard_enum_match_arm   = "warn"
clone_on_ref_ptr          = "warn"
unused_result_ok          = "warn"
let_underscore_must_use   = "warn"
fn_params_excessive_bools = "warn"
match_same_arms           = "warn"
match_wildcard_for_single_variants = "warn"

# Documentation & design
missing_errors_doc        = "warn"
missing_panics_doc        = "warn"
missing_fields_in_debug   = "warn"
return_self_not_must_use  = "warn"
should_panic_without_expect = "warn"
allow_attributes          = "forbid"
allow_attributes_without_reason = "deny"
ignore_without_reason     = "deny"

# Performance & idioms
unnecessary_wraps         = "warn"
manual_let_else           = "warn"
default_trait_access      = "warn"
format_push_string        = "warn"
unreadable_literal        = "warn"

# File & I/O operations
create_dir                = "warn"
verbose_file_reads        = "warn"
pathbuf_init_then_push    = "warn"

# Resource management
mem_forget                = "warn"
rc_buffer                 = "warn"
large_include_file        = "warn"

# Miscellaneous
error_impl_error          = "warn"
cfg_not_test              = "warn"
missing_assert_message    = "warn"
tests_outside_test_module = "warn"
ref_option                = "warn"
large_types_passed_by_value = "warn"
unsafe_derive_deserialize = "warn"
large_stack_arrays        = "warn"
doc_link_with_quotes      = "warn"
copy_iterator             = "warn"
macro_use_imports         = "warn"

# Disabled: enabled by group but too noisy or harmful
module_name_repetitions   = "allow"
option_if_let_else        = "allow"
similar_names             = "allow"
struct_excessive_bools    = "allow"
struct_field_names        = "allow"
too_many_lines            = "allow"
```

**2. Add `clippy.toml` to the workspace root:**

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

**3. Each crate inherits with:**

```toml
# <crate>/Cargo.toml
[lints]
workspace = true
```

---

## Lint Reference

`pedantic` and `nursery` are enabled at `priority = -1` so every lint in these groups defaults to warn. Individual entries at default priority override the group level.

**Deny** — always wrong in production; no valid exception. **Warn** — usually wrong; suppression requires a documented reason (enforced by `allow_attributes_without_reason`).

### `disallowed_types`

Add to `clippy.toml`. Fires when the banned type appears in any position: use declarations, struct fields, function signatures, or local bindings. Catches structural choices that call-site lints miss.

**Synchronization**

| Path | Reason | Prerequisite |
|---|---|---|
| `std::sync::Mutex` | Poisons on panic; callers write `.lock().unwrap()`, defeating the error signal; `parking_lot::Mutex` is faster and non-poisoning | `parking_lot` |
| `std::sync::RwLock` | Same poisoning problem; prone to writer starvation on Linux | `parking_lot` |
| `std::sync::Condvar` | Must pair with `std::sync::Mutex`; if Mutex is banned, Condvar follows | `parking_lot` |
| `parking_lot::ReentrantMutex` | Reentrant locking hides recursive-lock bugs; redesign the call graph instead | — |

**Collections**

| Path | Reason | Prerequisite |
|---|---|---|
| `std::collections::LinkedList` | Heap-allocates every node; cache-hostile; `VecDeque` covers all practical use cases | — |
| `std::collections::HashMap` | Nondeterministic iteration order causes flaky tests and unstable serialized output; default SipHash is slower than alternatives for internal keys | team consensus on replacement |
| `std::collections::HashSet` | Same as `HashMap` | team consensus on replacement |
| `std::collections::hash_map::RandomState` | Closes the loophole of constructing a banned `HashMap` via `with_hasher(RandomState::new())` | ban `HashMap` first |

**Async and channels**

| Path | Reason | Prerequisite |
|---|---|---|
| `std::sync::mpsc::Sender` | Single-producer only, always unbounded, historically buggy; crossbeam-channel is strictly better | crossbeam or tokio |
| `std::sync::mpsc::Receiver` | Same as `Sender` | crossbeam or tokio |
| `futures::lock::Mutex` | Does not integrate with tokio's scheduler; wakeup behavior differs from `tokio::sync::Mutex` | tokio codebase |
| `futures::channel::mpsc::Sender` | Same scheduler mismatch; use `tokio::sync::mpsc` | tokio codebase |
| `reqwest::blocking::Client` | Creates its own tokio runtime; panics with "cannot start a runtime from within a runtime" inside tokio | tokio + reqwest |

**Portability**

| Path | Reason | Prerequisite |
|---|---|---|
| `std::sync::atomic::AtomicU64` | Not available on all 32-bit targets; use `AtomicU32` or `Mutex<u64>` | targets include 32-bit |
| `std::sync::atomic::AtomicU128` | Same as `AtomicU64` | targets include 32-bit |

**Time**

| Path | Reason | Prerequisite |
|---|---|---|
| `std::time::Instant` | Bypasses tokio's clock — `tokio::time::pause()` and `advance()` do not affect it; use `tokio::time::Instant` for testable timeouts | tokio codebase |

**Filesystem**

`std::fs` errors omit the file path. `fs-err` wraps every error with the path and operation, turning `"No such file or directory (os error 2)"` into `"failed to open 'config.toml': No such file or directory (os error 2)"`. The same applies to `tokio::fs`.

| Path | Replacement | Prerequisite |
|---|---|---|
| `std::fs::File` | `fs_err::File` | `fs-err` |
| `std::fs::OpenOptions` | `fs_err::OpenOptions` | `fs-err` |
| `std::fs::DirEntry` | `fs_err::DirEntry` | `fs-err` |
| `std::fs::ReadDir` | `fs_err::ReadDir` | `fs-err` |
| `tokio::fs::File` | `fs_err::tokio::File` | `fs-err` + tokio feature |
| `tokio::fs::OpenOptions` | `fs_err::tokio::OpenOptions` | `fs-err` + tokio feature |

**Randomness**

| Path | Reason | Prerequisite |
|---|---|---|
| `rand::rngs::ThreadRng` | Holding `ThreadRng` in a struct makes code untestable; `hardcoded_randomness` catches call sites but misses struct fields — this closes that gap | — |

---

### `disallowed_methods`

Add to `clippy.toml`. Unlike `disallowed_types`, these fire only at call sites — suitable when the type is fine but a specific function has a better alternative or a known footgun.

**Environment mutation**

| Path | Reason |
|---|---|
| `std::env::set_var` | Process-global; concurrent reads from other threads — including libc DNS lookups inside `ToSocketAddrs` — cause data races; no safe use in multi-threaded code |
| `std::env::remove_var` | Same as `set_var` |
| `std::env::set_current_dir` | Process-global CWD; parallel tests race on the working directory; use absolute paths |
| `std::env::var` | Direct env reads bypass config layers and cannot be overridden in tests; route through a config struct |
| `std::env::var_os` | Same as `var` |
| `std::env::vars` | Same as `var` |
| `std::env::vars_os` | Same as `var` |
| `std::env::temp_dir` | Returns the platform temp path with no uniqueness guarantee — concurrent callers collide; `tempfile::tempdir()` creates a unique directory and removes it on drop |

**I/O**

| Path | Reason |
|---|---|
| `std::io::Write::write` | May do a partial write, returning `Ok(n)` where `n < buf.len()`; most callers silently discard `n` and lose bytes; `write_all` loops until all bytes are written |

**Threading**

| Path | Reason |
|---|---|
| `std::thread::spawn` | Spawns an unnamed thread that appears as `<unnamed>` in panic messages and profilers; use `Builder::new().name("…").spawn()` |
| `std::panic::catch_unwind` | Catching panics hides bugs; use `Result` for expected failures; legitimate uses (FFI boundaries, plugin isolation) should carry `#[expect]` with a reason |

**Numeric**

| Path | Reason |
|---|---|
| `f32::abs_sub` | Deprecated; computes `max(0.0, self - other)`, not `(self - other).abs()` — the name actively misleads |
| `f64::abs_sub` | Same as `f32::abs_sub` |

**Tokio task lifecycle** *(teams using `tokio-console`)*

| Path | Reason |
|---|---|
| `tokio::task::spawn` | Unnamed tasks appear as opaque IDs in tokio-console; use `tokio::task::Builder::new().name("…").spawn()` |
| `tokio::task::spawn_blocking` | Same as `tokio::task::spawn` |
| `tokio::runtime::Handle::spawn` | Same as `tokio::task::spawn` |

---

## Lint rationale

Why each workspace lint is included, grouped by concern.

### Unsafe code discipline

| Lint | Level | Why |
|---|---|---|
| `undocumented_unsafe_blocks` | deny | Requires `// SAFETY:` comment explaining soundness. |
| `multiple_unsafe_ops_per_block` | deny | One operation per block — each justification must be independent. |

### Error handling contracts

| Lint | Level | Why |
|---|---|---|
| `panic_in_result_fn` | warn | `panic!` inside `-> Result` should return `Err` instead — avoids silent panics in recoverable error contexts. |
| `unwrap_in_result` | warn | Same principle: `.unwrap()` in Result-returning functions converts recoverable errors to crashes. |
| `wildcard_enum_match_arm` | warn | Forces explicit variant matching — new variants from dependency updates won't silently fall through. |
| `unused_result_ok` | warn | `.ok()` called solely to silence `#[must_use]` hides that the error is being discarded. |
| `let_underscore_must_use` | warn | `let _ = must_use_expr` silently discards a value marked as important. |

### Type safety

| Lint | Level | Why |
|---|---|---|
| `cast_possible_truncation` | warn | `u64 as u8` silently drops bits — forces `u8::try_from()` or explicit `#[expect]`. |
| `cast_sign_loss` | warn | `-1i32 as u32` wraps to `u32::MAX` — same discipline. |
| `cast_possible_wrap` | warn | Completes the cast safety trio for production code. |
| `float_cmp` | warn | `==` on floats almost always wrong — use epsilon comparison or domain-specific equality. |
| `unsafe_derive_deserialize` | warn | `#[derive(Deserialize)]` on unsafe types can bypass invariants. |

### Code idioms & maintainability

| Lint | Level | Why |
|---|---|---|
| `manual_let_else` | warn | `if let Some(x) = ... { } else { return }` → `let Some(x) = ... else { return }` — modern Rust idiom. |
| `match_same_arms` | warn | Duplicate match arms usually indicate copy-paste errors. |
| `match_wildcard_for_single_variants` | warn | `_ =>` matching only one variant obscures intent. |
| `missing_fields_in_debug` | warn | Manual `Debug` that skips fields produces incomplete output after refactoring. |
| `return_self_not_must_use` | warn | Builder methods returning `Self` without `#[must_use]` let callers silently drop the result. |
| `unnecessary_wraps` | warn | Private functions always returning `Some`/`Ok` should return the inner type. |
| `ref_option` | warn | `&Option<T>` in signatures should be `Option<&T>` — better ergonomics, no double indirection. |

### I/O and resource management

| Lint | Level | Why |
|---|---|---|
| `create_dir` | warn | `std::fs::create_dir()` fails if any parent is missing — prefer `create_dir_all()`. |
| `verbose_file_reads` | warn | Prefer `fs::read_to_string()` over manual `File::open()` + `read_to_end()`. |
| `pathbuf_init_then_push` | warn | `PathBuf::new().push(...)` should be `.join()` or `PathBuf::from()`. |
| `mem_forget` | warn | `std::mem::forget()` on `Drop` types leaks resources — rarely needed. |
| `rc_buffer` | warn | `Arc<String>`/`Arc<Vec<T>>` wastes an indirection — use `Arc<str>`/`Arc<[T]>`. |
| `large_include_file` | warn | Prevents accidental binary bloat from embedding multi-MB files via `include_bytes!`. |

### Design & testing

| Lint | Level | Why |
|---|---|---|
| `error_impl_error` | warn | A type named `Error` implementing `Error` creates ambiguity — force specific names. |
| `cfg_not_test` | warn | `#[cfg(not(test))]` hides code from tests, inflating coverage. |
| `missing_assert_message` | warn | Bare `assert!` produces unhelpful panic messages — always include context. |
| `should_panic_without_expect` | warn | `#[should_panic]` without `expected = "..."` passes on *any* panic, not just the right one. |
| `tests_outside_test_module` | warn | `#[test]` functions belong in `#[cfg(test)]` modules for organizational clarity. |
| `allow_attributes` | forbid | Flags `#[allow]` — use `#[expect]` instead so stale suppressions become warnings automatically. Cannot be locally overridden. |
| `allow_attributes_without_reason` | deny | Every suppression must carry a `reason` — prevents silent lint bypasses and keeps `#[expect]` stale-suppression detection meaningful. |
| `ignore_without_reason` | deny | `#[ignore]` without rationale accumulates silently. |

### Performance & async

| Lint | Level | Why |
|---|---|---|
| `large_types_passed_by_value` | warn | Large `Copy` types passed by value cause unnecessary memcpys. |
| `format_push_string` | warn | `s.push_str(&format!(...))` allocates twice — use `write!(s, ...)` instead. |
| `unreadable_literal` | warn | `1000000` → `1_000_000` for readability. |
| `large_stack_arrays` | warn | Oversized local arrays may overflow the stack. |
| `clone_on_ref_ptr` | warn | `Arc::clone(&x)` is clearer than `x.clone()` — makes cheap pointer clone visually distinct. |

### Additional clippy lints

| Lint | Level | Why |
|---|---|---|
| `debug_assert_with_mut_call` | warn | Mutation inside `debug_assert!` disappears in release builds. |
| `fallible_impl_from` | warn | `From` implementations that panic should be `TryFrom` — enforces the conversion contract. |
| `significant_drop_in_scrutinee` | warn | `MutexGuard` in match scrutinee stays locked across all arms. |
| `significant_drop_tightening` | warn | Drop locks as soon as possible to reduce contention. |
| `future_not_send` | warn | Async functions returning non-`Send` futures break on multithreaded runtimes. |
| `non_send_fields_in_send_ty` | warn | `unsafe impl Send` on types with `Rc` fields — soundness hole and data race risk. |
| `collection_is_never_read` | warn | Collection built but never read is dead code or a logic bug. |
| `read_zero_byte_vec` | warn | `Vec::with_capacity(n)` + `read_to_end` reads zero bytes — need `resize` first. |
| `path_buf_push_overwrite` | warn | `buf.push("/absolute")` silently replaces the entire path. |
| `coerce_container_to_any` | warn | `&Box<T> as &dyn Any` downcasts to `Box<T>`, not `T` — almost never intended. |
| `literal_string_with_formatting_args` | warn | `println("hello {name}")` (missing `!`) — format args in non-format function. |
| `while_float` | warn | `while x < 1.0 { x += 0.1; }` accumulates precision errors — use special iterators instead. |
| `branches_sharing_code` | warn | Duplicate code across if/else branches should be hoisted. |
| `or_fun_call` | warn | `.unwrap_or(expensive_fn())` evaluates eagerly — use `.unwrap_or_else(closure)`. |
| `unused_peekable` | warn | `.peekable()` where `.peek()` is never called — leftover from refactoring. |

### Disabled lints

Lints enabled by the `pedantic`/`nursery` group but explicitly turned off.

| Lint | Group | Why disabled |
|---|---|---|
| `module_name_repetitions` | pedantic | Renaming types to drop the module prefix breaks public APIs and hurts readability when multiple types are imported (e.g., `FooError` vs bare `Error`). |
| `option_if_let_else` | nursery | Suggests `map_or_else` which is less readable for multi-line arms, side effects, or early returns. `if let`/`else` is idiomatic Rust. |
| `similar_names` | pedantic | Flags `req`/`res`, `item`/`items` — too many false positives to be useful. |
| `struct_excessive_bools` | pedantic | Noisy on config/options structs where multiple bools are the natural representation. |
| `struct_field_names` | pedantic | Shared prefixes/suffixes on fields are often intentional grouping, not redundancy. |
| `too_many_lines` | pedantic | Arbitrary 100-line threshold — useful signal but not always actionable. |

---

## Suppression discipline

`allow_attributes_without_reason` requires every suppression to carry `reason`:

```rust
#[expect(clippy::unwrap_used, reason = "index is bounds-checked by the constructor invariant")]
let val = self.items[self.cursor];
```

Prefer `#[expect]` over `#[allow]`. If the lint stops firing (because the code was fixed), `#[expect]` becomes a warning itself — stale suppressions do not accumulate:

```rust
#[expect(clippy::too_many_arguments, reason = "mirrors the shape of the external C API")]
pub fn configure(host: &str, port: u16, timeout: u64, retries: u32, tls: bool) {
    // ...
}
```

### Suppressing dylint lints

Dylint lints are loaded at runtime, so rustc does not recognise their names and reports `unknown lint`. To use `#[expect]` or `#[allow]` on a dylint lint without blanket-allowing all unknown lints, add a crate-level `cfg_attr` gated on the `dylint_lib` cfg that dylint sets when it runs:

```rust
// lib.rs / main.rs — only suppresses unknown_lints when dylint is *not* running,
// so typos in lint names are still caught during `cargo dylint`.
#![cfg_attr(not(dylint_lib = "rust_lints"), allow(unknown_lints))]
```

Then `#[expect]` works normally on individual items:

```rust
#[expect(suggest_builder, reason = "false positive: struct is not a builder")]
pub struct Config { /* ... */ }
```

The `unexpected_cfgs` entry in `[lints.rust]` (see [Setup](#setup)) tells rustc that `dylint_lib` is a valid cfg name.

---

## Pre-commit setup

Commit a hook script to the repository and point git at it:

```bash
# one-time setup per clone
git config core.hooksPath .githooks
```

### `.githooks/pre-commit`

```bash
#!/usr/bin/env bash
set -euo pipefail

# 1. Auto-fix idioms — always-correct machine-applicable fixes applied silently.
cargo clippy --all-targets --fix --allow-dirty --allow-staged -- \
  -W clippy::uninlined_format_args \
  -W clippy::redundant_closure_for_method_calls \
  -W clippy::manual_string_new \
  -W clippy::single_char_pattern \
  -W clippy::needless_raw_string_hashes \
  -W clippy::collapsible_else_if \
  -W clippy::redundant_else \
  -W clippy::range_plus_one \
  -W clippy::range_minus_one \
  -W clippy::semicolon_if_nothing_returned \
  -W clippy::manual_assert \
  -W clippy::assign_op_pattern \
  -W clippy::needless_for_each \
  -W clippy::manual_instant_elapsed

# 2. Check for debug remnants — runs after auto-fix so cleaned-up code is checked too.
# Suppress intentional cases with #[allow(debug_remnants, reason = "intentional CLI output")].
DYLINT_RUSTFLAGS="-W debug_remnants" cargo dylint --all -- --all-targets

# 3. Check for banned crates — catches additions at the point they enter Cargo.toml.
# Crate name must match exactly as it appears as a key in Cargo.toml.
declare -A BANNED=(
  [lazy_static]="use std::sync::LazyLock (Rust 1.80+)"
  [once_cell]="use std::sync::OnceLock / LazyLock (Rust 1.70+/1.80+)"
  [failure]="use thiserror for libraries, anyhow for applications"
  [dashmap]="use RwLock<HashMap> — DashMap deadlocks when a Ref is held across map calls"
  [openssl]="use rustls"
  [md5]="MD5 is cryptographically broken; use SHA-256 or SHA-3"
  [sha1]="SHA-1 collision resistance is broken; use SHA-256 or SHA-3"
)

rc=0
while IFS= read -r toml; do
  for crate in "${!BANNED[@]}"; do
    if grep -qE "^[[:space:]]*${crate}[[:space:]]*(=|\{)" "$toml"; then
      echo "banned dependency '${crate}' in ${toml} — ${BANNED[$crate]}"
      rc=1
    fi
  done
done < <(find . -name "Cargo.toml" -not -path "*/target/*")

exit $rc
```

Make it executable:

```bash
chmod +x .githooks/pre-commit
```

The grep pattern `^[[:space:]]*crate[[:space:]]*(=|\{)` matches both `crate = "1"` and `crate = { version = "1" }` without matching values. It scans only direct dependencies — transitive bans require `cargo-deny`.

---

## Auto-fixable lints

All 350 lints below carry `applicability: MachineApplicable` — `cargo clippy --all-targets --fix` applies them without human review.

**Recommended workflow:**

| Phase | Action |
|---|---|
| Development | Lints are `allow` (not configured) — no noise while writing code |
| Pre-commit | `cargo clippy --all-targets --fix` applies all fixes silently before the commit lands |
| CI | Same lints as `deny` — fails the build if any escaped the pre-commit step |

**Pre-commit fix command** (replace the fix step in `.githooks/pre-commit`):

```bash
cargo clippy --all-targets --fix --allow-dirty --allow-staged -- \
  -W clippy::all \
  -W clippy::pedantic \
  -W clippy::nursery
```

`clippy::all` covers `style`, `complexity`, `perf`, and `suspicious`. `pedantic` and `nursery` require explicit opt-in. `restriction` lints must be added individually — several come in contradictory pairs (`pub_with_shorthand`/`pub_without_shorthand`, `semicolon_inside_block`/`semicolon_outside_block`).

**CI check command:**

```bash
cargo clippy --all-targets -- \
  -D clippy::all \
  -D clippy::pedantic \
  -D clippy::nursery
```

### Full list

**complexity** (100 lints)

`bind_instead_of_map` · `bool_comparison` · `borrow_deref_ref` · `bytes_count_to_len` · `char_lit_as_u8` · `clone_on_copy` · `default_constructed_unit_structs` · `deprecated_cfg_attr` · `deref_addrof` · `derivable_impls` · `double_comparisons` · `double_parens` · `duration_subsec` · `explicit_auto_deref` · `explicit_write` · `extra_unused_type_parameters` · `filter_map_identity` · `filter_next` · `flat_map_identity` · `get_last_with_len` · `identity_op` · `implied_bounds_in_impls` · `int_plus_one` · `iter_count` · `iter_kv_map` · `let_with_type_underscore` · `manual_abs_diff` · `manual_c_str_literals` · `manual_div_ceil` · `manual_filter` · `manual_filter_map` · `manual_find` · `manual_find_map` · `manual_flatten` · `manual_hash_one` · `manual_inspect` · `manual_is_multiple_of` · `manual_main_separator_str` · `manual_ok_err` · `manual_option_as_slice` · `manual_range_patterns` · `manual_rem_euclid` · `manual_slice_size_calculation` · `manual_split_once` · `manual_strip` · `manual_swap` · `manual_unwrap_or` · `map_all_any_identity` · `map_flatten` · `map_identity` · `match_as_ref` · `match_single_binding` · `needless_arbitrary_self_type` · `needless_as_bytes` · `needless_bool` · `needless_bool_assign` · `needless_borrowed_reference` · `needless_ifs` · `needless_lifetimes` · `needless_match` · `needless_option_as_deref` · `needless_option_take` · `needless_question_mark` · `needless_splitn` · `nonminimal_bool` · `option_as_ref_deref` · `option_filter_map` · `option_map_unit_fn` · `or_then_unwrap` · `precedence` · `ptr_offset_with_cast` · `redundant_as_str` · `redundant_async_block` · `redundant_at_rest_pattern` · `redundant_closure_call` · `redundant_slicing` · `repeat_once` · `result_filter_map` · `result_map_unit_fn` · `seek_from_current` · `seek_to_start_instead_of_rewind` · `short_circuit_statement` · `single_element_loop` · `string_from_utf8_as_bytes` · `swap_with_temporary` · `transmute_ptr_to_ref` · `transmutes_expressible_as_ptr_casts` · `unit_arg` · `unnecessary_cast` · `unnecessary_first_then_check` · `unnecessary_literal_unwrap` · `unnecessary_map_on_constructor` · `unnecessary_min_or_max` · `unnecessary_operation` · `unnecessary_sort_by` · `unneeded_wildcard_pattern` · `useless_asref` · `useless_concat` · `useless_conversion` · `useless_format` · `useless_nonzero_new_unchecked`

**style** (111 lints)

`assign_op_pattern` · `blocks_in_conditions` · `bool_assert_comparison` · `box_default` · `byte_char_slices` · `bytes_nth` · `chars_last_cmp` · `chars_next_cmp` · `cmp_null` · `collapsible_if` · `comparison_to_empty` · `default_instead_of_iter_empty` · `disallowed_methods` · `disallowed_types` · `doc_lazy_continuation` · `err_expect` · `excessive_precision` · `filter_map_bool_then` · `for_kv_map` · `from_over_into` · `get_first` · `implicit_saturating_add` · `implicit_saturating_sub` · `infallible_destructuring_match` · `init_numbered_fields` · `into_iter_on_ref` · `io_other_error` · `is_digit_ascii_radix` · `items_after_test_module` · `iter_cloned_collect` · `iter_next_slice` · `iter_nth` · `iter_nth_zero` · `iter_skip_next` · `len_zero` · `let_and_return` · `let_unit_value` · `manual_async_fn` · `manual_bits` · `manual_dangling_ptr` · `manual_is_ascii_check` · `manual_is_infinite` · `manual_map` · `manual_next_back` · `manual_ok_or` · `manual_pattern_char_comparison` · `manual_range_contains` · `manual_repeat_n` · `manual_rotate` · `manual_saturating_arithmetic` · `manual_while_let_some` · `map_clone` · `map_collect_result_unit` · `match_ref_pats` · `match_result_ok` · `mem_replace_option_with_none` · `mem_replace_option_with_some` · `mem_replace_with_default` · `missing_enforced_import_renames` · `must_use_unit` · `needless_borrow` · `needless_borrows_for_generic_args` · `needless_else` · `needless_late_init` · `needless_parens_on_range_literals` · `needless_pub_self` · `needless_return` · `needless_return_with_question_mark` · `neg_multiply` · `new_without_default` · `obfuscated_if_else` · `ok_expect` · `op_ref` · `partialeq_to_none` · `print_literal` · `print_with_newline` · `println_empty_string` · `ptr_eq` · `question_mark` · `redundant_closure` · `redundant_field_names` · `redundant_pattern` · `redundant_pattern_matching` · `redundant_static_lifetimes` · `result_map_or_into_option` · `single_char_add_str` · `single_component_path_imports` · `single_match` · `string_extend_chars` · `to_digit_is_some` · `toplevel_ref_arg` · `trim_split_whitespace` · `unnecessary_fallible_conversions` · `unnecessary_fold` · `unnecessary_lazy_evaluations` · `unnecessary_map_or` · `unnecessary_mut_passed` · `unnecessary_owned_empty_strings` · `unneeded_struct_pattern` · `unused_unit` · `unwrap_or_default` · `while_let_on_iterator` · `write_literal` · `write_with_newline` · `writeln_empty_string` · `zero_ptr`

**pedantic** (57 lints)

`bool_to_int_with_if` · `borrow_as_ptr` · `cast_lossless` · `checked_conversions` · `cloned_instead_of_copied` · `collapsible_else_if` · `doc_comment_double_space_linebreaks` · `doc_markdown` · `elidable_lifetime_names` · `enum_glob_use` · `explicit_deref_methods` · `explicit_into_iter_loop` · `explicit_iter_loop` · `filter_map_next` · `flat_map_option` · `if_not_else` · `ignored_unit_patterns` · `implicit_clone` · `inconsistent_struct_constructor` · `inefficient_to_string` · `ip_constant` · `manual_assert` · `manual_ilog2` · `manual_instant_elapsed` · `manual_is_power_of_two` · `manual_is_variant_and` · `manual_midpoint` · `manual_string_new` · `map_unwrap_or` · `match_bool` · `must_use_candidate` · `needless_bitwise_bool` · `needless_for_each` · `needless_raw_string_hashes` · `non_std_lazy_statics` · `option_as_ref_cloned` · `ptr_as_ptr` · `ptr_cast_constness` · `ptr_offset_by_literal` · `range_minus_one` · `range_plus_one` · `redundant_closure_for_method_calls` · `redundant_else` · `ref_as_ptr` · `ref_binding_to_reference` · `semicolon_if_nothing_returned` · `single_char_pattern` · `single_match_else` · `stable_sort_primitive` · `unchecked_time_subtraction` · `unicode_not_nfc` · `uninlined_format_args` · `unnecessary_join` · `unnecessary_literal_bound` · `unnecessary_semicolon` · `unnested_or_patterns` · `wildcard_imports`

**perf** (20 lints)

`cmp_owned` · `collapsible_str_replace` · `double_ended_iterator_last` · `drain_collect` · `expect_fun_call` · `iter_overeager_cloned` · `large_const_arrays` · `manual_contains` · `manual_ignore_case_cmp` · `manual_retain` · `manual_str_repeat` · `map_entry` · `missing_const_for_thread_local` · `missing_spin_loop` · `redundant_iter_cloned` · `replace_box` · `to_string_in_format_args` · `unnecessary_to_owned` · `useless_vec` · `waker_clone_wake`

**nursery** (19 lints)

`clear_with_drain` · `derive_partial_eq_without_eq` · `equatable_if_let` · `imprecise_flops` · `missing_const_for_fn` · `needless_collect` · `needless_type_cast` · `nonstandard_macro_braces` · `redundant_clone` · `redundant_pub_crate` · `search_is_some` · `string_lit_as_bytes` · `suboptimal_flops` · `suspicious_operation_groupings` · `too_long_first_doc_paragraph` · `trait_duplication_in_bounds` · `unnecessary_struct_initialization` · `unused_rounding` · `use_self`

**restriction** (27 lints) — enable individually; four contradictory pairs resolved below

`alloc_instead_of_core` · `as_pointer_underscore` · `as_underscore` · `assertions_on_result_states` · `dbg_macro` · `deref_by_slicing` · `doc_include_without_cfg` · `get_unwrap` · `if_then_some_else_none` · `lossy_float_literal` · `missing_asserts_for_indexing` · `needless_raw_strings` · `non_ascii_literal` · `non_zero_suggestions` · `precedence_bits` · `pub_without_shorthand` · `rest_pat_in_fully_bound_structs` · `return_and_then` · `semicolon_outside_block` · `std_instead_of_alloc` · `std_instead_of_core` · `str_to_string` · `string_lit_chars_any` · `try_err` · `unseparated_literal_suffix` · `unnecessary_safety_comment` · `unused_trait_names`

*Contradictory pairs — one from each is omitted:*

- **`needless_return`** (in `clippy::all`) over `implicit_return` — idiomatic Rust uses tail expressions; explicit `return` is for early exits.
- **`pub_without_shorthand`** over `pub_with_shorthand` — `pub(super)` is the idiomatic form; the explicit `pub(in super)` spelling adds noise without clarity.
- **`semicolon_outside_block`** over `semicolon_inside_block` — `{ expr };` keeps the semicolon at the statement boundary where readers scan for it. Rustfmt agrees.
- **`unseparated_literal_suffix`** over `separated_literal_suffix` — `1u32` is the conventional form in the standard library and rustc. The `_` separator (`1_000_000`) is for digit grouping, not type suffixes.

**suspicious** (12 lints)

`cast_abs_to_unsigned` · `cast_slice_from_raw_parts` · `crate_in_macro_def` · `deprecated_clippy_cfg_attr` · `four_forward_slashes` · `ineffective_open_options` · `manual_unwrap_or_default` · `needless_character_iteration` · `swap_ptr_to_ref` · `unnecessary_clippy_cfg` · `unnecessary_option_map_or_else` · `unnecessary_result_map_or_else`

---

## Adoption in existing codebases

Adding this configuration to an existing codebase produces warnings everywhere at once. Work through them incrementally:

**Step 1 — Add everything as `warn`.**
Change all `deny` entries to `warn` initially. Shows the full scope without blocking builds or CI.

**Step 2 — Fix by lint, not by file.**
Fix one lint at a time (e.g., all `option_option` warnings, then all `map_err_ignore`). Keeps commits focused and reviewable.

**Step 3 — Promote to `deny` once clean.**
Once a lint produces zero warnings, promote it to `deny`. Carry forward legitimate exceptions as `#[expect]` with a reason.

**Suggested fix order:**

1. `unused_must_use` — mechanical; `let _ = result;` or add `?`
2. `exit`, `rc_mutex`, `todo`, `unimplemented` — low count, straightforward
3. `option_option`, `result_large_err`, `map_err_ignore` — error handling improvements
4. `await_holding_lock`, `let_underscore_future`, `blocking_in_async` — async design changes; may require refactoring
5. `unwrap_used`, `expect_used` — largest category; replace with `?` or `if let`; use `#[expect]` for invariants that are genuinely guaranteed

---

## Graduation

Only promote to deny when violations are genuinely always wrong. Most warn-level lints have legitimate exceptions — keep those at warn and use `#[expect]` case-by-case.

```toml
# Cargo.toml — safe to deny
map_err_ignore = "deny"
```

```toml
# dylint.toml — safe to deny
[result_result]
level = "deny"
[proper_error_type]
level = "deny"
[map_init_then_insert]
level = "deny"
[unstructured_log_fields]
level = "deny"
```
