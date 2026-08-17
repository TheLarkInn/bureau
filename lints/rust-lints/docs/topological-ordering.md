# Topological Ordering of Items Within a Module

## The Problem

When reading a Rust source file, the order of item definitions affects comprehension. If `main` calls `run`, which calls `process`, which calls `validate`, a reader benefits from a consistent ordering convention -- either always reading from high-level entry points down to implementation details, or from leaf utilities up to composition roots.

Without a convention, item order is accidental. Items end up wherever they were added chronologically, producing files where a reader must jump around to follow the call graph. This is worse in files maintained by multiple authors (human or AI), where no one is responsible for the whole file's narrative structure.

### Why a lint

Rustfmt does not control item order because it is semantic — it depends on the call/reference graph between items. A lint with HIR and type information can resolve references and enforce a consistent ordering.

### Relationship to dylint's `non_topologically_sorted_functions`

dylint has an existing lint that covers functions only. This lint extends the concept to all items: structs, enums, traits, type aliases, constants, and their impl blocks. It also addresses grouping (a struct and its inherent impl should be treated as one unit).

## Design

### Direction: callee-first (bottom-up)

An item appears before any item that references it. Leaf functions are at the top of the file, composition roots at the bottom. This is the C convention (historically required by the compiler). Reading top-to-bottom reveals building blocks before their compositions.

Callee-first is the only supported direction. There is no caller-first option.

**Rationale:**

1. **Natural fit with Rust's type system.** Types are defined before the functions that use them. `struct Config` appears before `fn process(cfg: Config)`, so the reader (or agent) understands the type before encountering its use.

2. **Matches existing Rust conventions.** `main()` and `pub fn` entry points conventionally appear at the bottom of a file, after internal machinery. dylint's prior `non_topologically_sorted_functions` lint also uses callee-first.

3. **Better for AI coding agents.** Transformer-based models attend to all tokens in context, but recency bias means tokens near the end of the context window carry more weight. With callee-first ordering, the agent finishes reading a file with the high-level orchestration code freshest -- the composition that references all the building blocks defined earlier. This is the most useful context for planning edits, since the agent can see how pieces connect while still resolving earlier type/function definitions by name.

### What items are covered

All named items at the module level:

- Functions (`fn`)
- Structs (`struct`)
- Enums (`enum`)
- Traits (`trait`)
- Type aliases (`type`)
- Constants (`const`)
- Statics (`static`)
- Impl blocks (grouped with their type -- see below)

Not covered:

- `use` statements -- import ordering is rustfmt's job
- `mod` declarations -- module structure is architectural, not topological
- `extern crate` -- rare and always at the top
- Macro definitions (`macro_rules!`) -- they must appear before use regardless of convention
- Items inside function bodies -- too granular, not visible at module level

### Grouping: structs and their impl blocks

A struct/enum and all its `impl` blocks are logically one unit. Separating them with unrelated items is confusing. The lint treats each type and its `impl` blocks as a single "item group" for ordering purposes.

**Specifically:**

1. A struct/enum and all its `impl` blocks (inherent and trait) are one group, provided the self type is defined in the same module.
2. The group's position is determined by the struct/enum definition.
3. All `impl` blocks must appear immediately after their struct/enum (no unrelated items in between).
4. `impl` blocks for types defined in other modules are independent items and follow normal topological rules.

### What constitutes a "reference"

An edge from item A to item B exists when A's body or signature references B:

- **Function calls:** `validate(x)` in `process()` creates an edge `process -> validate`
- **Type annotations:** `fn process(x: Config)` creates an edge `process -> Config`
- **Struct literals:** `Config { ... }` creates an edge from the containing item to `Config`
- **Method calls:** `x.validate()` creates an edge to the impl item that defines `validate`
- **Trait bounds:** `fn process<T: Validate>(x: T)` creates an edge to `Validate`
- **Constants/statics in expressions:** `let x = MAX_SIZE;` creates an edge to `MAX_SIZE`

Only references to items in the **same module** are relevant. Cross-module references do not affect intra-module ordering.

### Cycle handling

Mutual references between items in the same module are common and legitimate:

```rust
fn parse_expr() -> Expr {
    // ...calls parse_atom...
}

fn parse_atom() -> Atom {
    // ...calls parse_expr for subexpressions...
}
```

When items form a cycle, the lint treats the entire strongly connected component (SCC) as a single unit. Items within an SCC can appear in any order relative to each other. The SCC as a whole must be ordered topologically relative to items outside the SCC.

Reporting a cycle as an error would be wrong — cycles are valid Rust and common in recursive-descent parsers, state machines, and mutually recursive data structures. Silently ignoring cycled items would leave ordering undefined. Treating the SCC as a unit preserves the ordering guarantee for everything outside the cycle.

### Algorithm

1. **Collect items.** During the lint pass, for each module, collect all top-level items with their spans and `DefId`s.

2. **Build the reference graph.** For each item, walk its HIR subtree (body, signature, where clauses) and resolve references to other items in the same module. Each resolved reference is a directed edge.

