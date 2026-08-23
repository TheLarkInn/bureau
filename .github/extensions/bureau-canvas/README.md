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

The lab installs each state's request condition inside the frame before the
page's own modules run — a stalled save, a refused one, a payload that never
arrives — so a saving or a refused screen is something a reviewer looks at
rather than a note about a screenshot. Two states are the exception: a
`<script type="module">` is not fetched through `window.fetch`, so a blocked
renderer cannot be staged from inside the frame. For those the lab blanks the
stage and names the condition and the gallery file the browser suite wrote. It
used to return early and leave the *previous* state's render on screen beside
the new state's description, which is the one thing a review surface may not do
— present a screen it never produced as though it had.

### Visual regression checks

Normal pull requests compare ten approved product screens: empty config,
assignment overview, validation plus advisory, pipeline design, live run,
finished-run replay, clean editor, unsaved edit, and the two compact layouts
most likely to regress. A green `Canvas visual regression / 10 approved
screens` check means none changed.

```sh
cd .github/extensions/bureau-canvas/e2e/playwright
npm run test:visual
```

When a visual change is intentional, review it locally and replace the
approved images:

```sh
npm run test:visual:update
```

The one thing on these screens that is not the UI is the config directory the
header prints — `/home/runner/work/...` on CI, someone's checkout anywhere
else. It used to be baked into the baselines, so the suite passed on the CI
image and failed everywhere else, and the failure was read as font
rasterization and pinned away by distribution. It was not: the three editor
screens, which never draw a path, come out byte-identical between the CI
runner and a different Ubuntu release. So the path is masked instead, and the
approved images are portable. Rendering is pinned by the Chromium that
`@playwright/test` bundles, which `npm ci` installs from the lockfile.

On CI failure, the `canvas-visual-differences` artifact contains the expected,
actual and highlighted diff images.

The state matrix runs on every pull request, on `main`, and nightly, through
the `Canvas state matrix` workflow. It was scheduled-only at first, on the
reasoning that exhaustive rendering is discovery work rather than PR feedback.
That reasoning does not survive the arithmetic: `schedule` fires only on the
default branch, so until the workflow merged it had never run at all, and
`scripts/lint.sh` excludes `@matrix` — a change breaking two hundred states
would have merged green and gone unattributed. Rendering every state at both
viewports and walking every transition takes about as long as a fraction of the
Rust job, so they gate like anything else:

```sh
npm run test:matrix
```

Two numbers are easy to confuse, so the lab labels them apart. Each rule shows
how many tuples it was the **first** to prune, which depends on the order
dimensions are assigned in and is not "every tuple this rule forbids"; the
picker is what answers the order-free question for one combination. The
surviving set itself is order-independent, and an offline test re-enumerates
under permuted orders to keep it that way.

| | |
|---|---|
| `dimensions.mjs` | the axes the canvas varies along, and what each value promises on screen |
| `constraints.mjs` | why a combination is or is not a state — `structural` (cannot render), `scoping` (renders, but adds nothing to cross), or `harness` (renders in production; this harness cannot produce it, and it names the limit and the state that renders the same screen) |
| `enumerate.mjs` | walks the product with pruning, so the totals are exact without materialising 10^9 tuples; per-rule figures count what each rule pruned *first*, in walk order |
| `probes.mjs` | crossings each `scoping` rule excluded, rendered anyway to hold the rule to account, plus content samples the dimensions do not model |
| `paths.mjs` | how each state is reached, as data — and which of them need a route intercepted rather than a click |
| `fixtures.mjs` | four composable layers of offline payload: status, content, plan, selection |
| `driver.mjs` | the one interpreter for an entry path; the lab and the browser suite both run it |
| `checks.mjs` | what "the render matched the registry" means — controls, copy, contrast, overlap, clipping, and copy that reserves a region instead of drawing it |

Two rules keep it honest. A state is reachable only if the driver can get
there by clicking, so nothing sets component state directly. And every walk
starts from a fresh session, because the assignment stack remembers its
expanded card in `sessionStorage` and a replayed path would otherwise toggle
it shut.

A third keeps it from flattering itself. The lab settles a walk by watching
for the host's own SSE state event, and that observer has a window it can
miss; when it does, the lab says the render was not proved settled rather than
presenting a possibly-raced screen as verified.

A fourth governs the shape of the axes themselves, and it is the one most
easily broken by accident: **regions that render as siblings get their own
axis.** Whatever `ConfigView` draws unconditionally — the create bar, the
stack, the orphan strip, the relation disclosure — can be in any combination
with the others, so folding two of them onto one axis makes their crossings
*unrepresentable rather than excluded*, and an unrepresentable combination has
no rule to point at and no probe to answer for it. Only genuine alternatives
share an axis. That is why "nothing configured yet, and therefore everything
unreferenced" — the ordinary first-run landing, where the orphan strip is at
its fullest — is a state rather than a gap.

The same rule applies to axis *values*: every value must be one a payload can
actually take. A config can hold validation errors and advisories at once,
because `mergeAdvisories` concatenates them, so `invalid-advisory` is a value
rather than a pair the axis cannot express.

