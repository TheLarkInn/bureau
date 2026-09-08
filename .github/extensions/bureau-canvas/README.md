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
