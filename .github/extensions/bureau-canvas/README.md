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
bureau dashboard
bureau dashboard --pipeline my-pipeline
bureau dashboard --no-open
```

It prints a `127.0.0.1` URL. Only the launcher and the agent-callable actions
belong to the app; the view, the validation and the editing endpoints do not.
Normal dashboard use serves the web bundle embedded in the Bureau binary.
The source-level `node .github/extensions/bureau-canvas/serve.mjs` entry point
remains available for server tests and debugging.

### Live design

```sh
bureau dashboard --dev
```

Development mode polls `web/` and sends a reload event to every open dashboard
page. Run it only from a source checkout you trust: this mode intentionally
executes that checkout's server and web files rather than the embedded bundle.
The standalone browser and a Bureau canvas opened with `{ "dev": true }` use
the same canonical assets, disable static caching, and reload without restarting
the server. Session storage restores the expanded assignment, graph mode,
selected run, and selected step. `--port <port>` pins the otherwise ephemeral
loopback port.

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
| `GET /runs` | one summary per run: `run_id`, `assignment`, `pipeline`, `started_at`, `live`, `current_step` |
| `GET /runs/<id>/events` | the run's full event log, via `bureau show <id> --events --json` when a binary with that flag is on hand, else parsed straight from `events.jsonl` (`source: "log"`) |
| `GET /events` | the SSE channel; live runs forward each appended event as `event: run-event` with `{ run_id, event }`, and stop when the run's `run_finished` arrives |
| `POST /intent` | `pause-run`, `resume-run`, `cancel-run` with `{ run_id }`, plus `reconcile-now`; all shell out to the matching `bureau` command — the canvas never writes run markers itself |

The runs root is `BUREAU_CANVAS_RUNS` when set, then `BUREAU_HOME`. Otherwise
it follows bureau rather than this process: when the workspace (or the resolved
binary) lives inside a WSL distro, the bureau home lives there too, so the root
is that distro's `~/.bureau/runs` addressed through its `\\wsl.localhost\` share
— a canvas hosted on Windows would otherwise look in `C:\Users\...\.bureau` and
find no runs at all. Share paths translate back to Linux paths when passed as
`--runs`, so replay and run control reach the same root. Failing all that it is
`runs/` under the host's own bureau home, the same default the CLI uses.
Liveness is pure filesystem: a run is live while its `events.jsonl` holds no
`run_finished` event; no daemon is consulted. Tailing polls rather than
`fs.watch`, because the log is appended by another process and can sit on a
WSL share where watch events are unreliable; a one-second poll is cheap at
run-log sizes and behaves the same on every platform.

The pipeline toolbar polls that listing in every graph mode. Its Live badge is
the count for the pipeline currently open. **Run reconcile now** executes one
`bureau reconcile --now` pass and prevents a duplicate click while it runs.
Because that command returns only once the pass is over, the button re-reads
the listing itself rather than waiting for the poll, and reports which of three
things happened: it started a run for this pipeline, it claimed no work, or the
listing could not be read. A run it started is shown only when nothing was
already being watched — a pass never moves the overlay off a run the reader
chose, and a refused pass moves nothing at all.

Zero is an explicit idle state, not evidence that the reconcile process
stopped: a reconcile loop becomes a run only after it claims eligible work. A
failed listing is shown separately and retried rather than silently presented
as zero.

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
| `web/live/transcript.js` | parses a step's captured output into blocks: an agent's Copilot CLI transcript, a deterministic step's contract line, or raw process output |
| `web/` | draws what it is given; `web/editor/` is the pipeline editor and the read-only relation graph |
| `web/graph-measure.mjs` | the one measurement repair every React Flow surface renders, so a graph cannot stay blank |
| `web/assignment-state.js` | which assignment is open, and which open field editors actually hold unsaved work |
| `web/statelab/` | the UI state registry and the state lab that renders it |

### Draft safety on the config card

Every field editor on an assignment card publishes `data-dirty` on its root,
from the same value it uses to decide whether to offer its own save, and draws
the shared `.draft-mark` — "unsaved changes" — while that value is true. The two
controls that navigate away from an open card (collapsing it, and opening its
pipeline) read `DIRTY_FIELD_EDITORS` from `web/assignment-state.js` and confirm
only when something really is unsaved.

That is one rule rather than two: the pipeline editor's own `navigate` has
always gated on its `dirty` flag. The config surface used to ask whether an
editor was *open*, so opening the repos editor to read it and then clicking the
pipeline beside it demanded you dismiss a prompt about discarding changes you
had not made — and a prompt that cries wolf trains the reader to dismiss the one
that was protecting real work.

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
| `constraints.mjs` | why a combination is or is not a state — `structural` (cannot render), `scoping` (renders, but adds nothing to cross), or `harness` (renders in production; this harness cannot produce it, and it names the limit and the nearest state this harness can reach — one named axis away, not the same screen) |
| `enumerate.mjs` | walks the product with pruning, so the totals are exact without materialising 10^9 tuples; per-rule figures count what each rule pruned *first*, in walk order |
| `probes.mjs` | crossings each `scoping` rule excluded, rendered anyway to hold the rule to account, plus content samples the dimensions do not model |
| `paths.mjs` | how each state is reached, as data — and which of them need a route intercepted rather than a click |
| `fixtures.mjs` | four composable layers of offline payload: status, content, plan, selection |
| `driver.mjs` | the one interpreter for an entry path; the lab and the browser suite both run it |
| `checks.mjs` | what "the render matched the registry" means — controls, copy, the ink and generated content that decide whether promised words are the ones on screen, contrast, overlap, clipping, and copy that reserves a region instead of drawing it |

Two rules keep it honest. A state is reachable only if the driver can get
there by clicking, so nothing sets component state directly. And every walk
starts from a fresh session, because the assignment stack remembers its
expanded card in `sessionStorage` and a replayed path would otherwise toggle
it shut.

### A verdict is a result, and a result requires a run

The pipeline side panel is headed `Validation (0)` when it has no findings for
the pipeline it describes, and says a sentence about it. An empty findings list
arrives three different ways, and the panel said the same thing about the first
two:

```
clean — bureau validate would pass
```

Under `data:validated` that is true — the CLI ran and returned nothing. Under
`data:fixture` there is **no bureau binary**: `extension.mjs` reads the bundled
payload, which carries no `findings` key at all, so the list is empty for want
of anyone to fill it. Nothing ran, and the panel reported a pass. Two feet
above it the header reads *"Showing bundled sample; bureau binary not
available."* — the two statements cannot both be true, and the one under the
heading "Validation" is the one a reader takes as the verdict.

`state.validation.state` has always distinguished them; the panel simply never
consulted it. It does now, and says `not checked — bureau validate did not run`
when nothing ran.

The registry is the more interesting half. This branch did not introduce the
sentence — it introduced the **assertion** of it, wiring both `data` values to
the same `panelVerdict(combo, true)`, so seven states and fourteen renders
certified a verdict no validator ever gave. That is a mark standing in for a
check that was never made, which is the defect this branch exists to refuse,
found inside the branch's own expectations. `data:fixture` is also the state a
user lands on *precisely because* the binary is missing, so it is where an
unearned "would pass" is most likely to be believed.

The offline test asks the two values' own derivations whether they disagree,
rather than comparing each against a literal — a test naming both sentences
goes on agreeing with itself once someone makes them one sentence again — and
pins which of the two is the unrun one, because "they differ" alone is
satisfied by swapping them.

| mutation | offline | browser |
|---|---|---|
| `data:fixture` wired back to the clean sentence | **1 failed** | — |
| `emptyVerdict` returning the clean sentence unconditionally | — | **14 failed** |
| restored | **422 passed** | **653 passed** |

The fourteen are the seven `surface:pipeline+data:fixture` states at both
viewports, which is exactly the set the registry certifies.

### `validated` is not a verdict either

The fix above reads `state.validation.state`, and that is the third way an empty
list arrives. `lib/findings.mjs` sets `state: "validated"` whenever
`bureau validate` **returned JSON** — accepted *and* rejected alike — and records
which of the two in `ok`, a field `emptyVerdict` never read. Meanwhile
`pipelineFindings` scopes the panel's list to the pipeline on screen, and
`targetFor` routinely aims a finding somewhere else: `roles`, `assignments`,
`repos`, a bare `file`, or another pipeline.

So a config the CLI **rejected** for a reason that names a role or `repos.yaml`
leaves the open pipeline with an empty list, and the panel announced:

```
clean — bureau validate would pass
```

while the header on the same screen read *"Validation findings"*. That is the
identical defect one branch over — a mark standing in for a check nobody made —
except that this one asserts the exact opposite of the command's answer, under
the heading a reader takes the verdict from.

The panel has three honest things to say, so it now says three:

| `validation` | sentence |
|---|---|
| not `validated` | `not checked — bureau validate did not run` |
| `validated`, `ok` | `clean — bureau validate would pass` |
| `validated`, not `ok` | `no findings for this pipeline — bureau validate rejected the config elsewhere` |

`invalid` could not catch it. That fixture hand-places a finding on the very
pipeline it opens, so the panel is never empty there and the `validated && !ok`
branch never renders — the defect lived precisely in the gap between the two
values, which is why `invalid-elsewhere` is a value on the `data` axis rather
than a variation of an existing one. Its fixture rejects the config for reasons
that name no step and no pipeline, and the offline test asserts that premise
rather than assuming it: a later edit that attached a finding to that pipeline
would fill the panel, stop rendering the branch, and otherwise pass while
covering nothing.

The rule moved to `web/panel-verdict.mjs` for `step-refs.mjs`'s reason — `web/`
is the only tree the browser can reach, and `app.mjs` cannot be imported without
a browser, so a verdict spelled inside it is a rule the offline suite can only
read. The registry imports the same three constants rather than re-spelling
them, so the expectation is bound to the page instead of agreeing with a second
literal.

| mutation | offline | browser |
|---|---|---|
| `emptyVerdict` reading `state` alone, ignoring `ok` | **2 failed** | **15 failed** |
| `PANEL_ELSEWHERE` collapsed into the unchecked sentence | **1 failed** | — |
| restored | **450 passed** | **681 passed** |

The fifteen are the seven `surface:pipeline+data:invalid-elsewhere` states at
both viewports plus the gallery audit. The config-surface states pass under the
mutation, correctly: that surface draws no panel.

The second row is the fix's own defect, found by the finish review after the
first was closed. Sharing one export between the page and the registry is what
stops the two drifting — but it also means the table test compares
`emptyVerdict`'s output against the very constants it returns, so both sides
move together and the wording is asserted only against itself. Two of the three
pairs were pinned by other tests; `elsewhere` against `unchecked` was pinned by
none, so collapsing them left **every gate green** while the panel told a reader
that a config the CLI had checked and rejected "was not checked". That is the
original defect with the labels exchanged. Distinctness is now asserted over the
set of three, and each sentence is held to its own opening besides — a set
survives any swap, and the openings are what make a swap fail.

### An opening is not an answer

Round twenty-three's finish review asked the same question of that fix, and
returned the same defect one clause further along. Each of these three sentences
puts its **subject** first and its **verdict** after the dash:

```
no findings for this pipeline — bureau validate rejected the config elsewhere
└──────────── pinned ────────────┘└─────────── pinned by nothing ───────────┘
```

So rewriting only the tail satisfied every pin above it. `PANEL_ELSEWHERE`
became `no findings for this pipeline — bureau validate accepted the config`:
the set stayed three, the opening still matched, and **every gate stayed
green** — offline, browser and gallery alike, because the page and the registry
read the one constant and moved together. The panel reported a config the CLI
had **rejected** as one it had accepted, under the heading a reader takes the
verdict from. Round twenty-two's defect, surviving round twenty-two's fix,
because the fix pinned where each sentence starts and the defect lives in where
it ends.

The verdict clause is now pinned as an exact **partition**: three clauses —
`did not run`, `would pass`, `rejected the config` — spelled **in the test file**
rather than imported, each required in its own sentence and absent from the
other two. The literals are the point. Every other comparison in the suite reads
`PANEL_*`, necessarily, and a comparison against the constant is one the
constant wins by definition; these three are the only independent statement of
what the sentences must *mean*, and so the only thing a reversal can fail
against. A partition rather than three `includes` checks, so a verdict moved
onto the wrong sentence fails on the row it arrived at as well as the one it
left.

### An import is not a call

The same review asked what made the page *use* the rule, and nothing did.
`panel-verdict.mjs` is pure and fully pinned, and none of that reaches a reader
unless `SidePanel` calls it — so putting the clean sentence back inline, the
literal the module was extracted to delete, restored the original false verdict
with all **450 offline tests green**. `web-imports.test.mjs` sees the import,
but an import is not a call, and a module can be imported and ignored.

`app.mjs` cannot be imported without a browser — React and `@xyflow/react` come
in through bare specifiers — so the rule is read from source, in both
directions: the call must be **there**, and the three sentences must **not** be.
Either clause alone is half a check, because an `emptyVerdict` call left beside
a second inline copy is exactly the drift the extraction was for.

| mutation | offline | browser | before this round |
|---|---|---|---|
| `PANEL_ELSEWHERE`'s tail rewritten to "accepted the config" | **1 failed** | **681 passed** | **450 passed, 0 failed** |
| `PANEL_CLEAN`'s tail replaced with `did not run` | **1 failed** | — | **450 passed, 0 failed** |
| `SidePanel` spelling the clean sentence inline | **1 failed** | — | **450 passed, 0 failed** |
| restored | **452 passed** | **681 passed** | 450 |

The browser column on the first row is **measured, not reasoned**: a full matrix
run over the mutated constant — 540 renders, the whole gallery audited and
reported complete — passed **681 of 681**. It is the expected answer and it is
still worth the three minutes, because the reason it passes is the reason the
defect is invisible: the page renders the constant and the registry asserts the
same constant, so both sides move together and 28 renders drew a false verdict
that every one of them approved. A browser cannot referee a claim whose two
sides are one value; only a literal written somewhere else can.

The second row is the vacuity guard: a clause moved onto the wrong sentence must
fail as loudly as one deleted, or the partition is only a presence check wearing
a partition's shape.

**No production module changed.** `panel-verdict.mjs` and `app.mjs` are
byte-for-byte what they were — the canvas renders exactly what it rendered
before. What changed is whether two of its claims are able to fail.

### A render that was not proved settled says so

The matrix waits for a state's own signature to stop changing before it judges
and captures it, and under a fully-parallel run it does not always get there
inside the budget — the residue is CPU contention rather than a step the page
takes, and it is measured in `matrix-fixtures.mjs` beside the constant that
sets the window. What matters is what happens to the renders that miss it.

They used to be published indexed and captioned like any other, which made two
different claims untrue at once. A reviewer scrolling the gallery was signing
off on a frame a contended worker happened to be drawing, with nothing to
distinguish it from a screen the product actually holds. And the twin audit
compared those frames as evidence: on one run it reported three declared twins
as "no longer draw the same screen", and rendering the same six states one at a
time produced byte-identical signatures for all three pairs. The audit had
published its own contention as a finding about the UI.

So every render now carries whether it was proved settled, `settled.json`
records it beside `signatures.json`, the figure is marked in the gallery, and a
mismatched twin is a `broken-twin` only when **both** sides were proved —
otherwise it is an `unproven-twin`, which says the difference is a frame rather
than a finding. `web/statelab/lab.mjs` has always refused to present an
unsettled render as verified; this is the browser suite keeping the same rule,
and the two consumers of one registry no longer disagree about what a render is
worth.

Stability alone turned out not to be the whole of "settled". React Flow lays
its edges out in a pass after it has measured the nodes, and the lull in
between is long enough to satisfy three agreeing samples — so a render could be
filed as settled on a graph of disconnected boxes. The mark introduced to tell
a screen from a frame was vouching for frames, and the twin audit published the
difference as a finding about the UI: two states declared twins were reported
as no longer drawing the same screen on runs where one had drawn its four
relation edges and the other had not. Each graph now publishes what it was
handed as `data-graph-edges`, and a render is finished only once every visible
graph has drawn at least that many — a claim the surface makes about itself, at
the one place that knows the number, rather than a list of selectors that falls
out of date the next time a graph gains an edge. Drawable rather than declared,
because React Flow draws nothing for an edge whose endpoint is missing, and
waiting on a line that was never coming is a wait with no end.

The same pass is why the relation disclosure now mounts its graph on open
instead of keeping it behind a shut `<details>`. A closed disclosure keeps its
subtree in the document and Chromium still reports client rects for parts of
it, so a graph nobody could see was measuring nodes and laying out edges — and
losing that race sometimes, which made two renders of one state describe
different documents.

Waiting for the edge pass is not the same as reporting that it never came, and
for a while only the first happened: a graph that would never draw held the loop
to its deadline and then left an amber note about the frame. That is a mark
standing in for a check, so a graph still incomplete at the deadline is now an
ordinary failure naming the surface — `Config relation graph declared 3 edge(s)
and drew 1` — rather than two numbers and no screen to go and look at.

What counts as "never came" is **two questions asked of three numbers per
graph**, and every layer of that was learned by finding the escape in the
simpler shape. A single flag over the whole render answered `true` for a render
carrying no graph *yet*, so one sample taken between a `<details>` toggle and
React committing the subtree switched the failure off for the rest of the
budget — on precisely the states that open a graph. Keying to the graph's own
name closed that. A *run* of consecutive incomplete looks closed the next
layer, a latch being permanent — but a run is reset by any complete look, so a
graph flashing its edges on and off all budget was exempt again. A bare
cumulative total closed that and opened another: never being reset, it was
spent by two harmless early relayouts, turning one ordinary late miss into a
hard failure on exactly the animating states that must never fail for it. And
rebuilding the tally from each look made unmounting a third reset, so a graph
blinking in and out of the document accumulated nothing.

So each graph keeps how many looks it was on screen, how many it was missing
edges for, and its current run — and a graph is failed if it broke and *stayed*
broken (`run >= SETTLE_REPEATS`) or was missing edges for a material share of
the looks it was present for. Neither question is derivable from the other, and
each is wrong on the case the other catches. The tolerance that matters lives
somewhere else entirely: nothing is reported about a graph that is complete on
the final look, whatever its history, so a healthy surface caught mid-relayout
on the way past is never accused. `web/statelab/checks.mjs` owns the threshold
and both rules, so the State Lab and the matrix cannot answer the question
differently. The lab marks where the matrix fails, because it renders one state
into a live frame a human is watching rather than gating a run.

A render the gallery published and holds no record for is marked too. The
screenshot is written before its record, so a worker killed between the two
leaves a PNG that is absent from every audit that reads records — not counted as
unsettled, not compared as a twin, and presented to a reviewer captioned exactly
like a proved render. `auditUnaudited` names those, and a record that exists and
will not parse is excluded from it, so each render is answered once rather than
told both "could not read its record" and "filed no record at all".

An undeclared match is read the same way in the other direction. A pair that
*matches* is evidence of sameness only when both sides were proved, exactly as
a pair that differs is evidence of a difference only then: two renders each
captured a beat early are each missing whatever had not arrived yet, and they
collide on a signature neither will still have a moment later. That is an
`unproven-match`, and it goes in the amber note with the unsettled count rather
than the red banner, because its own words say the difference is a frame.
Keeping the count out of the alarm while letting the findings in was the hole
in the first version of the rule, so the two halves are now split by kind, in
`isDrift`, where a finding cannot reach the alarm by being appended to the
wrong list.

The first thing the rule found is the one it should: on a clean run the only
renders it marks are the two for `transport:playing`, the replay state whose
scrubber advances on a 100ms interval. That state is animating by design — the
registry already says so, and asserts the Play button's label rather than its
position for exactly that reason — and it is the one screen in the gallery a
reviewer must not read as fixed. The marker says so on the figure.

Which is also why an unsettled render gets its own amber note and never the red
banner. That banner asserts one thing — this gallery is not the whole matrix,
or not every state in it draws its own screen — and an unsettled render is
neither. Counting it as a finding would have lit the alarm on every clean run
from the first, and a review surface that cries wolf about itself is read as
background by the second time. An offline test holds the two apart.

And a twin that really has broken now says what it broke in. The renders a
declared twin names carry their signature as well as its digest, so the finding
quotes the first element the two disagree on. Before, a broken claim was a dead
end: two screenshots, two hashes, and a difference that is usually one attribute
on one element — the thing a person cannot find by eye and a diff finds at once.

A render whose record carries no settle evidence at all is the third finding,
and it used to be none. `auditMotion` asked its two questions of the records
carrying a boolean, so a record without the field fell out of both lists and was
reported by neither — deleting the field from the record writer left the whole
matrix green over five hundred renders the artefact then made no motion claim
about. The leniency was written as backwards compatibility with galleries
published before the rule existed, and there are none: the audit reads only the
records this run staged. It protected nothing and hid the one thing it was
watching for, so absent evidence is now `unproved` and reported with the rest.

### The words are found where they live, not where they are contained

A promise that names an element is checked on that element. Most of this
registry's promises name none — **1,068 of 1,190** — and a plain phrase used to
be settled against `body.innerText`, which reports words drawn in transparent
ink, inside a container at `opacity: 0`, or behind a `filter`, exactly as the
DOM-only check always did. So the rule the scoped promises get is the rule all
of them get: a plain phrase is found on the element holding it in its **own
direct text**, and judged on the ink that reaches the screen.

*Own direct text* is the load-bearing half, and it is a narrowing in the
**loosening** direction — which is why it needs a fixture rather than a comment.
Reading `innerText` instead would add every ancestor that merely *contains* the
words, and an ancestor is honest whatever the element actually holding them is
painted in. Since one holder painting the words honestly keeps the promise, an
innocent wrapper would vouch for a label the reader cannot see: a false
negative, in the module that exists to catch exactly that.

Both clauses now have a fixture that fails on them alone, and neither could
before — every carrier fixture was a lone leaf, so the walk could have read
`innerText` and agreed with all of them:

| fixture | the screen | answer |
|---|---|---|
| `a masked promise` | the words in a leaf drawn in nothing, under an honestly-painted wrapper carrying no text node of its own | `ink: false` |
| `a shared promise` | two holders of the same words, the unreadable one **first** | `ink: true` |

| mutation | result | before this round |
|---|---|---|
| `ownText` reads `innerText` | **1 failed** — names `a masked promise` | **84 passed, 0 failed** |
| `honest` judges `holders[0]` alone | **1 failed** | **84 passed, 0 failed** |
| restored | **84 passed** | 84 |

The second row is why the pair exists rather than one fixture. "One element
painting the words honestly keeps the promise" is a claim about *all* the
holders; with a single holder per phrase, a walk that judged the first one alone
was indistinguishable from the rule, and would report a screen the reader can
read perfectly well as unreadable.

**No production module changed.** `checks.mjs` is byte-for-byte what it was, so
the renders and their verdicts are identical. What changed is whether two of its
clauses are able to fail.

### A mark can be attached, spelled right, and painted in nothing

Every rule above binds where a mark attaches and what it says. None of them
asked whether a reviewer can see it. `border-color:transparent` beside
`border-width:2px`, and `color:transparent` on the caption's `::after`, leave
every selector matching, every mark landing, `unmarked` empty and every computed
value readable and *different from its neighbours* — over 540 renders on which
nothing is drawn at all. `hidden`, `display:none` and `opacity:0` do the same to
the notice a bad run depends on, and the offline suite cannot tell, because it
holds `notices` as a string and a hidden banner serializes exactly like a shown
one.

So both are asked whether they are painted, by a browser — and asked of the
pixels rather than of the properties. An alpha channel was only the first of the
ways to be silent: `border-style:none` computes the border's width to zero,
`font-size:0` and `clip-path:inset(100%)` on the caption's `::after` leave the
phrase present in `content` at full alpha, and a foreground set to its own
background is painted and unreadable. That list has no end, so no list of
properties closes it.

Each channel the stylesheet paints is therefore screenshotted and read back for
the colours Chromium actually put there. The mark's own ink — `SETTLED_INK`, the
one the sheet interpolates and the amber notice is written in — has to be among
them on the stamped figure's border *and* on its caption, and on neither channel
of any figure that was not stamped.

Which figures wear it is a *set*, not a sample. While the stamped figure was
read as the first one carrying the attribute and the control was every figure
without it, a mark landing where the run never asked simply moved those figures
out of the control group: stamping every compact render — one line in
`figureTag` — left the stamped figure saying it, no unstamped figure saying it,
and half the gallery telling a reviewer it was not proved settled. Exactly the
shots handed to `applyMarks` carry the attribute, and no others.

The attribute being exact is still not the mark being exact, because the mark is
what a reviewer *sees*. Two rules keyed on position rather than on the attribute
leave the set perfectly correct — `.card::after` says the phrase outside every
figure, where a figure-scoped read cannot see it, and
`.shots figure:last-of-type:not([data-settled]) img` draws the amber border on a
figure no sample happened to take. So the phrase is counted over the whole
document, and the ink is swept across **every** figure's two channels from a
single full-page shot. That page is built from a slice of the registry rather
than all of it, which is what makes "every figure" affordable: what is under
test is the marker and the stylesheet, and neither has any idea which states it
was handed.

Both channels are asked **separately**, and the sweep reports the ink it finds
**outside all of them**. Collapsing the two to "either one carries ink" approves
`border-style:none` on the mark's own border — a rule still present, still
correctly spelled, still interpolating the ink, painting nothing on the render
it judges — on the strength of the caption alone. And a channel is a *box*,
while a mark can be drawn where no box is: an `outline` at an `outline-offset`
lands wholly outside the border box it belongs to, and a generated block sits
below the caption whose box was measured. Both put the mark's exact colour on
figures nothing stamped while every sampled rectangle stays clean, so ink found
where no channel is is a mark on a screen that was never marked. Coverage is
rounded outwards by less than a pixel, because a box measured in CSS pixels and
a shot measured in device pixels disagree at the edge, and a sliver of a genuine
border left uncovered would be reported as a mark drawn nowhere.

Both instruments are then required to say how much they looked at, and **at
what**. A `channels` that returns one figure fewer, or a `sweep` that returns
fewer verdicts than it was handed regions, leaves the positional read comparing
an empty slice — and `.some` of nothing is `false`, indistinguishable from a
figure checked and found right. Counting closes that and closes nothing about
*which* figures were measured: `[...document.querySelectorAll(selector)].fill(…)`
returns the owed number of entries, every one of them the stamped figure, so
every count is exact, every duplicated region carries the mark's ink, `wrong` is
empty and `stray` is zero — over a sweep that never looked at one unmarked
figure. A count is not a correspondence, so the figures are named: exactly the
page's own shots, in page order, each owing the two channels the positional read
assumes.

The words themselves are asked of the region they occupy rather than of the box
that holds them, and asked five things: they are in the render tree, there is a
region, the ink is in it, it is not one flat colour, and the ink does not fill
it. A box is bigger than its words, and both of the gaps that opens were real.
`font-size:0` leaves a notice its padding, so a two-pixel stripe of its own ink
painted there passes ink-present and not-flat over a blank bar. And a phrase
painted over its own background is unreadable but not flat — a block of solid
ink blends into what is behind it at its own edges and answers with two
colours — so only the ink's *share* of the region separates words from a block:
glyphs leave most of their line box unpainted and a block leaves none of it.

All of those are statistics about colour, and colour statistics can be
manufactured without drawing a glyph: transparent type over a one-pixel
repeating gradient of the expected ink has the ink, several colours, and a small
share, and says nothing. So the last question is whether the ink is *the
words'*. Their colour is taken away — `color:transparent` on the element under
test, which changes no geometry, so this is not the comparison of two layouts
this suite rejects elsewhere — and the region is read again. Real type
disappears and the pixels move; a decoy painted behind it is unmoved, because it
never depended on the colour the words are written in.

That is also why each caption carries an otherwise empty element for the mark to
be written into. The phrase is CSS `content`, and pseudo-content has no box
anything can measure; the caption's own box is shared with the viewport name, so
flatness measured over it is satisfied by a neighbour of the thing under test.
The slot is a box that holds the phrase and nothing else.

The notices are asked the same five things, over the rectangles their text
occupies — and so is the promised phrase inside them, on its own, because
`strong { visibility:hidden }` hides exactly the words the notice exists to say
while leaving the sentence around them perfectly readable. The advisory is asked
*alone* as well as beside the alarm — an advisory-only run is the ordinary
result of a full matrix, so the notice a reviewer meets on almost every good run
was the one with no browser check at all.

Comparing a stamped render against an unstamped one is deliberately not the
question: the mark changes the border's width, so the geometry moves and the
pixels differ whether or not anything was painted. Ink present is about paint
alone. What counts as drawn is decided once, in `e2e/playwright/drawn.js`, and
injected into the page by both checks, so the two cannot drift into asking
different questions about the same artefact.

### Three checks that read something other than what a reviewer gets

Painting is not the only place a check can agree with a page nobody will see.
Three more sat one step to the side of what they claimed.

**The row parser disagreed with the browser about case.** `attributesOf` already
took the *first* of a repeated attribute, because that is what Chromium takes.
It keyed them by the name as written, so `SRC="./broken.png"` and
`src="./real.png"` were two attributes to the check and one to the browser: no
repeat reported, the second value read and approved, and a gallery of broken
images. Names are folded to lower case now, which makes the collision a repeat —
which is what it is. The parser also returns what it could not account for, and
that has to be empty: it understands double-quoted values and nothing else, and
an attribute it cannot see is one it silently reports absent.

**The CI-publication check read a line, not the step.** The gallery path was
required to appear as some trimmed line of the workflow. `upload-artifact` reads
a leading `!` as an exclusion, so a `path:` block that lists the gallery and then
excludes it keeps the line, uploads an artefact, stays green, and publishes
nothing. The check finds the `actions/upload-artifact` step and reads that step's
own `with.path`, its own `if:`, and the `if:` of the job that holds it — a step
that says `always()` inside a job that never starts publishes exactly as much as
a step that says nothing, and `always()` is required of the step rather than
merely something truthy, since the run a reviewer most needs the gallery from is
the one that failed.

Those four readers were hand-rolled scans over the file's text, and each round
found another spelling that walked past one of them: an input named `if:` under
`with:`, a folded `name: >-` continuation sitting deeper than the keys it was
measured against, an over-indented comment in that line's place, `if :` spaced
away from its colon, `"if"` in quotes. Two more closed the argument. `"\u0069f"`
is the key `if` — a double-quoted scalar is escape-decoded, so those letters are
not in the file at all — and a comment written shallower than a job's own keys
ends no mapping, though a scan for "the next line no deeper than this" reads it
as the end of the job. Both defeat the reader that fails **open**: the job's
condition is asserted to be *absent*, so a job that never starts reads as a job
with nothing in its way.

So the workflow is parsed now, by the same vendored YAML the extension itself
ships, and spelling stops being a variable: the step is the one whose `uses:`
names the action, the job is the mapping it is genuinely inside, and a key is a
key however it is written. The seven hostile workflows are kept as a fixture
rather than run once and thrown away, because what defeated these readers five
times over was never the case anyone thought to write a test for.

**And none of them asked whether the workflow ever starts.** Three rounds went
into that upload step — its patterns, its own `if:`, its job's `if:`, the parse
that reads a key however it is spelled — over a workflow whose `on:` nothing
had ever read. Delete `pull_request:` and every reader above answers exactly as
it does now, green, about a run that publishes nothing for the reviewer of a
pull request, which is the only reader this gallery has. The events are read
from the same parse, and `on` is three shapes and a trap: a scalar, a sequence
or a mapping, and under YAML 1.1 the key is the *boolean* `true`. The vendored
parser is 1.2 and is asked rather than assumed. Nine spellings hold it,
including one written `"\u006fn"` and `pull_request_target`, which merely begins
the same way and starts nothing on a pull request.

**The teardown was proved only through the seam tests use.** `globalTeardown`
forwards `audit` and `resolve` so the offline suite can watch the hand-over, and
both of its tests injected a stand-in. The un-injected path — the only one
Playwright ever takes, because it calls a global teardown with the config and
nothing else — was never walked, so a guard returning early when no seam is
passed left every offline test, every identity check and the whole browser suite
green while a real run's teardown did nothing. It is now called the way
Playwright calls it, over a staging directory the test owns, and required to
produce that audit's whole observable behaviour for an empty one: the directory
is discarded, and the answer names the directory it looked in. The returned
sentence alone is a literal a guard can write out without looking at anything,
and the discard alone is only the destructive half — a stand-in that deletes
staging and reports has thrown a run's renders away without publishing them.

### The audit could not be found among the run's checks
Every rule above produced a *finding*, and nothing was answerable for any of
them. A full matrix could publish `This gallery is not the whole matrix` in red
at the top of the artefact, and the only things carrying that news were a
console line and a banner nobody is gated on — which is the same defect this
whole branch is about, an amber mark standing in for a check that found
something, made by the instrument whose job is to find it.

The audit's findings are two different kinds of statement, and the split is
still worth keeping — but both kinds now gate. Some are arithmetic over a file
list: did this run write every render the registry asked for, does every
published render belong to a state, did every published render file a record
that can be read, and did a declared twin get both of its sides rendered. The
rest — two states drawing one screen, a declared twin that parted — *are*
comparisons between two renders.

The comparisons were reported and not asserted for several rounds, excused as a
signal that "still drifts on some renders because content occasionally arrives
after a surface has held still for a poll interval". That excuse had stopped
being true, and it was the last unasserted mark in the instrument built to
remove unasserted marks. A claim is only made once **both** renders have proved
they stopped changing; every other pair becomes an `unproven-*` finding, which
is routed to drift and never asserted. So the drift the gate was traded against
is filtered out before a claim can exist.

Measured, because a stale excuse is exactly how this survived: two full matrix
runs over one tree, 512 renders each at the time of the measurement, agree on
**511 of 512** signatures. The
one that moves is `…mode_replay_run_finished_transport_playing` — the single
state the registry declares in motion, `settled: false`, therefore unproved,
therefore drift by construction. Among the renders that can reach a claim the
disagreement is **0 of 511**.

`partitionFindings` still splits the three ways once. `incomplete` and `claims`
are two separate assertions in the audit spec, so a failing run says which kind
of thing it got wrong, and drift stays in the amber note where it cannot cry
wolf. `unchecked-twin` sits with the arithmetic, because "this run rendered one
side or neither" is not a comparison at all — it is the same arithmetic as a
missing render.

Where it is asserted matters too. A throwing `globalTeardown` does fail a run —
that was measured rather than assumed — but it fails it as an error belonging to
no test: the reporter says "1 error was not a part of any test", and the run's
own record of what it checked does not contain the check. So the audit is a
teardown *project* — `specs/gallery.audit.spec.mjs`, which runs from the same
vantage point once every worker has finished — and it is a named check that
fails by name. It also asserts that it *ran*: a run that published no index
produces no findings, and without that guard the check would be green for a run
that never looked. Its project sets `retries: 0` deliberately, because
publishing is a rename: a retry finds staging already gone, reads that as "this
run rendered nothing", and would turn a real finding green by asking twice.

### The lab certified frames the matrix rejected

Settling is two claims — the DOM has stopped changing, and every graph has
finished its edge pass — and for a round only the second was shared. Both
surfaces imported `graphsDrawn`, so they agreed about a graph mid-draw, and the
stability half stayed written out at each call site where the two copies were
not the same rule at all: `matrix-fixtures.mjs` required `SETTLE_REPEATS`
consecutive looks with an unchanged signature, and `lab.mjs` required nothing,
leaving on the first failure-free look.

`transport:playing` is what that costs. Its scrubber advances every 100ms, so
its signature never holds still; the matrix records it as not proved settled and
the lab handed a reviewer its first frame with every line green. Two consumers
of one registry answering the same question differently is the contradiction
this registry exists to remove, and sharing only the half that had already been
caught left it standing in the other half. Leaving on the first clean look is
also what made a *late* failure invisible — an error that arrives a beat after
first paint was never sampled, because nothing looked again.

`settleStep` is now the whole rule, both halves, in one place. And the note had
to change with it: "a graph on this render has not drawn all of its edges" was
true while `settled` meant the edge pass alone and became a false sentence the
moment stability joined it, because a playing replay has drawn every edge it
declared. `unsettledReason` picks between the three causes, and an offline test
holds each to its own words.

### The relation graph's edge count answered for itself

`data-graph-edges` was derived from the same projection React Flow renders,
which is exactly right for the barrier it was written to serve — *has the edge
pass happened* — and is no evidence at all for *are the right edges there*: a
projection that dropped every edge declares zero, draws zero, and satisfies it.

That was tolerable while the count was only a barrier, because both numbers moved
together by construction. `undrawn-graph`, added last round, changed what the
number is *for*: it fails a render whose graph never drew the edges it declared,
which is a claim about the screen. Against that claim a self-reported count does
not merely fail to help, it answers by lowering the bar to nothing — the exact
shape this branch exists to remove, committed by the newest instrument on it.

The previous revision of this file said the relation graph was "now counted
against `relationView`'s own edges, derived from the config rather than from the
render". That counting existed, in `e2e/run.mjs`, and it is a real assertion —
but `e2e/run.mjs` is the Edge harness, which is opt-in and named by no workflow,
no npm script and no step in `scripts/lint.sh`. So the sentence described a check
that ran nowhere in CI, and the pipeline graph's in-page count had the same
defect the sentence claimed was fixed for the relation graph.

All three surfaces now count from their own model instead: the relation graph
from the config it was handed, the editor from `view.steps` and the terminals its
edges reference, and the pipeline from the overlay *plan* — `overlayPlan`, split
out of `overlayEdges` so the remap-and-dedupe that is the overlay's meaning stays
on the counted side and `flowEdge`, the projection, is the untrusted half.

The consequence is that `undrawn-graph` becomes a real correctness gate across
all 540 renders in CI, rather than one assertion in a harness that runs only when
someone has Edge. Both directions were measured on the same sabotage — a
`toFlow` that hands React Flow no edges at all:

| `data-graph-edges` derived from | result |
|---|---|
| the projection (before) | **26 passed, exit 0** — the defect is invisible |
| the config (after) | **18 failed, exit 1** — `Config relation graph declared 4 edge(s) and drew 0` |

Independence is a property of the *call site*, and nothing inside `drawableEdges`
can enforce it, so `test/graph-edges.test.mjs` holds it offline by reading how
each of the three surfaces computes the attribute.

### A laid-out edge is not a drawn one

The other half of that comparison was `drawnPath`, and it asked SVG geometry:
`path.getTotalLength() > 0`. Geometry was the right question for the barrier —
*has the layout pass produced a path yet* — and it survives every way there is
to not be on screen. One line of `.react-flow__edge-path { display: none }`
took every edge off every graph in the matrix and each one still reported
`drawn` equal to `declared`, on 653 green tests.

Length was necessary and was being read as sufficient. It is now length *and*
paint: `display` and `opacity` walked to the root, because neither resolves an
ancestor's value into the child's computed one the way `visibility` does, and a
stroke that is `none`, fully transparent or zero-wide is a line that was laid
out and never inked. A client rect still cannot stand in for any of it — an edge
between two vertically aligned handles is a straight vertical line with no
width, which is the ordinary shape of a pipeline stacked in a column.

### The rule against self-reporting was itself only a mark

That test asked its question as a *ban*. It collected the lines containing
`drawableEdges(`, and failed a surface whose line matched `flow.(nodes|edges)`.
Mutation-checked in one direction it looked convincing: revert a surface to
`drawableEdges(flow.nodes, flow.edges)` and it fails.

It fails for the one spelling it was shown. The same projection under any other
name passes, and so does the same projection under the *same* name written
across two lines, because the line holding the call name then holds no argument
at all:

| the pipeline viewer's count, mutated | old rule | now |
|---|---|---|
| `drawableEdges(flow.nodes, flow.edges)` | **fails** | **fails** |
| `drawableEdges(projection.nodes, projection.edges)` | passes | **fails** |
| the same call across three lines | passes | **fails** |
| `drawableEdges(nodesOut, edgesOut)` | passes | **fails** |

So the defect this round exists to remove could walk back in under a rename or a
line break, beneath a green test written to prevent exactly that. A ban answers
"is this one known-wrong shape present", which is a question about the shapes
someone thought of.

The rule is now an allowlist, and reads the complete call expression by scanning
balanced parentheses rather than by reading a line. Each surface's approved
model-side argument is recorded in the test:

| surface | approved argument |
|---|---|
| relation graph | `source.nodes, source.edges` |
| pipeline editor | `[...view.steps.map((step) => ({ id: step.name })), ...terminals], edges` |
| pipeline viewer | `sources, planned.map((item) => item.remapped)` |

Whitespace is collapsed before comparing, so reformatting an approved expression
is not a finding while changing what it reads is. That inverts the default: an
unreviewed counting expression fails whether or not anyone predicted its shape,
and the only way to change how a surface counts is to edit the table where a
reviewer is asked the question.

What it holds is the counting expression, not the provenance of every name
inside it — a surface that rebound `sources` to the projection elsewhere in the
same function would still read as approved. Recorded here rather than implied,
because the weaker claim is the true one.

### Absence, and the writes that are not a Save

Two families of screen were missing from the matrix for as long as it has
existed, and both were missing for the same reason: the axes were written from
what a config *has* and from what a field editor's Save does.

The first is what the canvas says when a field is unset. An assignment written
but not yet filled in draws six sentences no other state produces — `no
source`, `no pipeline`, `no filter`, `no approval label`, `branches: not set`,
`no repos`, and the mark the pipeline row draws instead of a door into a
pipeline that does not exist. The `bare-assignment` fixture puts one beside a
configured card, and `probe--bare-assignment` pins the glance line as a whole
rather than by substring, so a card that dropped one fallback and kept the rest
fails. Rendering it is also what found the row that answered an absence with
punctuation: work source read `? · ?`, on the field that decides whether the
assignment does anything at all, and now says `no work source`.

The second is a request whose control is not a field editor's Save. `fieldState`
models both ends of every save through `FIELD_SAVE`, and two of this UI's
in-flight requests have no entry there: the delete **preflight** — a read, the
question Delete asks before it is a prompt — and the repo **registration**,
which is a write. Both hold their own control while they are in flight —
`Checking…`, `Adding…` — which is the whole of what stops a card queueing three
preflights or writing `repos.yaml` twice, and neither end of either had ever
been drawn. That the preflight is a read is also why it needs its own route in
both hosts: `reachesHost` deliberately lets an unconfirmed delete through, so
neither `stall-intent` nor `fail-intent` can touch it.

Rendering the registration's refusal is what found the sentence it was refused
in: reordering and registering post the same `set-repos`, and both fell back to
"could not save those repos" — telling a reader who pressed **Add to registry
and this assignment** that a repo list they never reordered had failed to save.

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
intents — is held. That route is installed under **every** state, not only the
ones that ask for a condition. The distinction matters more than it sounds: a
deny that only exists where a state opted into it makes "no write reaches the
host" a fact about which paths happen to click what, and the next state to press
a Save it never modelled would have rewritten the contributor's own `.bureau/`
with every test still green. `holdWrites` is the floor, a state's own
`stall-intent` or `fail-intent` layers over it, and a write that reaches the
floor is recorded and fails the state that posted it by name.

The one intent that still reaches the host is the delete **preflight**:
`lib/crud.mjs` `remove()` answers an *unconfirmed* delete with its referrer
report and writes nothing, so the round trip is a read, and `reachesHost` names
it alongside the two derivations. The nested `confirm` is what separates it from
a real deletion, so the predicate reads the body rather than the `kind`. It is
not free even so — the host republishes its own state while answering, which is
why the preflight cannot be reviewed over an injected config and why the rule
that says so is a `harness` rule. `.bureau/` is untouched by the run.

That route arrived for the field saves and was not extended to the other four
families, which kept rules saying `save-plan` "calls `applyPlan` on the host's
config directory" long after nothing of the sort could happen. The distinction
those rules blurred is the one this registry exists to keep: **"the harness may
not press this button" is not "this screen cannot be rendered."** A rule marked
`structural` claims the second, and claiming it waives every obligation the
registry has — only `scoping` rules owe a crossing probe, so no test asks a
`structural` rule for anything. Two of the six screens were consequently
asserted nowhere in the repository at all.

The same substitution had been made twice more in the editor family, and an
independent review found it after the rest of this was written.
`a-clean-editor-can-only-select-what-it-already-draws` and
`a-move-needs-a-step-the-fixture-already-draws` both opened their reason with
the *fixture* — the sample pipeline ships a deterministic and an agent step —
and then claimed structural impossibility on the strength of it. `PipelineEditor`
selects and drags any step it draws, so a user whose pipeline holds a decision
step reaches both screens with no mutation at all; only this bundle cannot. Both
are `harness` rules now, each naming its limit and the state that stands for it,
and re-enumerating without either one keeps two further combinations, which is
the `costless` check confirming they were hiding something.

`stands` was, for a while, checked for nothing but *existence* — any state id in
the registry satisfied it — so the field that says "here is where a reviewer can
go and look at the screen this rule removed" could name something unrelated and
still read green. That is a label that reads as evidence and is not, which is
the defect `unbroken` already removes from crossings, left standing on the rule
kind where the cost is highest.

It is held to two properties instead, and it is worth being exact about why they
are not "renders the same screen". The two screens differ by one axis on
purpose, because that axis is the thing the harness cannot reach: the *clean*
selection of a decision step stands on a **created** one, which carries a dirty
bar the excluded screen would not have, and claiming the renders match would be
the same overclaim in the other direction. So the standing state must satisfy
the rule — a state the rule itself excludes is not somewhere the harness can get
to — and changing exactly one of the axes the rule reads must produce a
combination the rule rejects **that no other rule rejects too**. That last
clause is the round-later half: adjacency asked only "does this rule reject the
neighbour?", so a `stands` could qualify by sitting next to a combination some
unrelated rule had already excluded — a combination that is not a screen this
harness rule hides at all, and in one measured case the only neighbour a
`stands` had. A witness that is itself not a screen proves nothing about the
screen being stood in for. A reviewer looking at `stands` is one named axis
away from the screen the rule removed, rather than somewhere else entirely, and
that is now a check rather than a promise. It is asked against the rule's own
predicate, not against `enumerate`'s worked example, because which tuple that
example is depends on the walk order — a check that passed or failed on `ORDER`
would be a mark by another route.

What the lab may *say* about `stands` is held too, and in the rendered DOM
rather than in the helper: `harnessNotes` words both sentences once, and
`specs/state-lab.spec.mjs` reads them back out of the constraint list and the
picker, then refuses the sentence they replaced anywhere on the page. Holding
the helper alone left either call site free to hard-code the old overclaim with
the offline suite still green.

Kinding is a judgement, and the check that catches this one is narrower than the
judgement: a rule whose verdict is computed from `SAMPLE_STEPS` is deciding from
the fixture's inventory, which is a statement about the bundle and not about the
product. `test/statelab.test.mjs` requires every such rule to be `harness`, and
asserts that it matched at least one rule so the check cannot pass by vacuity if
the constant is ever renamed.

So the plan bar's save, the pipeline editor's save, a refused create and both
ends of a run control are ordinary states now, each reached by pressing the real
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
to hold mid-request or to show a refusal on. Crossing it with replay produced
three ids for one render, which the distinguishability gate caught by name. It
reads `postsRunIntent` rather than the `refused` prefix, so the held end of the
same round trip is scoped by the same rule that scopes the refused one — written
the other way, `holding-pause` crossed with replay would have been enumerated
and its path would have hunted for a Pause button on a surface that draws none.
And `mutations-need-a-selected-step` now covers the editor saves, whose path is a
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


## The step log

Live and replay show what the focused step actually did, under the graph.
Click a step to follow it; with nothing selected it follows the step the run
is inside, then the last one it finished.

An agent step streams a Copilot CLI transcript, where a tool call is a marker
line with indented argument and result lines drawn in box characters. Those
glyphs are layout, not content, so `web/live/transcript.js` consumes them and
the panel draws a block per tool call instead of echoing the drawing. A
deterministic step streams one line of the v2 step-result contract, rendered
as its outcome, message, outputs and artifacts. Anything that is neither —
a stack trace from a failed step — is kept byte-for-byte in a preformatted
block, because reflowing a stack trace destroys it.

Output events are not retained by the overlay reducer, which is why both mode
hooks also hand the panel the raw events.

### The agent identity a step was run with

A role names an agent in config (`/bureau:implementer`), and the identity that
reference selects depends on the adapter: Copilot keeps the `plugin:agent`
qualifier, Claude takes the bare agent name, and a path contributes its file
name. `step_started` carries the role, the configured reference and that
selected identity, and the log head names it.

Both sides of the comparison are projections of config, never observations of a
spawn. The run log is written before the worktree guard has captured the
worktree's originals, so it may only use the pure form — materializing the
agent there would have the guard record the copy as an *original* and commit it
onto the run branch. `bureau validate --json` reports the same projection per
role under `agents`, for the config as it stands now, and that is where the side
panel's **Agent identities** section and the head's expected value come from.

So when the two disagree, what has moved is the config — the run used one
identity and the config now selects another. The head says exactly that: `this
run used implementer; the config now selects bureau:implementer`. It used to say
`invoked implementer; expected …`, which claimed an observation nothing on this
path makes, about a comparison that against an unchanged config cannot fail at
all. The **Agent identities** list says `selects` for the same reason, where it
used to draw an arrow that reads as resolution.

That the logged name really is the one the adapter passes to `--agent` is a
property of `crates/bureau/src/config/validate.rs`, not a coincidence: an
`agent` that is neither a plugin invocation nor a `.md` path does not validate,
and that is the only shape for which the pure and the resolving forms differ.
Both halves are pinned — `the_logged_name_is_the_one_the_adapter_invokes` and
`the_two_forms_diverge_on_the_shape_the_loader_rejects`.

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
- **The page can only load what the page can fetch.** The dashboard serves
  `web/` and nothing above it, so a module under `web/` that imports `../../lib/…`
  is a 404 at runtime rather than a build error — the import throws, the surface
  never mounts, and every browser spec that needed it times out on an element
  that was never coming. That is why a rule both trees must obey lives on the
  reachable side (`web/step-refs.mjs`, imported by `lib/edit.mjs`, so the
  editor's draft delete and the host's saved-view delete cannot drift the way
  three separate `removeStep`s did; `web/step-edit.mjs` carries the editor's
  draft transforms there for the second reason — a rule the offline suite
  cannot import is a rule it can only check by *reading its spelling*, which
  passes when the refusal is deleted and fails when the line is reformatted).
  `test/web-imports.test.mjs` holds the
  boundary offline: every relative specifier under `web/` resolves to a file
  that exists inside `web/`, and every bare one is declared in the import map of
  **the page that loads it**. Per page, because an import map belongs to a page:
  unioning all three and checking every module against the union let
  `editor.html`'s declarations answer for a module only `index.html` loads, and
  answered for `statelab.html` — which carries no import map at all — with
  another page's. The rule now walks each page's own module graph, starting from
  the `src` and `await import(…)` inside its inline `<script type="module">`,
  which is where both surfaces actually begin. A specifier it cannot read from
  source — an interpolated template, or an `import()` whose argument is not a
  literal at all — is reported as its own finding rather than skipped, because a
  specifier the scanner never captures is one no later rule is ever asked about.
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

That "no network" was an arrangement rather than a check until
`e2e/playwright/offline.mjs` existed. The hermetic mode, the committed runs
root and the absence of any fixture that asks for a host are all true, and none
of them could be contradicted by a run: a `fetch("https://example.com/")` added
to `web/app.mjs`, swallowing its own rejection, left all 147 browser tests
green. Both fixture families now sit on an offline floor, in two parts because
they fail in opposite directions. `page.on("request")` sees every request the
page makes — including one a spec's own `route.continue()` sends straight out
without consulting the handlers underneath it — and cannot be got round, which
makes it the detector. A `page.route("**/*")` registered before any spec's
routes is the refusal: Playwright runs handlers newest-first, so it is consulted
last, and a request no state claimed is aborted before it leaves the machine.
The matrix reports a destination as a per-state finding beside its held writes,
so the news is *which screen reached out*; the PR suite throws at fixture
teardown. `offsite()` fails closed — a URL the parser cannot read is refused
rather than waved through, since the one input nobody anticipated is exactly
the one that must not be the one that passes.

Both of those mechanisms are HTTP-shaped, and a WebSocket is not an HTTP
request: it raises no `request` event and `page.route` never sees it. So a
`new WebSocket("ws://192.0.2.1:9/")` added to `web/app.mjs` left the suite green
with nothing recorded — the same hole, through the one door the first floor
could not watch. `page.on("websocket")` and `page.routeWebSocket` now do the
same two jobs for sockets, and `offsite()` judges `ws:`/`wss:` by the loopback
set rather than by the scheme, because a socket to a remote host has left this
machine and a socket to loopback has not.

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

The audit is asked whether the artefact is real, not only whether it is named.
Two ways a published gallery used to pass while being useless:

- **No index.** `auditGallery` returned `ran: false` for a run that published
  renders but no `index.html`, on the reasonable ground that a narrow `--grep`
  can leave the index out. `ran: false` is what `specs/gallery.audit.spec.mjs`
  *skips* on — so deleting the staged index from a full matrix run turned five
  hundred unaudited figures into a **skipped check and a green run**. The
  absence of the index was excusing the audit from noticing the absence of the
  index. A missing index is a finding now, reported with the rest.
- **No bytes.** Completeness was arithmetic over a *file list*, and a file list
  is the one place a broken render still looks right. Truncating every published
  PNG to zero bytes left the gallery suites green at **54 of 54**. `auditBytes`
  reads both ends of every published render: the header says it is a PNG, and
  the `IEND` chunk says the writer reached the end of it — which is the half a
  size check alone would miss, and the likelier accident, because it is what a
  worker killed mid-write leaves behind.
- **Both ends and nothing between them.** Those two ends alone accepted the
  sixteen bytes `iVBORw0KGgpJRU5ErkJggg==` decodes to: a signature with the
  closing chunk stapled straight onto it. So the first chunk is read too. `IHDR`
  must be the first chunk of every PNG, its data length is fixed at 13 by the
  format, and it carries the dimensions — so a file too small to hold an image,
  one whose first chunk is not a header, and one describing a picture of no size
  are all caught.
- **The middle, which no end speaks for.** Reading 33 bytes at the front and 12
  at the back left everything between them unread, and that is where the picture
  is: renaming a render's only `IDAT` chunk to `JUNK` — same length, same size,
  both ends untouched — left every offline test green while Chromium refused the
  file outright with *"the source image could not be decoded."* A run could
  publish five hundred undecodable figures and certify the gallery complete. So
  the chunk stream is walked from the signature to the end of the file, every
  chunk checked against its own CRC, and at least one `IDAT` required — the one
  conjunct two perfect ends can never carry. The reader takes whole files, which
  reverses the note above about reading only their ends: 512 files at the time it
  was measured, about 43 MB,
  roughly half a second against the three and a half minutes that produced them.
- **Clauses the walk answered for.** Adding that walk quietly retired three of
  the checks above. A dimension, a chunk type and a declared length all live
  *inside* `IHDR`, so a fixture that broke one broke the chunk's checksum with
  it and the walk refused the file first — and deleting the header comparison,
  either dimension clause or the closing-chunk comparison left all 443 tests
  green. Each is pinned again by a row that **walks**: four flip one byte of the
  first chunk's type and repair every checksum after, one is assembled with a
  fourteen-byte first chunk and the real file's own `IDAT` and `IEND`, the
  dimension rows are resealed, and the closing row drops `IEND` entirely and
  still ends exactly where the file ends. A clause a passing suite cannot lose
  is the thing this audit exists to keep out, one layer in.
