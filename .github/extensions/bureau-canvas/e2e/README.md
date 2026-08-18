# Bureau canvas browser e2e

Run the opt-in browser suite from the repository root:

```sh
node .github/extensions/bureau-canvas/e2e/run.mjs
```

The suite starts the canvas extension server in-process with
`BUREAU_CANVAS_TEST=1`, launches Microsoft Edge headless, and drives the page
through the Chrome DevTools Protocol with Node's built-in `WebSocket` and
`fetch`. It captures config and pipeline screenshots under
`e2e/screenshots/`.

This is intentionally not part of `scripts/lint.sh`: it needs Microsoft Edge
and network access to `esm.sh` for the renderer import map. If Edge is missing
or the network preflight fails, the command exits 0 and prints `skipped`.
