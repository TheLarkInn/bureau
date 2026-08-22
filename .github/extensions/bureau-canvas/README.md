# Bureau canvas

A view of your `.bureau/` config: work source → assignment → role and repos →
pipeline, with a step diagram behind each pipeline.

## Two ways in

**In the GitHub Copilot app** — ask the agent to open the Bureau canvas. It
appears in the side panel, and the agent can drive it (`describe`, `focus`,
`reload`, and the editing actions).

**Standalone, no app** — the canvas is an ordinary loopback web server, so it
also runs on its own:

```sh
node .github/extensions/bureau-canvas/serve.mjs --dir .bureau
node .github/extensions/bureau-canvas/serve.mjs --pipeline my-pipeline --open
```

It prints a `127.0.0.1` URL. Only the launcher and the agent-callable actions
belong to the app; the view, the validation and the editing endpoints do not.

## What it needs

| | Why | Without it |
|---|---|---|
| a `bureau` binary with `validate --json` | the canvas never re-implements loading or validation, so it asks the CLI | falls back to a bundled sample and says so |
| a `.bureau/` directory | the config to show | falls back to the sample |
| Microsoft Edge | only for the browser test suite | `e2e/run.mjs` skips |

The binary is looked up on `PATH`, then at `target/debug/bureau`, and
`BUREAU_CANVAS_BUREAU` overrides both. Renderer modules are vendored under
`web/vendor/`, so **nothing here needs network**.

## Run observation and control

Alongside the config view, the same server exposes the run log
(DESIGN.md layer 3):

| | |
|---|---|
| `GET /runs` | one summary per run: `run_id`, `assignment`, `started_at`, `live`, `current_step` |
| `GET /runs/<id>/events` | the run's full event log, via `bureau show <id> --events --json` when a binary with that flag is on hand, else parsed straight from `events.jsonl` (`source: "log"`) |
| `GET /events` | the SSE channel; live runs forward each appended event as `event: run-event` with `{ run_id, event }`, and stop when the run's `run_finished` arrives |
| `POST /intent` | `pause-run`, `resume-run`, `cancel-run` with `{ run_id }`, shelled out to `bureau pause/resume/cancel` — the canvas never writes run markers itself |

The runs root is `BUREAU_CANVAS_RUNS` when set, else `runs/` under the bureau
home (`BUREAU_HOME`, default `~/.bureau`) — the same default the CLI uses.
Liveness is pure filesystem: a run is live while its `events.jsonl` holds no
`run_finished` event; no daemon is consulted. Tailing polls rather than
`fs.watch`, because the log is appended by another process and can sit on a
WSL share where watch events are unreliable; a one-second poll is cheap at
run-log sizes and behaves the same on every platform.

## Layout

Everything that decides meaning runs in Node with no DOM, which is why it is
testable without a browser:

| | |
|---|---|
| `lib/view.mjs` | projects the CLI payload into the config and pipeline views |
| `lib/layout.mjs` | deterministic positions, and one of four routes per edge |
| `lib/findings.mjs` | attaches `bureau validate --json` errors to what they name |
| `lib/trust.mjs` | flags untrusted input laundered into a write-capable step |
| `lib/advisories.mjs` | write-capable agent steps with no downstream check; missing `run` scripts |
| `lib/codec.mjs` | CST-preserving YAML round-trip, so a one-edge change is a one-line diff |
| `lib/actions.mjs` | the agent-callable actions, read and write |
| `lib/dryrun.mjs` | replays a pipeline over the `fake` adapter |
| `lib/runs.mjs` | run liveness, event-log replay and tailing, and the `bureau` shell-out |
| `lib/edit.mjs` | step-graph edits on a pipeline view, plus the editor's inline hints |
| `lib/pipeline.mjs` | the `save-pipeline` round-trip (write → validate → revert) and the `layout.json` sidecar |
| `web/` | draws what it is given; `web/editor/` is the pipeline editor and the read-only relation graph |
| `web/graph-measure.mjs` | the one measurement repair every React Flow surface renders, so a graph cannot stay blank |
| `web/statelab/` | the UI state registry and the state lab that renders it |

## The state lab

The canvas has more UI states than anyone can hold in their head, so they are
written down. `web/statelab/` is a pure registry — dimensions, the rules that
say which combinations are real, an entry path per state, and the controls and
copy each state promises — and `web/statelab.html` is the review surface over
it:

```sh
node .github/extensions/bureau-canvas/serve.mjs --dir .bureau
# then open /statelab.html on the address it prints
```

The lab renders each state by loading **the production page** into a frame and
walking that state's entry path with real clicks. There is no second copy of
the UI: if the lab shows it, that is what ships. It also reports each state's
dimensions, expected controls against what actually rendered, at either
recorded viewport — and a picker answers "is this a state?" for any
combination a reviewer assembles, naming every rule that rejects it.

Two numbers are easy to confuse, so the lab labels them apart. Each rule shows
how many tuples it was the **first** to prune, which depends on the order
dimensions are assigned in and is not "every tuple this rule forbids"; the
picker is what answers the order-free question for one combination. The
surviving set itself is order-independent, and an offline test re-enumerates
under permuted orders to keep it that way.

| | |
|---|---|
| `dimensions.mjs` | the axes the canvas varies along, and what each value promises on screen |
| `constraints.mjs` | why a combination is or is not a state — `structural` (cannot render) or `scoping` (renders, but adds nothing to cross) |
| `enumerate.mjs` | walks the product with pruning, so the totals are exact without materialising 10^8 tuples; per-rule figures count what each rule pruned *first*, in walk order |
| `probes.mjs` | crossings each `scoping` rule excluded, rendered anyway to hold the rule to account, plus content samples the dimensions do not model |
| `paths.mjs` | how each state is reached, as data |
| `fixtures.mjs` | four composable layers of offline payload: status, content, plan, selection |
| `driver.mjs` | the one interpreter for an entry path; the lab and the browser suite both run it |
| `checks.mjs` | what "the render matched the registry" means — controls, copy, contrast, overlap, clipping |