The sibling rule is why the field disclosures have two axes rather than one.
Every field keeps its own open state, so opening one does not close another,
and a single-valued `field` made *two editors open at once* unrepresentable —
a screen with no rule to point at. `fieldPair` gives that screen a tuple to
be, two structural rules say when it can exist, and a scoping rule plus
`probe--two-disclosures-open` say why the matrix reviews it once instead of
once per pair.

And it applies to fixtures, which is where it is easiest to break without
noticing, because a fixture is a payload rather than a value. A fixture may
only build a payload `buildState` could have served. `orphans` used to name a
role the pipeline actually uses and a pipeline the config did not contain,
which is a payload `lib/view.mjs` cannot produce — `orphanItems` derives the
list from the config's own roles, repos and pipelines, keeping the ones
nothing references. The render said so out loud: the header counted one
pipeline while the strip called another unreferenced, and the graph drew the
"unreferenced" role wired to its pipeline. Adding a card without its relation
node, or a repo without its `usedBy`, had the same shape. Every coupled
projection moves together, or the state under review is one no user can reach.

That was the same mistake three times, so it is a gate rather than a comment
now: `test/statelab.test.mjs` builds the base payload through the real
`relationView` and requires every fixture's graph to be **exactly** the one its
own config implies. Both directions, and both halves: a node for every listed
item and no node for anything unlisted, an edge for every pipeline or repo an
assignment names and for every role a pipeline's steps name, and *no edge
the config does not imply*. Deleting `multi-repo`'s relation patch fails it by
name; so now does adding an edge between two nodes that are already there, or
dropping a pipeline's role edges — both of which the containment-only form let
through, and both of which make the graph state a relation the config does not.

That rule also decides what a fixture may not attempt. `orphans` leaves a role
and a *repo* unreferenced rather than a pipeline, because a pipeline is also
keyed into `state.pipelines`, and that entry is a `pipelineState` — view,
layout, handles, containers, summary — which only `lib/` can build and which
`test/bundle.test.mjs` forbids `web/` from importing. Answering a fabricated
payload with a fabricated projection is the defect, not the fix.

Each value also has to promise something a render can fail. An axis that
contributes no expectation is an axis three states can share one set of
assertions across, which is how a `dirty` field editor that was quietly clean —
it typed the values the payload already held, so Save stayed disabled — passed
as dirty. The lifecycle axis asserts the save button's own state, and the run
axis asserts what only the selected run's log can produce: the step decoration
live folds out of it, and the span replay's timeline takes from it.

A lifecycle value asserts the *treatment* as well as the words, because for
several fields that is the whole message. `deriveWorkSource` gives a paste three
different answers and the editor draws three: an exact derivation offers the
save, a host it does not recognise refuses outright — no preview, no save — and
a URL with no search query derives a filter it had to *infer* and offers the
save with a warning. The third is neither lifecycle value, so it is a content
sample; the second is `invalid`. Asserting only the sentence would let a
refusal render as ordinary advice, and an inferred filter render as an exact
one — which is the hazard `lib/worksource.mjs` exists to prevent.

The Relations tab is why two of the rules are `scoping` rather than
`structural`: `EditorApp` keeps `PipelineEditor` mounted and merely `hidden`,
so a selection and an unsaved rename both survive the switch — draft safety
depends on it. Nothing of the draft is on screen there, so the crossings are
probes, and a content sample walks the round trip and requires the rename to
still be there on the way back. The isolation is asserted both ways: the
probes require the editor panel gone behind Relations, and `tab: pipeline`
requires the relation graph gone behind Pipeline.

The matrix will not **act on the host**, and for a while that was discharged by
excluding every screen a write leads to. `./intent` is how the page writes *and*
how it reads, so the route that answers it in the browser is named by the
intent's `kind`: `derive-work-source` and `resolve-repo` are let through, and
every other kind — `save-plan`, `discard-plan`, `save-pipeline`, the create
intents — is held wherever a state installs the route. The one intent that
still reaches the host is the delete **preflight**, which no state intercepts:
`lib/crud.mjs` `remove()` answers an unconfirmed delete with the preflight and
writes nothing, so the round trip is a read. It is not free, though — the host
republishes its own state while answering, which is why the preflight cannot be
reviewed over an injected config and why the rule that says so is a `harness`
rule. `.bureau/` is untouched by the run.

That route arrived for the field saves and was not extended to the other four
families, which kept rules saying `save-plan` "calls `applyPlan` on the host's
config directory" long after nothing of the sort could happen. The distinction
those rules blurred is the one this registry exists to keep: **"the harness may
not press this button" is not "this screen cannot be rendered."** A rule marked
`structural` claims the second, and claiming it waives every obligation the
registry has — only `scoping` rules owe a crossing probe, so no test asks a
`structural` rule for anything. Two of the six screens were consequently
asserted nowhere in the repository at all.

So the plan bar's save, the pipeline editor's save, a refused create and a
refused run control are ordinary states now, each reached by pressing the real
button under `stall-intent` or `fail-intent`. Both ends are exact rather than
timing-dependent: stalling pins the in-flight branch on, and a refusal is the
branch that renders each surface's own sentence. Each declares the 500 the
browser logs as its own, the way `data: render-error` declares its failed
module request — the registry stays the only place that says which failures are
a state and which are a bug.

