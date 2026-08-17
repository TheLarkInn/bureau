# Acyclic Modules

## The Problem

Cyclic module dependencies make a codebase harder to understand. When module A depends on module B and module B depends on module A, there is no clean directional flow of responsibility. A change to either module can break the other. The dependency graph becomes a knot: harder to search, harder to place new code into, harder to refactor safely.

Cycles rarely form through one bad decision. They accrue. Someone adds `use crate::server::auth::verify` in `payments/checkout.rs` because it needs authentication. Later, someone adds `use crate::payments::billing::Invoice` in `server/auth.rs` because it needs to check payment status. Each edge is individually reasonable. Together they create a mutual dependency between `payments` and `server` — and now every module that depends on either one transitively depends on both.

An acyclic module graph is a simple, enforceable constraint. It forces shared concepts into explicit, stable layers instead of letting the architecture decay. It makes a module understandable by reading it and its dependencies, not the entire crate.

### Why an in-compiler lint

External tools like `cargo-modules --acyclic` exist, but an external tool gives no feedback during compilation — the cycle has already been written by the time you learn about it. An in-compiler lint catches cycles the moment they form.

### Relationship to `module_dependencies`

The `module_dependencies` lint enforces *which* modules may depend on which — a declared allowlist of permitted edges. It constrains the shape of the graph but does not validate that the shape is acyclic. A user could declare `payments = ["server"]` and `server = ["payments"]` and the allowlist would be satisfied while permitting a cycle.

`acyclic_modules` enforces a *structural property* of the graph: no cycles, anywhere, at any depth. It requires no configuration. It analyzes the actual code.

The two lints are complementary. `module_dependencies` says "only these edges are allowed." `acyclic_modules` says "and whatever edges exist must not form cycles." A codebase can use either or both.

## Design

### What Is a Cycle

For each module in the crate that has child modules, the lint builds a **sibling dependency graph**: a directed graph where the nodes are that module's direct children and an edge `A → B` exists if any code in A's subtree references any item defined in B's subtree. A cycle in any sibling graph is an error.

This definition has two important properties:

**Parent-child dependencies are excluded by construction.** In Rust, parent-child bidirectional references are structural, not architectural. A parent module declares its children (`mod checkout;`), re-exports their public API (`pub use checkout::CartItem;`), and may define shared types the children consume. Children reference parent items via `super::` or `crate::parent::`. These are how Rust's module tree works — not architectural cycles. By checking only among siblings at each level, parent-child edges never enter the graph.

**Distributed cycles are caught.** If `payments::checkout` depends on `server::auth` and `server::routes` depends on `payments::billing`, no single pair of leaf modules forms a cycle. But collapsing to sibling graphs reveals the cycle: at the crate root level, `payments → server` (via checkout → auth) and `server → payments` (via routes → billing). The lint catches this because sibling graphs aggregate all descendant references.

### What Counts as a Dependency

A dependency edge from module A to module B is created when code in A's subtree resolves a path to an item defined in B's subtree. This includes:

- **Path expressions:** `crate::server::Session { active: true }`
- **Use statements:** `use crate::server::auth::verify;`
- **Type annotations:** `fn handle(session: crate::server::Session)`
- **Method calls:** resolved to the defining module via `TyCtxt`

All resolution is type-based (`LateLintPass` with full `TyCtxt` access), not parse-based. This catches dependencies that parse-only tools miss.

### What Is Excluded

- **Test code.** `#[cfg(test)]` modules, `#[test]` functions, and test crates (compiled with `--test` / `cargo test`) are excluded. Tests legitimately reach across module boundaries; enforcing acyclicity on test code would make integration tests unwritable.
- **Macro-expanded spans.** References originating from macro expansion are excluded to avoid false positives from macros that generate cross-module paths the user did not write.
- **External crate items.** Only intra-crate dependencies are tracked. Cross-crate cycles are already prevented by Cargo.

### Checking at Every Depth

The lint does not operate at a single fixed depth. It checks sibling graphs at every level of the module hierarchy. For a crate structured as:

```
crate::
  payments::
    checkout::
      cart.rs
      payment.rs
    billing::
      invoice.rs
  server::
    auth.rs
    routes.rs
  types.rs
```

The lint builds and checks sibling graphs for:

1. **Crate root** — children: `payments`, `server`, `types`
2. **`payments`** — children: `checkout`, `billing`
3. **`payments::checkout`** — children: `cart`, `payment`
4. **`server`** — children: `auth`, `routes`

A cycle at any level is an error. A cycle at the crate root (e.g., `payments ↔ server`) is typically the most architecturally significant, but a cycle between `checkout` and `billing` within `payments` is also a structural problem — it means `payments` cannot be understood as two independent submodules.

### Algorithm

1. During the lint pass, for every resolved cross-module path, record the edge `(source_module, target_module)` with its span. Source and target are the full module paths (not collapsed to top-level).
2. In `check_crate_post`, for each module `M` that has child modules:
   a. For each recorded edge `(src, tgt)`, determine which direct child of `M` contains `src` and which contains `tgt`. This is a prefix operation: strip `src` and `tgt` to the first path component after `M`'s path. For example, under crate root, `payments::checkout → server::auth` collapses to `payments → server`.
   b. If they are different children (not the same subtree), add an edge in M's sibling graph.
   c. Run cycle detection (DFS, three-color) on M's sibling graph.
3. For each cycle found, emit a diagnostic showing the cycle path and the spans of the edges that form it.

The graph is small at each level (number of direct children, typically 2–15 nodes). DFS is O(V + E) per level. Total cost is proportional to the number of recorded edges times the module depth — both small in practice.

