# `unsafe_send_missing_drop`

**Level:** `warn`

Flags types that have `unsafe impl Send` with `!Send` fields but no `Drop` implementation, meaning the implicit destructor will drop those fields on whatever thread happens to drop the owning struct.

## Why

When a type contains `!Send` fields (e.g. `Rc`, `*mut T`, ObjC pointers), those fields must be created, accessed, and destroyed on a specific thread. Writing `unsafe impl Send` lets the struct move across threads, but without a `Drop` impl the compiler-generated destructor drops each field on whichever thread drops the struct:

- **Silent unsoundness** — the `!Send` field is destroyed on a thread it was never meant to touch. For reference-counted types this corrupts the count; for OS handles this violates API contracts.
- **Hard to catch** — the bug only manifests when the value is dropped on the "wrong" thread, which may be rare in practice but catastrophic when it happens (use-after-free, double-free, data races on the reference count).
- **Easy to forget** — the author of `unsafe impl Send` focused on making the type movable across threads but didn't consider what happens at destruction time.

The fix is to implement `Drop` and ensure `!Send` fields are destroyed in the correct context: dispatch to the right thread, use `ManuallyDrop` to suppress implicit destruction, or otherwise take explicit responsibility for cleanup.

## What it checks

The lint fires on `unsafe impl Send for T` when all of these are true:

1. `T` is a struct or enum (ADT).
2. `T` has no `Drop` implementation.
3. At least one field has a type that does not implement `Send`. Fields wrapped in `ManuallyDrop<_>` or `PhantomData<_>` are excluded.

## Examples

### Fires

```rust
struct Handle {
    //~^ WARNING: `Handle` has `unsafe impl Send` but contains `!Send` fields and no `Drop` impl
    inner: UnsafeCell<Rc<String>>,
}
unsafe impl Send for Handle {}
```

```rust
struct Generic<T> {
    //~^ WARNING: `Generic` has `unsafe impl Send` but contains `!Send` fields and no `Drop` impl
    value: T,
}
// Promises Send for ALL T, including T: !Send — the implicit drop of T is unsound.
unsafe impl<T> Send for Generic<T> {}
```

### Does not fire

```rust
// Has a Drop impl — author has taken responsibility for destruction.
struct WithDrop {
    inner: UnsafeCell<Rc<String>>,
}
unsafe impl Send for WithDrop {}
impl Drop for WithDrop {
    fn drop(&mut self) {
        // Dispatches destruction to the correct thread.
    }
}
```

```rust
// All fields are Send — implicit drop is fine.
struct AllSend {
    data: String,
    count: usize,
}
unsafe impl Send for AllSend {}
```

```rust
// ManuallyDrop suppresses implicit destruction.
struct WithManuallyDrop {
    inner: ManuallyDrop<Rc<String>>,
}
unsafe impl Send for WithManuallyDrop {}
```

```rust
// PhantomData has no value to drop.
struct WithPhantom<T> {
    _marker: PhantomData<T>,
    data: String,
}
unsafe impl<T> Send for WithPhantom<T> {}
```

## Configuration

No additional configuration.

## Related lints

- Clippy's `non_send_fields_in_send_ty` flags `unsafe impl Send` for types containing `!Send` fields. That lint is broader — it fires even when the author has a valid reason for the unsafe impl. This lint only fires when there is no `Drop` impl, meaning the author hasn't addressed the destruction problem.
- Complements `panic_in_drop` — together they cover the two most common `Drop`-related soundness issues: missing a `Drop` impl when one is needed (this lint) and panicking inside one that exists (`panic_in_drop`).
