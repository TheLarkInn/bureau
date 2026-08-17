# Module Dependencies

## The Problem

AI coding agents widen visibility modifiers (`fn` → `pub fn`, `pub(crate)` → `pub`) as a quick fix for compiler access errors, silently exposing internals and creating architectural debt. The ball of mud forms because architectural changes are invisible in code review — a `use crate::server::SessionInfo` buried on line 847 of a new file is an architectural decision that ships unreviewed.

Two parts: a **lint** that forces architectural intent before implementation, and a **CI process** that makes dependency changes visible to reviewers.

### Why Compile-Time, Not CI-Only

A CI-only check (flagging bad dependencies in PR diffs) fires *after* the agent has written the code. The agent pushes a PR, CI flags it, the agent rewrites — or the human rejects and explains. That's the review bottleneck this tool exists to eliminate. A compiler lint fires *during* implementation, before a PR exists. The agent gets immediate feedback and self-corrects without human involvement.

### Demand Side Only

The lint enforces **who may consume** (demand side), not **what is exposed** (supply side). An agent widening `fn` to `pub fn` within the same module won't trigger this lint. That's deliberate — Rust's visibility modifiers (`pub`, `pub(crate)`, `pub(super)`, private fields, accessor methods) already handle the supply side. Trying to control both sides is what Packwerk attempted with privacy enforcement; they removed it in 3.0 because it created more debt than it resolved. The lint constrains dependency direction. Rust constrains API surface. Each does its job.

## Why an Allowlist, Not a Deny List

We considered two approaches:

- **Deny list:** Everything is permitted by default. Bad edges are recorded after they're caught in review.
- **Allowlist:** Nothing is permitted by default. Every cross-module dependency must be declared upfront.

The deny list is reactive — by the time a reviewer catches a bad edge, the agent has already written code that depends on it. Rejecting the PR means throwing away work. Approving it means the bad edge ships.

The allowlist forces the architectural decision **before the code exists**. The agent can't write `use crate::server::SessionInfo` in the payments module without first adding `server` to payments' dependency list in the config. That config change is the architectural decision, and it happens explicitly, not buried in an implementation file.

A wrong allowlist is fixable — update the config, the lint stops firing. A bad dependency that shipped is structural debt — code depends on it, tests assume it, removing it is a refactor. The allowlist's failure mode is friction. The deny list's failure mode is erosion. Erosion is worse.

## What We Learned From Other Ecosystems

We researched how Java (ArchUnit), Python (import-linter), TypeScript (dependency-cruiser), Ruby (Packwerk), Go (language + third-party tools), and .NET (ArchUnitNET, NDepend) solve this problem.

### The Packwerk Story

Shopify built allowlist-based module dependency enforcement for a 2.8M-line Ruby monolith. Gusto extended the tooling (including a Rust reimplementation, `pks`, for 10-20x speed). GitHub adopted it. The results were mixed:

**What worked:** "Stop the bleeding" — new violations are blocked, old ones grandfathered. Exhaustive mode prevents modules from escaping enforcement. Gusto achieved ~20% CI savings via conditional pack builds.

