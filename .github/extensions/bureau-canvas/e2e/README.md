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
