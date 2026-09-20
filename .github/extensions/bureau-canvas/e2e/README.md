# Canvas browser tests

Tests use bundled fixtures and loopback traffic, not agents or forges.
Installing browser dependencies needs network; running tests does not.

## Playwright

From `.github/extensions/bureau-canvas/e2e/playwright`, install prerequisites:

```sh
npm ci
npx playwright install --with-deps chromium
```

| Command | Runs |
|---|---|
| `npm run test:pr` | Everyday UI checks |
| `npm run test:matrix` | Registered states and transitions; writes `../gallery/index.html` |
| `npm run test:visual` | Approved screenshot comparisons |
| `npm run test:visual:update` | Replaces approved screenshots; review changes before committing |
| `npm test` | All Playwright suites |

`scripts/lint.sh` runs `test:pr` when `node_modules` exists; otherwise it
prints a skip notice. CI installs dependencies and also gates matrix/visual
checks. Visual failures publish `canvas-visual-differences`.

The **13 approved screens** retain the ten named Configuration, Design, Live,
Replay, and editor cases and add Operations at desktop and 375px, plus invalid
read-only configuration with missing run evidence. Legacy fixtures explicitly
select their named view; they do not capture the default Operations screen.
Operations fixtures pin time and parse event logs through the production
reader/reducer. They retain unknown costs, stale and corrupt logs, unknown
adopted source, and configured safeguards rather than measured headroom.

Inspect expected, actual, and diff images before updating Linux Chromium
goldens. Match the CI browser and fonts, retain the strict pixel comparison,
and keep the only mask on the existing host-specific header path. The shared
44px navigation adds a 69px row; Live/Replay may scroll to preserve the graph's
minimum usable height. Fixture screenshots are presentation evidence, not
proof of a running daemon or a managed deployment.

## Native engine-to-canvas checks

Presentation fixtures intentionally use a missing binary. They are not
evidence that the native factory engine or CLI works. The separate integration
check uses exact run IDs retained by offline fake-runtime engine tests and a
freshly built Bureau executable:

```sh
node .github/extensions/bureau-canvas/e2e/factory-native.mjs \
  /absolute/path/to/new/bureau /absolute/engine-evidence/runs \
  completed-run-id paused-run-id
```

It checks actual engine events through `show --events --json`, the
CLI-backed canvas endpoint, run summaries and snapshot/codec projections.
It rejects raw-log/sample fallback and does not start factories, invoke
models, or perform forge mutations. These checks validate Bureau's
engine-to-canvas contract, not SDK release compatibility, live runtime
eligibility or cloud access. Run it on the executable's OS.

## Edge (optional)

From the repository root:

```sh
node .github/extensions/bureau-canvas/e2e/run.mjs
```

Use Node on the **same OS as Edge**. Windows Edge requires Windows Node,
even for a WSL checkout. `BUREAU_CANVAS_EDGE` overrides the browser path.
Missing or cross-OS browsers print `skipped` and exit 0.

Screenshots go to `e2e/screenshots/` under the canvas extension. This harness
is not part of `scripts/lint.sh`.
