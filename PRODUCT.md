# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

The primary user is bureau's own maintainer — a solo developer operating a
local, self-hosted agent-work daemon — with future contributors who adopt
bureau as a secondary audience. They operate the graph-designer TUI and the
bureau-canvas web UI as an agent-ops console: watching pipeline runs, editing
pipeline graphs, reviewing agent output at gates, and steering work.

## Product Purpose

bureau is a single-binary daemon that continuously compares desired state
("every work item matching this filter should have an open PR") with observed
state on the forge, and closes the gap by running agent-driven pipelines
against git worktrees. It is a CI runner with a work queue and a reconcile
loop whose step body is an LLM. Success means agent work runs unsupervised
but stays auditable, budgeted, and reviewable.

## Positioning

Control through graphs. Pipelines as visual, editable graphs are the
differentiator: design makes complex agent workflows legible and editable.
The four capabilities no existing runner provides — pull-based claiming,
durable cross-run state, nondeterministic cost control, and nondeterministic
output handling — are exercised through those graphs. A neighboring
agent-runner can copy individual features; it cannot truthfully copy
graph-native control of a self-hosted reconcile loop.

## Operating Context

Runs in a Linux dev container on one developer's machine; one process, no
cluster. Operated through a TUI (graph designer) and a browser UI
(`.github/extensions/bureau-canvas`, the Copilot app canvas) against the same
`.bureau/` config. The forge (GitHub/ADO) owns work items, PRs, review
threads, and identity; bureau consumes them via API. Upcoming practice:
self-hosted bureau runs its own agentic loops for design review and visual
testing of the TUI and web UI.

## Capabilities and Constraints

- Rust 2021 single-binary daemon; offline tests at every layer (no network,
  no model calls).
- Deny-level lint gates: 25-line functions, cognitive complexity ≤ 4,
  300-line files, zero `#[allow]`, zero `unsafe`, zero warnings.
- `dylint.toml` is the exhaustive module architecture; new cross-module
  edges require updating it in the same change.
- Naming law (DESIGN.md §2): no invented nouns, no mascots, no Kubernetes
  vocabulary — applies to UI copy as well.
- Non-goals (DESIGN.md §3) are hard; do not design features on that list.
- Config fields beyond DESIGN.md §6 require asking first.
- Undecided: how TUI and web UI share design tokens (they currently evolve
  independently).

## Brand Commitments

Working name `bureau` is a placeholder, swappable with one sed; no mascot,
animal, or themed name. No binding visual constraints recorded — no pinned
palette, type, or theme; the visual world is decided per-surface in new-work.

## Evidence on Hand

- `DESIGN.md` — authoritative engineering spec.
- `.github/extensions/bureau-canvas/web/` — incumbent web UI (React flow
  graph, vendored deps), with e2e screenshot harness in `e2e/`.
- TUI graph designer in `crates/` (ratatui-based).
- No marketing site, testimonials, or external proof; future work must not
  fabricate any.

## Product Principles

1. The graph is the interface — every agent workflow is visible and editable
   as a graph before it is anything else.
2. Autonomy with audit — every agent action, cost, and diff is inspectable;
   review gates are first-class surfaces, not log lines.
3. Boring operations — the reconcile loop should feel calm; surfaces
   highlight exceptions and pending gates, not noise.
4. Offline-verifiable — every layer, including UI behavior, is testable
   without network or model calls.