**What failed:** Boundaries drawn from semantic intuition ("this is billing code") rarely matched actual runtime coupling. Todo files grew "monstrously large." Even after clearing all Packwerk violations for a package, only 40% of tests passed when actually extracted — static analysis misses dynamic coupling (metaprogramming, test fixtures, initializers). Privacy enforcement (controlling which items are part of a module's public API) was removed entirely in Packwerk 3.0 — it created more debt than it resolved.

**Shopify's conclusion:** Packwerk remains valuable for "holding the line against new dependencies at the base layer" but is "no longer as central as it once was." Architecture-as-tests works. Getting the boundaries right is the hard part.

### What to Adopt

1. **Exhaustive mode** (from import-linter): Every module must appear in the config. An agent cannot create a new module that silently escapes enforcement.

2. **Full dependency chains in diagnostics** (from ArchUnit + import-linter): Show the exact line, the item being referenced, and which module it resolved to.

3. **Architecture-as-configuration** (from ArchUnit): The config file is the crate's architectural diagram — documentation that fails the build when violated.

4. **Full type resolution, not parse-only** (learning from cargo-archtest): Parse-only tools detect dependencies via `use` statements but miss direct path references in function bodies, type annotations, and trait bounds. A compiler lint with `TyCtxt` access sees all resolved paths.

5. **Dead edge detection** (novel): Warn on edges declared in the config that have no corresponding dependency in code. Stale edges make the config lie and the diagram misleading. The config must stay honest.

### What to Skip

1. **Baseline / todo files:** Packwerk's `package_todo.yml` and ArchUnit's `FreezingArchRule` exist for gradual adoption on legacy codebases. We skip this — violations are errors, not warnings. This is viable because the lint targets new projects where dependencies are clean from the start. Baselines defer debt; they don't resolve it. Shopify's todo files grew for years without being resolved. If legacy adoption becomes a requirement, revisit this decision — but the no-baseline stance is correct for the current scope.

2. **Privacy enforcement:** Shopify tried controlling which items are part of a module's public API. They removed it in Packwerk 3.0. Dependency direction is the right lever — not API surface control. Rust's visibility modifiers (`pub`, `pub(crate)`, `pub(super)`) already handle the supply side. The lint handles the demand side.

3. **Transitive dependency checking** (from import-linter): import-linter flags `A → B → C` as `A` depending on `C`. The allowlist already handles this — if both edges are approved, the transitive dependency is by design.

4. **Multi-layer classification schemes:** Every ecosystem uses layers (Java: 3-5, Stripe: 5). Every engineer complains about classification ambiguity. A struct belongs in the foundation, but its methods may belong in application logic — they live in the same impl block. Per-module allowlists sidestep this by not classifying at all; they just constrain edges.

### Known Failure Modes to Design Against

Research across ecosystems identified six failure modes:

| Failure Mode | Root Cause | Our Mitigation |
|---|---|---|
| Utopian graph design | Boundaries from ideals, not code reality | Config is reviewed in PRs with a rendered graph — bad boundaries are visible |
| Baseline only grows | Adding exceptions easier than fixing violations | No baseline — violations are errors |
| Config burden > value | Config must track every structural change | Config is small (top-level modules only) and changes are the review artifact |
| Cross-cutting gravity well | Shared/foundation module absorbs everything | Visible in the graph — a module with arrows from everything is obvious |
| Refactoring cliff | Large refactors require simultaneous config changes | Config is one file with short lines — updating it is trivial |
| Tool as noise | Violations without actionable guidance | Prescriptive diagnostics that say what to do, not just what's wrong |

## The Gap in Rust Tooling

No existing Rust tool provides type-resolved, directional module dependency enforcement within a single crate:

| Tool | What It Does | What It Doesn't Do |
|---|---|---|
| `unreachable_pub` | Flags `pub` items that aren't reachable outside the crate | No dependency direction |
| `pub(in path)` | Controls what's exposed (supply side) | Doesn't control who consumes (demand side) |
| `cargo-modules --acyclic` | Detects cycles | No directional enforcement |
| `arch_test_core` / `cargo-archtest` | Layer and cycle rules | Parse-only — misses direct path references |
| Crate splitting | Full enforcement via `Cargo.toml` | High cost, orphan rule complications |

The infrastructure for a proper lint exists: dylint provides `LateLintPass` with full `TyCtxt` access. Nobody has built it.

## Design

### Lint: `module_dependencies`

**Core idea:** Each top-level module declares which other top-level modules it may depend on. Anything not in the allowlist is a compile-time error. The config file is the architecture diagram.

**Configuration:**

```toml
[module_dependencies]
exhaustive = true  # Every top-level module must appear

[module_dependencies.allow]
types = []
errors = ["types"]
utils = ["types", "errors"]
payments = ["types", "errors", "utils"]
server = ["types", "errors", "utils", "payments"]
```

**Workspace scoping (bureau patch):** the config is read once per workspace,
so a multi-crate workspace shares one allowlist. Keys may be prefixed with a
crate name to scope an entry to one crate, and dead edges are only flagged
when both endpoint modules exist in the crate being linted:

```toml
[module_dependencies.allow]
"app:server" = ["types", "errors"]
"helper:types" = []
```

Unprefixed keys apply to every crate. Crate-root items behave as
pseudo-modules named after the item (e.g. a crate-root `struct Id` is
referenced as module `Id`); declare them with an empty allowlist.

**Submodule granularity (future):** The config schema should not prevent optional nesting for cases where top-level granularity is too coarse. For example, allowing only `payments.checkout` (not all of `payments`) to depend on `server`:

```toml
[module_dependencies.allow]
payments = ["types", "errors", "utils"]
payments.checkout = ["server"]  # only checkout, not all of payments
```

This is not required for the initial implementation, but the config format and lint internals should not make it impossible to add later. Top-level modules are the default enforcement boundary; submodule overrides are opt-in where the risk justifies the config cost.

**How it works:**

1. At compile time, for every resolved path, the lint determines the source and target top-level modules.
2. If the target module is not in the source module's allowlist, the lint emits an error.
3. Modules not in the config are errors (exhaustive mode).
4. `#[cfg(test)]` code is excluded — tests can reach anywhere. This is safe because test code doesn't run in release builds, so an agent cannot use `#[cfg(test)]` to smuggle production logic past the lint. Integration tests genuinely need cross-module access; enforcing the allowlist on tests would make them unwritable without mirroring the entire dependency graph.
5. Edges declared in the config with no corresponding dependency in code produce a warning (dead edge detection).

**Diagnostic format:**

```
error: `payments` depends on `server`, which is not in its allowlist
  --> src/payments/checkout.rs:12:5
   |
12 |     use crate::server::SessionInfo;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: if this dependency is architecturally correct, add "server" to the
           `payments` allowlist in dylint.toml under [module_dependencies.allow]
           if not, move the item to a module that `payments` is allowed to
           depend on (currently: errors, types, utils)
```

The diagnostic tells the agent exactly what to do: either update the config (which will be reviewed) or restructure the code.

### CI Process: Graph Rendering

The config file is machine-readable architecture. CI renders it as a visual graph on every PR that changes it.

**What CI does:**

1. Detect if `dylint.toml` changed in the PR.
2. Render the before and after dependency graphs (mermaid, graphviz, or similar).
3. Post the graph diff as a PR comment, highlighting new and removed edges.

**Example PR comment:**

```
Module dependency changes in this PR:

  Added:    payments → server
  Removed:  utils → legacy

[Before] types ← errors ← utils ← payments
                             ↑
                           server

[After]  types ← errors ← utils ← payments
                             ↑        ↓
                           server ←───┘
```

The reviewer sees the architectural change visually. "Payments now depends on server and server depends on payments — that's a cycle." Ten-second review. Without this, the same change is a `use` statement buried in an implementation file.

**Why render from the config, not from the code:** The config is the architectural intent. The lint ensures the code matches the config. The graph shows the intent. If we derived the graph from code, it would show the current state including any violations — which defeats the purpose.

### Complementary Conventions

The lint enforces dependency direction (demand side). Rust's existing mechanisms enforce API surfaces (supply side). Together:

- **Private submodules:** Submodules within a top-level module should be private (`mod`, not `pub mod`). The parent re-exports its API via `pub use`. This is standard Rust (the std library does this everywhere). An agent widening a submodule to `pub mod` is a code smell independent of this lint.
- **`unreachable_pub` denied:** Items marked `pub` that aren't reachable outside the crate should use `pub(crate)`. This is an existing Rust lint, set to `deny`.

These conventions are recommendations, not enforced by the `module_dependencies` lint. They reduce the surface area of what's importable, making the allowlist more meaningful.

## Resolved Questions

### 1. False positive rate in practice

**Proc macros:** Non-issue. Proc macros must live in external crates (Rust language requirement). The lint only tracks intra-crate dependencies, so proc macro-generated paths are invisible to it.

**`macro_rules!` macros:** References originating from macro expansion are excluded (`span.from_expansion()` is skipped). A `macro_rules!` macro in `utils` that expands to `crate::server::SessionInfo` will **not** create a dependency edge, because the reference span is marked as macro-expanded. This avoids false positives from macros that generate cross-module paths the user did not explicitly write. The tradeoff is that macro-mediated coupling is invisible to the lint.

**Re-exports:** The lint tracks the *syntactic* dependency — the module the path resolves through, not the module that originally defines the item. If `api` re-exports `types::UserId` and `payments` uses `api::UserId`, the lint sees `payments → api`. This is correct. The alternative (tracing to the origin module) would require the config to model re-export chains, adding config bloat without meaningful architectural signal.

**Feature-gated code:** `#[cfg(feature = "...")]` code is treated the same as non-gated code. A dependency that only exists under a feature flag is still a dependency. Only `#[cfg(test)]` code is excluded.

**Blanket trait impls:** If `types` defines `trait Validate` with a blanket impl and `payments::Order` satisfies the bounds, the implicit dependency may or may not surface as a path the lint can intercept. This is a known potential gap. **Action:** Validate empirically during implementation with real-world trait patterns. If `TyCtxt` does not surface these as interceptable paths, document as a limitation rather than adding complexity to cover an edge case.

### 2. Orphan rule interaction

The lint does not solve crate splitting. Module boundaries are not crate boundaries. A clean module dependency graph is *necessary* for a future crate split but not *sufficient* — the orphan rule (`impl ForeignTrait for ForeignType` is forbidden across crate boundaries) introduces constraints that module-level analysis cannot detect.

This is a known limitation, not a design flaw. The lint's job is enforcing architecture within a crate. Crate splitting is a separate problem with a separate strategy (newtypes, facade crates, etc.) to be documented independently when the need arises.

### 3. What agents need

**Model:** The AI agent can either restructure code *or* propose a config change in a separate commit. The config change goes through CI, which renders the graph diff. A human reviews the architectural decision (config + graph), not the implementation.

This means the agent is never blocked — it can always unblock itself by proposing the dependency. The review gate is on the config change, not the code. If the human rejects the architectural change, the agent restructures.

**Diagnostic update:** The error message makes both paths explicit:

```
error: `payments` depends on `server`, which is not in its allowlist
  --> src/payments/checkout.rs:12:5
   |
12 |     use crate::server::SessionInfo;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: if this dependency is architecturally correct, add "server" to the
           `payments` allowlist in dylint.toml under [module_dependencies.allow]
           if not, move the item to a module that `payments` is allowed to
           depend on (currently: errors, types, utils)
```
