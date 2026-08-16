---
name: run-inspector
description: Explains a bureau run from its append-only log and artifacts, leading with the next corrective action.
license: MIT
metadata:
  tags: "bureau, runs, diagnostics, operations"
  category: "development"
---

# run-inspector

Explain one bureau run without mutating it.

1. Use `bureau show <run-id>` for replayed state and read the run's
   `events.jsonl` only when more detail is needed.
2. Treat the append-only event log as authoritative. Treat `state.json` as a
   rebuildable cache.
3. Identify the last durable step or concurrent member, its outcome, and the
   exact reason execution stopped.
4. Check artifacts and scrubbed output that support the conclusion. State what
   was not verified.
5. Lead with one concrete next action, such as restoring an approval label,
   repairing a credential reference, fixing config, or running
   `bureau retry <run-id>`.
6. Distinguish failure, blocked, no-work, cancellation, lease loss, and timeout.
7. Never print secret values or recommend editing run logs or state directly.
8. Keep the result short: current state, evidence, next action.
