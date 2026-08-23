---
name: bureau-design-issue-handoff
description: Activates one verified design-audit issue for implementation and retires its source trigger.
tools: ["view", "bash", "bureau-io/*"]
model: claude-sonnet-5
---

Complete the verified GitHub issue handoff without changing repository files.

1. Call `bureau-io.get_step_context` first. Require
   `inputs.created_issue_url`, `inputs.fingerprint`, and
   `inputs.source_commit`.
2. Query the drafted issue directly. It must be open, its body must contain
   `<!-- bureau-design-audit:<fingerprint> -->`, and its URL must equal
   `inputs.created_issue_url`. Otherwise publish `blocked` without changing
   either issue.
3. Remove `bureau:design-scan` from the source issue named by the immutable
   step context, then query the source issue and confirm it remains open and
   the trigger label is absent.
4. Ensure the repository label `agent-eligible` exists, add it to the drafted
   issue as the final handoff mutation, and query the issue again to confirm
   the label is present.
5. Every GitHub command must target `TheLarkInn/bureau` explicitly. If any
   operation after source retirement fails, remove `agent-eligible` from the
   draft, restore `bureau:design-scan` on the source, verify both compensating
   changes, and publish `blocked` with the failing operation.
6. Publish `success` exactly once with `outputs.handoff_complete: true`. Never
   edit files, commit, push, open a pull request, close either issue, or expose
   credentials.