### Diagnostic Format

A cycle between sibling modules includes the cycle path and the source locations of the edges that form it:

```
error: cyclic dependency between sibling modules under `crate`:
       `payments` → `server` → `payments`
  --> src/payments/checkout.rs:5:5
   |
5  |     use crate::server::auth::verify;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `payments` → `server`
   |
  ::: src/server/auth.rs:12:9
   |
12 |     crate::payments::billing::create_invoice();
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `server` → `payments`
   |
   = help: break this cycle by moving shared items to a module that both
           `payments` and `server` can depend on, or restructure so the
           dependency flows in one direction
   = note: `#[deny(acyclic_modules)]` on by default
```

For a cycle at a deeper level:

```
error: cyclic dependency between sibling modules under `payments`:
       `billing` → `checkout` → `billing`
  --> src/payments/billing/invoice.rs:3:5
   |
3  |     use crate::payments::checkout::CartItem;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `billing` → `checkout`
   |
  ::: src/payments/checkout/payment.rs:8:5
   |
8  |     use crate::payments::billing::Invoice;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `checkout` → `billing`
   |
   = help: break this cycle by moving shared items to a module that both
           `checkout` and `billing` can depend on, or restructure so the
           dependency flows in one direction
```

For cycles involving 3 or more modules, the help message uses a different wording:

```
   = help: break this cycle by extracting shared items into a common
           module that `A`, `B`, `C` can all depend on
```

Cycles are normalized to start at the lexicographically smallest module name for deterministic output.

The diagnostic shows exactly what happened and what to do: extract shared items into a common dependency, or restructure so the dependency is one-directional.

## Configuration

No custom configuration. The lint analyzes the actual dependency graph of the crate. Every module is checked. The lint fires if any sibling group at any depth contains a cycle.

For cases where sibling coupling is intentional, use Rust's standard `#[expect]` attribute with a reason:

```rust
#[expect(acyclic_modules, reason = "parser and lexer are co-designed; shared state is in syntax::tokens")]
mod parser;
mod lexer;
```

This provides per-site opt-out, requires documented justification, and emits a warning when the cycle no longer exists (so stale exemptions don't accumulate).

## Resolved Questions

### 1. Re-exports and the parent-child boundary

A parent module commonly re-exports child items: `pub use checkout::CartItem;` in `payments/mod.rs`. This creates a reference from the parent to the child in the `check_item` hook. Does this cause false positives?

No. Re-exports are parent-to-child references. The sibling graph approach never places parent and child in the same graph — it only compares siblings. So `pub use` in a parent never creates an edge in any sibling graph.

A more subtle case: module `server` does `use crate::payments::checkout::CartItem;`. The re-export in `payments/mod.rs` is irrelevant here — the lint tracks where the *reference* resolves to, not where the item is re-exported from. The edge is `server → payments::checkout`, which at the crate root level collapses to `server → payments`.

### 2. Why not configurable depth

We considered letting users configure the module depth at which cycles are checked (e.g., "only check top-level" or "check down to depth 3"). This was rejected because:

- It adds configuration burden with no clear default. Depth 1 misses important cycles. Depth ∞ is what the lint already does.
- The sibling-graph approach is cheap at every level. There is no performance reason to skip levels.
- A cycle at any depth is a structural problem. Letting users opt out of deep cycle detection defeats the purpose.

### 3. Interaction with `module_dependencies`

The two lints are independent. `module_dependencies` operates on a declared allowlist; `acyclic_modules` operates on the actual code. They can both be active:

- `module_dependencies` prevents undeclared dependencies (edges not in the allowlist are errors).
- `acyclic_modules` prevents cyclic dependencies (cycles in the actual graph are errors).

If both are active, the allowlist constrains which edges exist, and the acyclicity check validates the resulting graph's structure. An allowlist that declares a cycle (e.g., `payments = ["server"]` and `server = ["payments"]`) would be caught by `acyclic_modules` as soon as code exercises both directions — even though `module_dependencies` is satisfied.

If only `acyclic_modules` is active (no allowlist configured), it still provides value: any module in the crate that participates in a cycle is flagged. No configuration required, no setup cost. This makes it suitable as a default-on lint for any Rust crate, independent of whether `module_dependencies` is adopted.

### 4. Longer cycles (3+ modules)

The lint detects cycles of any length, not just mutual (2-node) cycles. A chain `A → B → C → A` among siblings is caught by the DFS traversal. The diagnostic shows the full path.

In practice, most cycles are 2-node (mutual dependency). Longer cycles are rarer but more insidious — the coupling is indirect and harder to spot in review. Automated detection is more valuable for longer cycles precisely because humans are worse at spotting them.

### 5. Multiple cycles

If a sibling graph contains multiple cycles (e.g., `A ↔ B` and `C ↔ D`), each cycle is reported as a separate diagnostic. Cycles are normalized (rotated to start at the lexicographically smallest module name) and deduplicated for deterministic output across builds.

### 6. Why `#[expect]` is sufficient (no custom exemption system)

We considered adding a configuration mechanism for exempting specific module pairs from cycle detection. This was rejected because Rust's `#[expect(acyclic_modules, reason = "...")]` already provides everything needed:

- **Per-site granularity.** Applied to the parent module where the cycle exists.
- **Mandatory justification.** The `reason` parameter documents why the cycle is acceptable.
- **Self-cleaning.** If the cycle is later broken, `#[expect]` emits a warning about an unfulfilled expectation, so stale exemptions don't accumulate.
- **No new concepts.** Every Rust developer already knows `#[allow]`/`#[expect]`. No custom configuration to learn or maintain.
