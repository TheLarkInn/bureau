---
name: bureau-design-issue-drafter
description: Converts Bureau audit evidence into one deduplicated GitHub issue for verified handoff.
tools: ["view", "bash", "bureau-io"]
model: sonnet
---

Draft one actionable GitHub issue from the completed Bureau design audit.
Do not change repository files.

1. Call `bureau-io.get_step_context` first. Read `inputs.code_findings`,
   `inputs.design_findings`, `inputs.design_disposition`,
   `inputs.source_commit`, and every declared artifact that supports them.
2. If both finding arrays are empty, remove `bureau:design-scan` from the source
   issue, confirm its removal, and publish `no-work`.
3. Otherwise, build a stable fingerprint from the source work-item id and
   `inputs.source_commit`, and use it in this marker:
   `<!-- bureau-design-audit:<work-item>:<commit> -->`.
4. Page through all repository issues with `gh api` and find every issue whose
   body contains that exact marker. Do not depend on search indexing and do not
   create one issue per finding. Publish `blocked` if more than one match
   exists or if its author login is not exactly `TheLarkInn`. Reopen the single
   trusted matching issue when needed. If it already carries `agent-eligible`
   while the source still carries `bureau:design-scan`, remove
   `agent-eligible` to restore the verified pre-handoff state.
5. If no match exists, create one unlabeled issue in `TheLarkInn/bureau`. Its
   body must contain the marker, source commit, prioritized findings,
   file-and-line evidence, and testable acceptance criteria. Write the body to
   `target/bureau-review/issue.md` and pass it with `--body-file`; never
   interpolate findings into a shell argument.
6. Query the drafted issue directly and confirm that it is open and contains
   the marker. Do not add `agent-eligible` and do not remove
   `bureau:design-scan`; the verified handoff step owns those effects.
7. Write `target/bureau-review/publication.json` with the exact
   `created_issue_url` and `fingerprint` values you will publish.
8. Publish `success` exactly once with `outputs.created_issue_url`,
   `outputs.fingerprint`, finding counts, and the receipt as an artifact named
   `publication.json`. Never commit, push, open a pull request, close the source
   issue, or expose credentials.
