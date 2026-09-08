# Canvas reference

For everyday use, see the [README](README.md). These are the contracts to
preserve when changing the canvas.

## Hosts and paths

The app canvas and standalone dashboard share the same loopback server and
web assets. Normal dashboard use serves the embedded bundle; `--dev` executes
a trusted checkout. `--port <port>` fixes the otherwise ephemeral port.

| Setting | Resolution |
|---|---|
| Bureau binary | `BUREAU_CANVAS_BUREAU`, then `PATH`, then workspace `target/debug/bureau` |
| Run directory | `BUREAU_CANVAS_RUNS`, then `BUREAU_HOME/runs` |
| Default runs | The workspace/binary's WSL Bureau home when applicable; otherwise local `~/.bureau/runs` |
| Missing binary/config | Labeled bundled sample; validation has not run |

On Windows, automatically discovered WSL UNC binaries run through `wsl.exe`.
Share paths translate to Linux paths for CLI arguments. An explicit binary
override bypasses that bridge. See [binary lookup](lib/findings.mjs) and
[run-directory resolution](lib/runs.mjs).

## Requests

| Endpoint | Purpose |
|---|---|
| `GET /state` | Config, pipeline views, and validation state |
| `GET /runs` | Run summaries and liveness |
| `GET /runs/<id>/events` | CLI-backed event replay, falling back to `events.jsonl` with `source: "log"` |
| `GET /events` | SSE state updates and `run-event` notifications |
| `POST /intent` | Config edits and CLI-backed run controls |

`pause-run`, `resume-run`, and `cancel-run` take `{ run_id }`;
`reconcile-now` runs one foreground pass. The canvas never writes run markers.
Run listing/tailing polls the filesystem because WSL watch events are unreliable.
No `run_finished` event means live, not proof that a daemon is running.
Zero runs, a failed listing, and a pass claiming no work are distinct states.
A reconcile pass must not replace a run the reader already selected.

## Editing and validation

Saving proposes working-tree changes, never a commit, push, or PR.
[The YAML codec](lib/codec.mjs) preserves formatting.
[Pipeline saves](lib/pipeline.mjs) serialize per config directory, write,
validate, and restore the original on relevant findings or validation failure.
Findings elsewhere do not block that pipeline's save; advisories are non-blocking.

An empty findings list is not a verdict. Read both `validation.state` and
`validation.ok`:

| State | Meaning |
|---|---|
| Not `validated` | CLI did not complete validation |
| `validated`, `ok` | Config passed |
| `validated`, not `ok` | Config failed, possibly outside the selected pipeline |

Display CLI errors verbatim. Keep these meanings independently covered by
[verdict tests](test/statelab.test.mjs), not just shared text constants.

Field editors publish `data-dirty` and an unsaved marker. Confirm navigation
only for actual edits, not merely open editors. Switching to Relations must
preserve the pipeline draft.

Missing outcome branches fail closed; do not invent or persist an edge.
Layout is derived except for dragged positions in `.bureau/layout.json`:
`{ pipelines: { <name>: { steps: { <step>: { x, y } } } } }`.
Viewer/editor share layout; live/replay decoration must not move nodes.

## Run evidence

Dry runs report fake-adapter results from the run log, not predicted paths.
Selecting a step follows its output; no selection follows the current or last
finished step. Agent transcripts and deterministic `v2` results get structured
rendering; other output stays verbatim.

Displayed agent identities are config projections, not observations of a
spawn. A mismatch compares the recorded selection with today's config.

## Browser boundaries

Only `web/` is served. Shared host/browser rules live there; `lib/` may import
them, never the reverse. Each page must resolve its own module graph and bare
imports through its own import map. [Import tests](test/web-imports.test.mjs)
enforce this offline.

Every React Flow surface uses [MeasurementGuard](web/graph-measure.mjs) so
missed measurements cannot leave it blank. Expected edge counts come from the
model, not the renderer's projection; drawn edges need visible paint, not
just SVG length. See [graph design](DESIGN.md) for layout and navigation.

## State lab

From the repository root:

```sh
node .github/extensions/bureau-canvas/serve.mjs --dir .bureau
```

Open `/statelab.html` on the printed URL. The lab drives the production page,
not a separate UI. [Browser test commands](e2e/README.md) cover the matrix and
approved screenshots.

| Contract | Rule |
|---|---|
| Reachability | Real clicks from a fresh session; no direct component-state changes |
| Fixtures | Pin config and run endpoints to committed samples; keep every derived projection consistent |
| Coverage | Independent regions get separate axes; every value promises observable behavior |
| Exclusions | `structural`: impossible; `scoping`: redundant crossing with a probe; `harness`: fixture limit with a reachable, one-axis-away example |
| Rule counts | First-pruned counts depend on walk order; compare enumeration with/without a rule to measure its effect |
| Isolation | Block offsite HTTP/WebSocket traffic and host writes in every state; only approved read intents reach the host |
| Transitions | Cover both entry actions and declared undo actions |
| Settling | Require stable DOM samples and painted, model-counted edges; lab and matrix share the rule |

Harness examples must satisfy their rule and neighbor a combination excluded
by that rule alone. They are not claims that the two screens look identical.
Successful write tests use scratch config; held/refused writes exercise real
controls without mutating the host.

## Gallery evidence

Each matrix run stages its own gallery. Only a run that produced renders
replaces `e2e/gallery/`; unrelated suites leave it alone.

The named gallery audit gates missing states, index/records, PNG structure and
checksums, and proven screen-comparison failures. It must publish even when
tests fail. An unsettled render is marked explicitly; comparisons require
both renders to be proved settled. Animation or missing settle evidence must
never be presented as a verified fixed screen.

Checks must inspect what is visibly drawn: promised text, contrast, clipping,
overlap, edge paint, and audit notices. Presence in the DOM or a shared
constant alone does not prove that a reader can see the promised result.
