# Bureau canvas

Understand local Bureau operations, edit config, and inspect live or replayed runs.

## Open

**In the GitHub Copilot app:** ask the agent to open the Bureau canvas.

**In a browser:**

```sh
bureau dashboard
bureau dashboard --pipeline my-pipeline
bureau dashboard --no-open       # print the loopback URL
```

Needs Node.js, a `bureau` binary supporting `validate --json`, and your
config directory (normally `.bureau/`). Missing config or a missing binary
shows a labeled sample, **not a validation pass**.

## Use

- Start at **Operations** for authoring provenance, validation, observed runs,
  and assignment safeguards. Count buttons filter and focus the run list.
- Open **Configuration**, an assignment, then its pipeline.
  **Transitions** remains the default pipeline authoring view.
- Use **Find**, **Review next**, and **Fit** to navigate graphs.
- Select a step to read its configuration or run output.
- Switch between **Design**, **Live**, and **Replay** to inspect runs.
- **Run reconcile now** can start real agent work. Pause, resume, and cancel
  use the Bureau CLI.

Saving edits the working tree; it never commits, pushes, or opens a PR.
Config still needs review and merge before execution. Pipeline saves revert
on relevant validation errors; advisories do not block saves.

### Operations overview

The app canvas and dashboard use the same overview and navigation. Opening a
specific pipeline still goes directly to that pipeline. Nothing starts merely
by opening Operations, refreshing evidence, or following a run link.

Authoring HEAD, changed/untracked config files, pending plans, and validation
are separate from execution. Reconcile reads the configured committed
remote/ref, not these local edits. The overview cannot observe the currently
adopted source through the existing CLI contract; it says so explicitly.
Recorded run-source revisions are **historical evidence**, not proof of today's
settings or daemon. Use `bureau doctor --json` to inspect local setup and
`bureau setup` to change the reviewed source.

Runs appear attention-first. Exact run links open the existing Live controls
or Replay timeline; assignment links open the real configuration editor.
Missing directories, unreadable/corrupt logs, partial appends, and bounded
previews are labeled rather than silently dropped. A quiet unfinished run
becomes stale after five minutes; event freshness is not a daemon heartbeat.
After a listing failure, the last readable snapshot is labeled historical and
counts become unknown. Paused and failed runs also count as needing attention.

Configured limits are declarations, not remaining capacity. Missing run cost
or incomplete local-factory accounting is **Unknown**, not zero; explicitly
recorded zero stays zero. SDK credits and native completion remain distinct
from the Bureau outcome. Cloud controls stay unsupported, and the public
website does not connect to your runner. For reviewed starting points, see the
[scenario catalog](../../../docs/scenarios.md).

### Managed read-only inspection

Set `BUREAU_CANVAS_READ_ONLY=1` when opening a dashboard for a managed runtime.
The existing `bureau dashboard` launcher inherits this environment; no new
CLI flag or dispatch profile is implied. For the managed maintenance runtime:

```sh
BUREAU_HOME=/var/lib/bureau-maintenance \
BUREAU_CANVAS_READ_ONLY=1 \
/opt/bureau/bin/bureau dashboard \
  --dir /opt/bureau/source/.bureau/maintenance --no-open --port 7331
```

Use the managed runtime's documented user, `HOME`, and executable environment.
`--dir` selects **displayed configuration only**. It does not configure a
reconcile profile, propagate a config subdirectory, or acquire an external
owner lock. The managed daemon owns dispatch; edit a separate authoring
worktree and review a config PR instead of saving into its immutable source.

Read-only mode disables configuration writes and run controls in the UI and
rejects them at both the HTTP and app-action backends. Inspection, navigation,
and config validation remain available. Run history is read directly without
repairing a partial log; native continuation approval is not queried here.
The app canvas also accepts `{ "readOnly": true }` to restrict an instance.
Input cannot relax environment enforcement or an already restricted instance.
An invalid environment value fails closed with an explanation. Unset or `0`
preserves ordinary local behavior. This policy does not stop existing work or
replace loopback request authorization.

### Local Copilot factories

A Copilot-role agent step can opt into **Local Copilot factory** instead of
ACP. Enter the reviewed provider, qualified runtime and canonical digests.
`runtime.profile: copilot-sdk-factory-v1` names Bureau's SDK capability
contract, not an SDK release or version. `runtime.version` must be the exact
expected `connect.version`; independent canonical runtime/provider tree
digests remain mandatory.
The required model credential is a reference declared in local `settings.yaml`,
never a pasted token. The role must grant `model:invoke`; the reference
does not grant forge tools or copy an ambient login.
Static arguments are a JSON object or null, never an encoded prompt or
`inputs_from` expression. The dedicated `set_copilot_factory` action accepts
the same structured declaration (or null to remove it). Other edits,
step rename/delete/clone and save preserve its nested data without rewriting
argument strings. Unknown fields and invalid JSON block save.
Argument array order and repeated scalar, object or nested-array values are
preserved exactly. Changed arguments replace only their opaque subtree;
unrelated editor-managed lists retain their existing semantics.

This is trusted executable configuration, not a launch button. Native
ceilings are optional and never filled merely by opening the editor.
An explicit concurrent-subagent ceiling must be between 1 and 500;
the editor and registered action share that capability-contract bound.
Provider/runtime qualification, full schema validation and permission checks
remain Bureau's responsibility. The account must independently satisfy
runtime rollout and billing eligibility. Offline checks establish neither
live access nor a generally available SDK release mapping.

Live and Replay show the SDK session, native run/attempt/status, measured
credits and original workspace separately from the Bureau run outcome.
Paused, hard-interrupted and indeterminate runs are not rendered as
completed. Missing accounting is not zero, and an inspection-process exit
cannot certify the original execution. Unsupported resume actions stay
unavailable; controls still go through the Bureau CLI, not app/cloud APIs.
Resume consumes the read-only `show --json` decision for the exact session
and native event prefix. A clean, never-admitted bootstrap can continue only
when Bureau explicitly confirms it. An unavailable or older CLI cannot
approve continuation from the browser's raw-log fallback.
Pause reasons remain visible without hiding Resume for an ordinary or
eligible bootstrap pause; genuinely running and terminal runs keep their
distinct controls.

The selected policy uses explicit pinned resources with ambient discovery
disabled. Repository `extraKnownMarketplaces` and `enabledPlugins` intent
does not imply all plugins are installed, trusted or available to factory
children. See the [local factory guide](../../../docs/getting-started.md#opt-in-local-copilot-factories)
for the required opaque SDK external state and recovery limits.

## Development

From a trusted Bureau source checkout:

```sh
bureau dashboard --dev
```

This serves checkout files instead of the embedded bundle. Changes under
`web/` reload connected development pages while preserving selection.
The app canvas accepts `{ "dev": true }` for the same behavior.

Run offline tests from the repository root:

```sh
node --test .github/extensions/bureau-canvas/test/*.test.mjs
```

On Linux, the pause-control tests generate fresh Engine and CLI evidence.
They require Cargo, Git, Python 3 and working unprivileged user/PID namespaces
(`unshare`). The CI lint job configures this prerequisite only on its
disposable runner; the tests retain real process isolation and fail normally
when it is unavailable.

[Reference](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/reference.md) |
[Design](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/DESIGN.md) |
[Browser tests](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/e2e/README.md) |
[Vendored modules](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/web/vendor/README.md)