Two rules keep it honest. A state is reachable only if the driver can get
there by clicking, so nothing sets component state directly. And every walk
starts from a fresh session, because the assignment stack remembers its
expanded card in `sessionStorage` and a replayed path would otherwise toggle
it shut.

A third keeps it from flattering itself. The lab settles a walk by watching
for the host's own SSE state event, and that observer has a window it can
miss; when it does, the lab says the render was not proved settled rather than
presenting a possibly-raced screen as verified.

The Relations tab is why two of the rules are `scoping` rather than
`structural`: `EditorApp` keeps `PipelineEditor` mounted and merely `hidden`,
so a selection and an unsaved rename both survive the switch — draft safety
depends on it. Nothing of the draft is on screen there, so the crossings are
probes, and a content sample walks the round trip and requires the rename to
still be there on the way back.


## Rules worth knowing before changing it

- **The canvas never authors an error message.** It shows a `bureau validate`
  message verbatim or says it has not checked. Advisories are a separate,
  clearly-marked class and never block a save.
- **Editing is proposing.** Saving writes the working tree — never a commit, a
  push, or a pull request. PR review of the config repo stays the whole
  authorization model.
- **The dry run reports; it never predicts.** Steps and terminals come from the
  run log, not from following config edges.
- **An absent outcome branch is not an edge.** It fails closed to `abort`, and
  drawing or writing one would change what the file says.
- **Layout is derived and never persisted**, so it has to be deterministic.
  The exception is the editor's sidecar: `.bureau/layout.json` holds only the
  positions a user dragged to (`{pipelines: {<name>: {steps: {<step>: {x, y}}}}}`),
  keyed by name so it means nothing to the loader. Steps without a saved
  position fall back to the derived layout.
- **A graph may not come up blank.** React Flow measures a node once, from a
  ResizeObserver delivery, and drops the measurement if the viewport element is
  not queryable at that moment; the node's box never changes, so the observer
  never fires again and the node stays hidden for good. Every `ReactFlow` here
  renders `MeasurementGuard` as a child, which re-drives measurement from inside
  the store. `specs/graph-measurement.spec.mjs` withholds node measurements from
  the observer entirely and requires all three surfaces to draw anyway.
- **The pipeline editor never leaves an unloadable config.** `save-pipeline`
  renders the edited step graph, writes the file, and re-runs
  `bureau validate --json`; findings that name the edited pipeline revert the
  write and come back to the UI so the offending nodes can be marked.

## Tests

```sh
node --test .github/extensions/bureau-canvas/test/*.test.mjs   # offline, in scripts/lint.sh
node .github/extensions/bureau-canvas/e2e/run.mjs              # opt-in, needs Edge

cd .github/extensions/bureau-canvas/e2e/playwright             # browser, in scripts/lint.sh
npm ci && npx playwright install --with-deps chromium
npx playwright test
```

`e2e/run.mjs` looks for Edge where the Windows installer puts it, and
`BUREAU_CANVAS_EDGE` overrides that for an install kept somewhere else. It has
to run on the same OS as the browser: this harness talks to Edge over CDP on
loopback and hands it a `--user-data-dir`, and neither survives an OS boundary.
Pointing it at `/mnt/c/.../msedge.exe` from WSL launches a Windows Edge that
reads the POSIX profile path as a Windows one and exits without a word, so the
harness names that pairing and skips rather than failing with an empty error.
On a WSL checkout, run it with the Windows `node` against the UNC path.

The Playwright suite covers the assignment card's editable controls, which
the offline tests cannot see: what each field shows at rest, what opens when
it is clicked, and which actions it refuses. It runs against the host's own
hermetic mode — `BUREAU_CANVAS_TEST=1` points the binary lookup at a path
that does not exist, so every run serves the same bundled sample with no
`bureau` binary and no network. `scripts/lint.sh` skips it with a notice when
`node_modules` is absent, so a fresh clone still runs every other gate; CI
installs the browser, so there it always runs.

Rules that need a second repo or role to express — reordering repos, and the
read-only-primary warning — are asserted in the offline suite instead, since
the bundled sample registers only one repo.

Two suites pin the two-host split (Q12). `test/bundle.test.mjs` checks the
shared bundle headlessly: both pages pin the same import map, every web module
resolves only shared siblings and the vendored aliases, and `serve.mjs` only
launches the same `extension.mjs` endpoints. `test/smoke.test.mjs` boots each
host on an ephemeral loopback port — the standalone host as a spawned
`serve.mjs` against a fixture `.bureau`, the canvas host in-process with the
SDK stubbed the way every test stubs it (`BUREAU_CANVAS_TEST=1`) — and
asserts both answer `GET /state`, `GET /events`, and `GET /`. Both are
offline: no SDK, no network beyond loopback, no fixed ports.

The browser suite exists because the offline tests assert on served state and
cannot see the page. It checks for console errors, overlapping cards,
overprinted edge labels, one rendered path per edge, and a legend whose colours
match the edges — every one of which has been a real defect here.

`specs/state-matrix.spec.mjs` is generated from the state registry rather than
written by hand: it renders every reachable state at both recorded viewports,
walks every edge of the transition DAG, and writes a browsable gallery to
`e2e/gallery/index.html`. Nothing in it names a state, so a state added to
`web/statelab/` is rendered and asserted the moment it exists.
`specs/state-lab.spec.mjs` holds the lab itself to the registry, because a lab
that has drifted is worse than none.