3. **Group inherent impls.** Merge each inherent `impl` block with its type definition into a single node.

4. **Compute SCCs.** Run Tarjan's algorithm (or Kosaraju's) to find strongly connected components. Collapse each SCC into a single node.

5. **Topological sort.** Sort the DAG of SCC nodes. This gives the expected order.

6. **Compare.** Walk the actual item order and the expected order. For each item that appears out of order, emit a diagnostic.

### Diagnostic format

```
warning: items are not in topological order in this module
  --> src/lib.rs:5:22
   |
5  |     fn process(_cfg: Config) {}
   |                      ^^^^^^ `fn process` references `struct Config` but appears before it
...
LL |     struct Config {
   |     ------------- `struct Config` defined here
   |
   = help: reorder items so referenced items appear before their referencing items
```

For the "item group" violation (inherent impl not adjacent to type):

```
warning: `impl Widget` is separated from its type definition
  --> src/lib.rs:30:5
   |
LL | /     struct Widget {
LL | |         name: String,
LL | |     }
   | |_____- `Widget` defined here
...
LL | /     impl Widget {
LL | |         fn new(name: String) -> Self {
LL | |             Self { name }
LL | |         }
LL | |     }
   | |_____^
   |
   = help: move the impl block adjacent to its type definition
```

### Lint level

`Warn` by default. This is a style/readability lint, not a correctness lint. Code with items in non-topological order compiles and runs correctly. Use `#[allow(topological_ordering)]` on specific items or modules where the convention does not apply. Users who want stricter enforcement can set the level to `deny`.

## Resolved Questions

### 1. Why not just functions?

dylint's `non_topologically_sorted_functions` only covers functions. But types are part of the dependency graph. A function that takes a `Config` parameter depends on `Config`. If `Config` is defined 200 lines below the function, the reader cannot understand the function signature without scrolling. Covering all items makes the ordering holistic.

### 2. Why group trait impls with the type?

A trait impl (`impl Display for Config`) references both `Display` (possibly external) and `Config` (local). When `Config` is defined in the same module, the impl belongs with `Config` -- it is part of `Config`'s API surface. Grouping all impls with their type keeps the type's full interface in one place, which aids both human and agent comprehension.

### 3. Why SCCs instead of just ignoring all cycles?

If items A and B form a cycle and items C and D are independent, ignoring cycles entirely would leave A and B unconstrained relative to C and D. Using SCCs preserves ordering guarantees: the {A, B} group is ordered relative to C and D, even though A and B are unordered relative to each other.

### 4. What about `pub` items?

Some codebases put all `pub` items first (or last). This lint does not enforce that convention. The topological order is determined by the reference graph, not visibility. A `pub` function that calls a private helper should appear after the helper (in callee-first mode), regardless of visibility. If a codebase wants visibility-based ordering, that is a different lint.

### 5. What about test modules?

`#[cfg(test)] mod tests { ... }` is excluded from ordering analysis. Test modules conventionally appear at the bottom of the file and reference all items in the module. Including them would force every item to appear before the test module (which is already the convention) but would not provide useful ordering signal within the test module itself. Items inside the test module could optionally be checked in a future extension.

### 6. Interaction with `acyclic_modules`

`acyclic_modules` checks dependencies between modules. `topological_ordering` checks item order within a module. They are orthogonal. Both can be active simultaneously without interference.

## File Structure

```
src/lints/
  topological_ordering.rs   # lint implementation

ui/topological_ordering/
  main.rs                   # UI test cases
  main.stderr               # expected diagnostics
```

Registration in `src/lib.rs`:
- Add `lints::topological_ordering::TOPOLOGICAL_ORDERING` to `register_lints`
- Add `register_late_pass` for `TopologicalOrdering`

Registration in `src/lints/mod.rs`:
- Add `pub mod topological_ordering;`

Add `[[example]]` entry in `Cargo.toml` for the UI test.

## CI integration

Enable the lint in CI to catch violations:

```bash
cargo dylint --all -- --all-targets
```

| Phase | Action |
|---|---|
| Development | Lint is `Warn` — shows warnings in the editor |
| CI | `DYLINT_RUSTFLAGS="-D topological_ordering"` to promote to errors |

## Test Cases

1. **Basic violation:** caller before callee -- warning.
2. **Type reference violation:** function before struct it references -- warning.
3. **Inherent impl separated from struct:** warning.
4. **SCC with outside dependency:** SCC before its dependency -- warning.
5. **Correct callee-first ordering:** helper before caller -- no warning.
6. **Correct order with struct and function:** struct before function -- no warning.
7. **Inherent impl adjacent to struct:** no warning.
8. **Trait impl separated from type:** warning when separated by unrelated items.
9. **Mutual recursion (cycle):** no warning for items in the SCC, SCC ordered relative to other items.
10. **Items with no local references:** unconstrained, no warning regardless of position.
11. **`#[allow(topological_ordering)]` suppresses the warning.**
12. **`#[cfg(test)] mod tests` is excluded entirely.**
13. **Cross-module references do not create ordering edges.**
