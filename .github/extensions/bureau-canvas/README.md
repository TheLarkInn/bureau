# Bureau canvas

Edit Bureau config, explore pipelines, and inspect live or replayed runs.

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

- Open an assignment, then its pipeline. **Transitions** is the default editor.
- Use **Find**, **Review next**, and **Fit** to navigate graphs.
- Select a step to read its configuration or run output.
- Switch between **Design**, **Live**, and **Replay** to inspect runs.
- **Run reconcile now** can start real agent work. Pause, resume, and cancel
  use the Bureau CLI.

Saving edits the working tree; it never commits, pushes, or opens a PR.
Config still needs review and merge before execution. Pipeline saves revert
on relevant validation errors; advisories do not block saves.

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

[Reference](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/reference.md) |
[Design](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/DESIGN.md) |
[Browser tests](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/e2e/README.md) |
[Vendored modules](https://github.com/TheLarkInn/bureau/blob/main/.github/extensions/bureau-canvas/web/vendor/README.md)