One of them was hiding a defect, which is the argument for rendering them made
concretely. `edit: saving` promised the Save button was withheld while the write
was in flight. `EditorToolbar` disabled it on `!dirty || invalidNumbers` and
nothing else, so it stayed live for the whole round trip and a second click
raced a second write against the first one's revert. The registry asserted the
truth; the exclusion is what meant nobody ever rendered it to find out.

`transport: playing` came back on the same reasoning, with a narrower
resolution. Its *position* really is a function of the clock, but the matrix
gallery is a browsable render rather than a pinned baseline — only the ten
`@visual` screens are compared — so a scrubber a frame further on costs nothing.
The state asserts the label and aria name flipping to Pause and leaves the
position alone. What it replaces is a Play button the timeline shipped and no
state ever pressed.

The label has a deadline, though, and that is what `playing-outlasts-only-the-
longest-run` is for. `useReplayOverlay` stops itself at `range.end` and puts the
button back to Play, so the assertion is true only while the run has time left:
2.0s for the live log, 5.0s for the paused one, 10.0s for the finished one, all
at speed 1. `judge()` re-samples a disagreeing render for up to five seconds, so
on the first two the assertion's lifetime is at or below the retry budget — a
transient disagreement, which is likely while the graph is still re-decorating,
would be converted into a hard failure rather than retried away. The finished
run is the only span with margin, so it is the one the label is reviewed on.

Two further exclusions in this family remain, and they are narrower still.
`a-refused-control-is-a-live-control` is structural in the strict sense:
`web/replay/replay.js` draws a picker and a timeline, and its transport moves
the reader's own position without posting anything, so there is no control there
whose refusal could be shown. Crossing it with replay produced three ids for one
render, which the distinguishability gate caught by name. And
`mutations-need-a-selected-step` now covers the editor saves, whose path is a
rename plus a click — an editor with nothing selected has nothing to rename, and
without the rule the click would silently never happen. The successful write
paths still belong to `specs/editor.spec.mjs` and `specs/controls.spec.mjs`,
each against its own scratch config, and what a finished run's controls should
show is pinned by `runActions` in `test/overlay.test.mjs`.

The transition graph has two kinds of edge, for the same reason. An `enter`
edge is a prefix relation — the child's path is the parent's plus one
operation — and that is all a path-derived graph can ever produce, because
every entry path points away from the landing. Half of what a user does is the
other direction: collapse the card, cancel the create, go back to the Pipeline
tab, leave replay for design. None of it was under test, and a disclosure that
opens and will not close is exactly the defect a matrix exists to catch. So
each reversible control declares its undo in `REVERSIBLE`, the suite enters the
child, applies only that undo, and then holds the render to the **parent's**
expectations. Acyclicity is asserted over the entry subset alone: a return edge
is a cycle by definition, which is what "the way back" means.

That declaration has to be worth something, so an offline test holds every
`REVERSIBLE` entry to at least one edge — matched by the control that *opened*
it, not by its undo, since Live and Replay share the Design button and one
shared return edge would otherwise answer for both. It was not free: because an
entry edge is a prefix over the whole path, *including which fixture the state
publishes*, a field whose resting lifecycle entry named its own payload
diverged from the resting card before the click and produced no edge at all.
The forge-signals disclosure was in that position — declared reversible, walked
never, on the one control this work had changed from open-only to a toggle. Its
resting state now opens on whatever the card already holds.

The repos editor is the case where that trade does not pay, and it is worth
knowing which is which. Its `dirty` is a single click — the reorder — so rest
and dirty are themselves a prefix pair, and taking the fixture off rest to buy
an edge from the card would have paid for it by deleting the edge that proves
one click on Move-up makes this editor dirty. It keeps its fixture; its
disclosure gets its edges from the findings probe pair instead.


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
- **A control with a box may still be invisible.** The verdict measures every
  control a state promises, not a standing list of regions, and reports a box
  that hangs off either edge of the viewport or is cut away entirely by an
  `overflow: hidden` ancestor. Scroll containers are not clipping — content
  below the fold of the editor's side panel is one gesture away — so the two
  are told apart per axis. Overlap is a rule rather than a list of pairs:
  anything in normal flow that prints over a box sharing its parent.
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

The gallery is replaced by the run that filled it and by no other. A run
renders into a staging directory named after its own process, and the teardown
publishes it over `e2e/gallery/` only if something was written there — so a
state the registry has since dropped cannot sit in the gallery inviting a
reviewer to sign off on a surface that no longer exists; a run that renders
nothing (`test:pr`, `test:visual`) leaves the gallery alone; and two runs in
one checkout cannot delete each other's renders. That first distinction cannot
be read off the command line, because Playwright reports `grep` as `/.*/`
whatever was passed; and it deliberately does not consult a clock, because a
filesystem that records mtime more coarsely than `Date.now()` reports it dates
a shot written after the run began as older than it. A directory is empty or it
is not.
