// The authoritative UI state registry: dimensions and their values.
//
// This module is pure. It names every axis the Bureau Canvas can vary along,
// every equivalence-class value on that axis, and — for each value — the
// fixture it needs, the operations that reach it, and the controls and copy a
// reviewer should see once it is on screen. `registry.mjs` takes the Cartesian
// product of these values and applies the rules in `constraints.mjs`; nothing
// downstream invents a state that is not derivable from here.
//
// `n/a` is a real value, not a gap. A dimension that does not apply to a
// surface takes `n/a`, and a constraint rule says why — so an absent axis is
// recorded rather than silently dropped.

import { SELECTORS as S, editorCardFor, offered, relationCardFor, replayPositionAt, replaySpanFor, replaySpeed, replaySpeedActive, withheld } from "./selectors.mjs";
import { FIELD_SAVE, RUN_END, RUN_STEP, SAMPLE_STEPS, stepFor } from "./paths.mjs";

const NA = { id: "n/a", summary: "this axis does not exist on the chosen surface" };

/**
 * D1 — which top-level render surface is mounted. Boot covers the two states
 * that exist before any surface does: the loading placeholder and the
 * no-renderer fallback index.html installs.
 */
const surface = {
  id: "surface",
  title: "Surface",
  why: "the top-level render target; every other axis is scoped by it",
  values: [
    {
      id: "boot",
      summary: "index.html before a surface exists",
      page: "index",
    },
    {
      /*
       * editor.html boots on its own: it fetches its own `/state` and installs
       * its own fallback, so "the renderer never started" and "the payload
       * never arrived" are reachable there too. The fallback is the same
       * recovery shell index.html draws — same regions, same treatment — and
       * names this surface rather than the renderer as a whole. It also carries
       * the escape, which is the part that matters: editor.html is entered from
       * the landing and has no other navigation once the renderer is gone.
       */
      id: "boot-editor",
      summary: "editor.html before the editor exists",
      page: "editor",
      derive: (combo) => (combo.data === "render-error"
        ? { shows: [S.fallback, S.fallbackState, S.editorBack], copy: ["Pipeline editor could not start"], allowErrors: ["index.mjs", "Failed to load resource"] }
        : {}),
    },
    {
      id: "config",
      summary: "the assignment-first landing",
      page: "index",
      shows: [S.shell, S.header, S.configView, S.configHeading, S.relationSection],
      copy: ["Assignments"],
    },
    {
      id: "pipeline",
      summary: "the read-only pipeline viewer",
      page: "index",
      shows: [S.shell, S.pipelineView, S.pipelineToolbar, S.pipelineFlow, S.modeSwitcher],
      hides: [S.configView],
    },
    {
      id: "editor",
      summary: "editor.html — the pipeline editor and the relation graph",
      page: "editor",
      shows: [S.shell, S.editorTabs, S.editorTabPipeline, S.editorTabRelations],
      /*
       * The wrong render this page can actually produce. `.view-shell--config`
       * used to sit here, but that class is emitted only by `app.mjs`, which
       * `test/bundle.test.mjs` pins out of editor.html's module graph — so it
       * was a negative no state could ever fail. The reachable failure is
       * editor.html's own inline fallback, which replaces the root with a bare
       * `.status` line when the renderer throws.
       */
      hides: [S.loading],
    },
  ],
};

/**
 * D2 — where the config payload came from and whether the CLI accepted it.
 *
 * `only` scopes these expectations to the surfaces that draw a status line and
 * a findings strip. editor.html has its own header and reports findings
 * through the save result, so asserting the config status there would be
 * asserting something the surface never claimed.
 */
const data = {
  id: "data",
  title: "Data status",
  why: "the canvas never authors an error; it reports what validate said or says it has not checked",
  values: [
    { id: "loading", summary: "no state has arrived yet", shows: [S.loading], copy: ["Loading…"] },
    {
      id: "render-error",
      summary: "the renderer never mounted; the fallback reports the payload anyway",
      shows: [S.fallback, S.fallbackState],
      copy: ["Bureau renderer could not start"],
      // Blocking the module is the state; the failed request it causes is the
      // symptom being reviewed, not an unrelated defect.
      allowErrors: ["app.mjs", "Failed to load resource"],
      // editor.html installs a different fallback, which `boot-editor` derives.
      only: ["boot"],
    },
    { id: "fixture", summary: "no bureau binary; the bundled sample, said out loud", copy: ["bundled sample"] },
    { id: "validated", summary: "bureau validate ran and accepted it", copy: ["Validated"] },
    { id: "invalid", summary: "bureau validate rejected it; findings sit on what they name", copy: ["Validation findings"], shows: [".finding--validation"] },
    {
      id: "advisory",
      summary: "an advisory, which never blocks a save",
      // The class, not just the presence: an advisory painted in the
      // validation-error red would be a defect, and only this selector says so.
      shows: [".finding--advisory"],
      hides: [".finding--validation"],
    },
    {
      /*
       * `mergeAdvisories` concatenates advisories onto whatever validate
       * returned, so both classes really do arrive together — and adjacency is
       * exactly when the two treatments have to stay apart. Making these
       * alternatives would have hidden the pair rather than excluded it.
       */
      id: "invalid-advisory",
      summary: "validation errors and an advisory at once, which the CLI returns together",
      shows: [".finding--validation", ".finding--advisory"],
      copy: ["Validation findings"],
    },
  ],
};

/** D3 — the unsaved-work bar. Pending work has to look pending. */
const draft = {
  id: "draft",
  title: "Draft plan",
  why: "editing is proposing; unsaved work must read as unsaved and stay discardable",
  values: [
    { id: "none", summary: "nothing pending", hides: [S.draftBar] },
    {
      id: "pending",
      summary: "three unsaved changes",
      fixture: "draft-pending",
      shows: [S.draftBar, S.draftSave, S.draftDiscard, S.draftList],
      copy: ["3 unsaved changes"],
    },
    {
      id: "pending-one",
      summary: "one unsaved change, for the singular copy",
      fixture: "draft-single",
      shows: [S.draftBar],
      copy: ["1 unsaved change"],
    },
    /*
     * The two ends of a plan save. Both are ordinary renders of `DraftBar`
     * (`app.mjs`), reached by pressing Save with `./intent` routed in the
     * browser — so the plan is never applied and the host's `.bureau/` is
     * untouched. They were excluded outright while that route already existed,
     * which is why the refusal shipped asserted by nothing at all.
     */
    {
      id: "saving",
      summary: "save-plan in flight, so neither button takes a second click",
      shows: [S.draftBar, withheld(S.draftSave), withheld(S.draftDiscard)],
      copy: ["Saving…"],
    },
    /*
     * Discarding is not saving, and used to be drawn as if it were.
     *
     * `act()` shared one `busy` flag, and only the Save button read it — so
     * pressing Discard put "Working…" on *Save* and left Discard reading
     * "Discard". The registry called that one equivalence class with `saving`,
     * which was true of the pixels and was exactly the problem: a bar that says
     * a save is in flight while a discard is in flight is the one sentence a
     * draft surface may not get wrong. Now each button carries its own verb,
     * the two are genuinely two screens, and this is the value that says so.
     */
    {
      id: "discarding",
      summary: "discard-plan in flight, named on the button that was pressed",
      shows: [S.draftBar, withheld(S.draftSave), withheld(S.draftDiscard)],
      copy: ["Discarding…"],
      hides: [],
    },
    {
      id: "save-error",
      summary: "the plan came back refused and is still there to retry",
      shows: [S.draftBar, S.draftRefused, offered(S.draftSave), offered(S.draftDiscard)],
      copy: ["could not save changes"],
    },
    /*
     * Discard is the other half of the bar and it fails on its own sentence.
     * The fallback names the verb, so a bar that reported a failed discard as a
     * failed save would read as work still pending when it is the discard that
     * did not happen.
     *
     * It was the last write family on these surfaces with neither a value nor a
     * rule, which is the one thing this registry may not do.
     */
    {
      id: "discard-error",
      summary: "the discard came back refused, and says so in its own words",
      shows: [S.draftBar, S.draftRefused, offered(S.draftSave), offered(S.draftDiscard)],
      copy: ["could not discard changes"],
    },
  ],
};

/**
 * D4 — how much config the landing has to show. These are alternatives: one
 * config is published at a time, so a stack cannot also be empty.
 */
const section = {
  id: "section",
  title: "Config content",
  why: "the landing's shape follows the config it was given, from nothing configured to a full stack",
  values: [
    { id: "stack", summary: "the assignment stack at rest", shows: [S.assignmentStack, S.assignmentCard] },
    {
      id: "empty",
      summary: "no assignments configured yet",
      fixture: "empty",
      shows: [S.assignmentEmpty],
      hides: [S.assignmentCard],
      copy: ["No assignments yet."],
    },
    {
      id: "two-cards",
      summary: "more than one assignment, so the stack is a stack",
      fixture: "two-assignments",
      // The second card by name and by selector. `assignmentStack` alone is
      // satisfied by a stack of one, so this value asserted nothing the
      // ordinary `stack` value did not already claim.
      shows: [S.assignmentStack, S.assignmentCardSecond],
      copy: ["docs-triage"],
    },
  ],
};

/** The two items `fixtures.mjs` leaves unreferenced, as relation node ids. */
const ORPHAN_ITEMS = ["role:retired-reviewer", "repo:retired-sandbox"];

/**
 * D5 — whether anything in the config is unreferenced.
 *
 * `OrphanStrip` is a *sibling* of the assignment stack inside `ConfigView`,
 * not an alternative to it, so it has to be its own axis. Folded into the
 * content axis it made "nothing configured yet, and therefore everything
 * unreferenced" — the ordinary first-run landing, where the strip is at its
 * fullest — unrepresentable rather than excluded, which is the one thing the
 * registry may not do. The same reasoning already split the disclosures out.
 */
const orphans = {
  id: "orphans",
  title: "Unreferenced config",
  why: "the orphan strip is a sibling of the stack, so leftovers can accompany any amount of config",
  values: [
    { id: "none", summary: "everything is referenced", hides: [S.orphanStrip] },
    {
      id: "present",
      summary: "a role and a repo nothing references",
      fixture: "orphans",
      shows: [S.orphanStrip],
      // Naming the items, not just the heading: a strip that rendered its
      // frame and dropped its entries satisfied "Unreferenced" alone.
      copy: ["Unreferenced", "retired-reviewer", "retired-sandbox"],
      /*
       * An orphan is config the graph still draws — as a card with no edges.
       * Asserted only where the graph is on screen, so the claim sits on the
       * states that can actually fail it rather than on every landing.
       */
      derive: (combo) => (["relation-open", "both"].includes(combo.disclosure)
        ? { shows: ORPHAN_ITEMS.map(relationCardFor) }
        : {}),
    },
  ],
};

/**
 * D5 — which secondary landing region is disclosed.
 *
 * `ConfigView` renders the create bar, the stack, the orphan strip and the
 * relation disclosure as siblings that are always all present, so a disclosure
 * is open *over* whatever content the config happens to have — including an
 * empty one, which is exactly how the first assignment gets made. Folding these
 * into the content axis would have made "nothing configured yet, create form
 * open" unrepresentable rather than excluded.
 */
const disclosure = {
  id: "disclosure",
  title: "Landing disclosure",
  why: "the create bar and the relation graph are siblings of the stack, not alternatives to it",
  values: [
    {
      id: "none",
      summary: "every secondary region closed",
      // Collapsed is a state the disclosure itself records. The create form is
      // absent from the DOM until it is opened; the relation graph is not — a
      // closed `<details>` keeps its subtree mounted and still reports client
      // rects for it, so `.relation-flow` cannot tell open from closed. Its
      // own `open` can, and saying nothing at all here let a section that
      // shipped `open` pass in every one of these states.
      hides: [S.createBar, S.relationOpen],
    },
    {
      id: "create",
      summary: "the create form open",
      enter: [{ op: "click", selector: S.createOpen }],
      // The form opens with nothing typed, so its save is withheld — the same
      // draft safety every field editor keeps, on the one control that makes a
      // new item. Without it the form could ship a submit that was always live.
      shows: [S.createBar, S.createKind, S.createName, withheld(S.createSubmit), S.createCancel],
      hides: [S.relationOpen],
      copy: ["New reusable config"],
    },
    {
      // Typing the name is what arms the submit, and it is the only difference
      // between the two create screens — so it is a value rather than a step
      // folded into `create`, which would have made the withheld assertion and
      // the offered one contradict each other on one state.
      id: "create-named",
      summary: "the create form with a name typed, so the save is offered",
      enter: [{ op: "click", selector: S.createOpen }, { op: "fill", selector: S.createName, value: "second-reviewer" }],
      shows: [S.createBar, S.createKind, offered(S.createSubmit), S.createCancel],
      hides: [S.relationOpen],
      copy: ["New reusable config"],
    },
    {
      id: "relation-open",
      summary: "the shared relation graph expanded",
      enter: [{ op: "click", selector: S.relationSummary }],
      shows: [S.relationSection, S.relationOpen, S.relationFlow],
    },
    // The create bar and the relation `<details>` are siblings with separate
    // local state, so opening one does not close the other. Without this value
    // the pair would be excluded by the shape of the axis rather than by a
    // named rule — the one thing the registry may not do.
    {
      id: "both",
      summary: "the create form and the relation graph open at once",
      enter: [
        { op: "click", selector: S.createOpen },
        { op: "click", selector: S.relationSummary },
      ],
      shows: [S.createBar, S.createSubmit, S.relationSection, S.relationOpen, S.relationFlow],
      copy: ["New reusable config"],
    },
    /*
     * A create the host refused. The form stays open with the name still in it,
     * which is the whole point — a refusal that closed the form would lose the
     * work. `./intent` is routed in the browser, so nothing is written.
     */
    {
      id: "create-error",
      summary: "the create came back refused, with the form still there to retry",
      enter: [
        { op: "click", selector: S.createOpen },
        { op: "fill", selector: S.createName, value: "second-reviewer" },
        { op: "click", selector: S.createSubmit },
        { op: "wait", selector: S.createRefused },
      ],
      shows: [S.createBar, S.createRefused, offered(S.createSubmit)],
      hides: [S.relationOpen],
      copy: ["could not create pipeline"],
    },
  ],
};

/** D6 — whether the assignment card is open. */
const card = {
  id: "card",
  title: "Assignment card",
  why: "the assignment-first mental model: everything else is reached through a card",
  values: [
    { id: "collapsed", summary: "glance line only", shows: [S.assignmentCard], hides: [S.assignmentDetail] },
    {
      id: "expanded",
      summary: "work source, work rules, signals, repos, pipeline and limits",
      enter: [{ op: "click", selector: S.assignmentHead }],
      shows: [S.assignmentDetail, S.workSourceValue, S.workRulesValue, S.signalsValue, S.reposValue, S.limitsValue],
      copy: ["work source", "work rules", "forge signals", "repos", "pipeline", "limits"],
    },
  ],
};

/**
 * D7 — which field disclosure is open inside an expanded card. The shared
 * control is the point: every field rests as a button and opens in place.
 */
const field = {
  id: "field",
  title: "Field editor",
  why: "one shared disclosure control per field; opening one must not disturb the others",
  values: [
    { id: "none", summary: "every field at rest", hides: [S.workSourceEditor, S.reposEditor, S.limitsEditor, S.workRulesEditor, S.signalsEditor] },
    {
      id: "work-source",
      summary: "paste a board or issues URL",
      enter: [{ op: "click", selector: S.workSourceValue }],
      shows: [S.workSourceEditor, S.workSourceUrl],
      copy: ["link a work source"],
    },
    {
      id: "work-rules",
      summary: "filter, approval label and branch prefix",
      enter: [{ op: "click", selector: S.workRulesValue }],
      shows: [S.workRulesEditor, S.workRulesSave],
      copy: ["Work-item filter", "Branch prefix"],
    },
    {
      id: "forge-signals",
      summary: "the labels Bureau applies at terminal states",
      enter: [{ op: "click", selector: S.signalsValue }],
      shows: [S.signalsEditor, S.signalsSave],
      copy: ["Failed run label", "Needs-human label", "Bureau preserves unrelated work-item labels."],
    },
    {
      id: "repos",
      summary: "the ranked repo list",
      enter: [{ op: "click", selector: S.reposValue }],
      shows: [S.reposEditor, S.reposSave, S.reposAdd],
    },
    {
      id: "repos-add",
      summary: "registering a repo from its URL",
      enter: [{ op: "click", selector: S.reposValue }, { op: "click", selector: S.reposAdd }],
      shows: [S.reposEditor, S.reposUrl],
      copy: ["add a repo"],
    },
    {
      id: "limits",
      summary: "every limit, on or off",
      enter: [{ op: "click", selector: S.limitsValue }],
      shows: [S.limitsEditor, S.limitsSave, S.limitRow],
      copy: ["Off means no ceiling at all"],
    },
    {
      id: "delete",
      summary: "delete asks first and shows what breaks",
      enter: [{ op: "click", selector: S.deleteStart }, { op: "wait", selector: S.preflight }],
      // The point of a preflight is the answer it gives, and the answer is the
      // state of the confirm button. Asserting only that a preflight appeared
      // left the one control it exists to gate unasserted.
      shows: [S.preflight, offered(S.deleteConfirm)],
      copy: ["Nothing references this"],
      /*
       * The preflight is a real intent, and `runCrudIntent` answers even a
       * read-only one by refreshing and republishing the host's own state —
       * which replaces the injected payload, status line and all. So the
       * status axis has nothing left to assert here.
       *
       * Only the status. What the republished config *contains* is covered by
       * an exclusion instead: suppressing `section` too would have let the
       * host's one-card screen pass under the name `two-cards`, which is a
       * harness artifact recorded as a state.
       */
      suppress: ["data"],
    },
    /*
     * The refusal, kept as a value so it is excluded by a named rule rather
     * than missing — `delete-is-offered-only-where-nothing-refers` says why.
     * It is a real screen of `DeleteControl`, and one no config the canvas can
     * reach draws: the two places the control mounts are an assignment card
     * and the orphan strip, and an orphan is by definition the config nothing
     * references. `test/preflight.test.mjs` owns the blocking answer itself.
     */
    {
      id: "delete-blocked",
      summary: "a preflight that found referrers, so the confirm is withheld",
      shows: [S.preflight, withheld(S.deleteConfirm)],
      copy: ["Repoint these references before deleting this item."],
      suppress: ["data"],
    },
  ],
};

/**
 * D8 — the lifecycle of whichever field editor is open.
 *
 * Each value asserts the save button's own state, which is what draft safety
 * means at the field level and the only thing that tells the three apart.
 * Without it a `dirty` that rendered a clean, save-disabled form — because it
 * typed the values the payload already held — passed as dirty.
 */
const fieldState = {
  id: "fieldState",
  title: "Field lifecycle",
  why: "draft safety: save stays disabled until the value both changed and is valid",
  values: [
    { id: "rest", summary: "opened, nothing typed", derive: (combo) => save(combo, withheld) },
    { id: "dirty", summary: "changed and valid — save is offered", derive: (combo) => save(combo, offered) },
    { id: "invalid", summary: "changed into something the field refuses", derive: (combo) => save(combo, withheld) },
    /*
     * The two ends of a save, reached by routing `./intent` in the browser
     * rather than by letting the click reach the shared host — see
     * `SAVE_INTERCEPTS` in `paths.mjs`. They were excluded values until that
     * existed, on the grounds that the harness could not press the button; but
     * "cannot be performed here" was never the same claim as "cannot be
     * rendered", and a save in flight and a save refused are two of the most
     * ordinary screens this UI has.
     *
     * A refusal reaches the page as `{ ok: false }` in an HTTP 200 body —
     * `extension.mjs` answers every intent through `sendJson`, which writes 200
     * unconditionally — so nothing about being refused is logged to the
     * console. A console line on one of these states is a defect in it.
     */
    { id: "saving", summary: "the save is in flight; the button says so and refuses a second click", derive: (combo) => save(combo, withheld), copy: ["Saving…"] },
    {
      id: "save-error",
      summary: "the save came back refused and the draft is still there to retry",
      derive: (combo) => save(combo, offered),
    },
  ],
};

/** The open field's save button, in the state this lifecycle value claims. */
function save(combo, shape) {
  const button = FIELD_SAVE[combo.field];
  return button ? { shows: [shape(button)] } : {};
}

/**
 * D8b — whether a second field disclosure is open beside the one under review.
 *
 * Every field keeps its own open state, so opening one does not close another:
 * the pair is a real screen, not a hypothetical. `field` is single-valued, and
 * on its own that made two-open *unrepresentable* rather than excluded — the
 * one thing the registry may not do, and the same mistake that once folded the
 * landing's disclosures onto its content axis. This axis gives the pair a tuple
 * to be, so a rule can exclude it and a probe can answer for it.
 */
/** Every field editor, and the one each field value opens. */
const EDITORS = [S.workSourceEditor, S.workRulesEditor, S.signalsEditor, S.reposEditor, S.limitsEditor];
const EDITOR_OF = {
  "work-source": S.workSourceEditor,
  "work-rules": S.workRulesEditor,
  "forge-signals": S.signalsEditor,
  repos: S.reposEditor,
  "repos-add": S.reposEditor,
  limits: S.limitsEditor,
};

const fieldPair = {
  id: "fieldPair",
  title: "Second field editor",
  why: "field disclosures are siblings, so a second one open at once is a state rather than a gap",
  values: [
    {
      id: "none",
      summary: "only the field under review is open",
      // Every *other* editor absent, not merely the one `second-open` adds.
      // Scoped to the field under review, so this holds whichever it is, and
      // any disclosure that opened without being asked to fails it.
      derive: (combo) => ({ hides: EDITORS.filter((editor) => editor !== EDITOR_OF[combo.field]) }),
    },
    {
      id: "second-open",
      summary: "the limits disclosure open alongside it",
      shows: [S.limitsEditor],
    },
  ],
};

/** Fields that are editors, so a second one can be open beside them. */
export const PAIRABLE_FIELDS = ["work-source", "work-rules", "forge-signals", "repos", "repos-add", "limits"];

/**
 * The edge classes the two graphs draw, which are the whole of what an edge
 * means on screen. Both graphs key a control edge by its outcome and a
 * relation edge by its relation (`lib/view.mjs` emits `data` and `observes`),
 * so these are what the sample pipeline wires before anything is added to it.
 * `observes` is not here: it needs a step with an `over`, which the sample has
 * none of — `pick: decision` creates one and asserts it there.
 */
const VIEWER_EDGES = [".flow-edge--success", ".flow-edge--failure", ".flow-edge--blocked", ".flow-edge--no-work", ".flow-edge--data"];
const EDITOR_EDGES = [".editor-edge--success", ".editor-edge--failure", ".editor-edge--blocked", ".editor-edge--no-work", ".editor-edge--data"];

/**
 * D9 — the pipeline viewer's three graph modes.
 *
 * Design asserts the drawing's own semantics, not just that a graph appeared:
 * a control edge per outcome, the two relation edges the sample pipeline wires,
 * and a terminal pill for the terminal it ends on. Naming only `.pipeline-flow`
 * let a graph that drew every edge in one class — or dropped its terminals —
 * pass as the static config graph.
 */
const mode = {
  id: "mode",
  title: "Graph mode",
  why: "design is the config graph; live and replay restyle it from the run log, never from config edges",
  values: [
    {
      id: "design",
      summary: "the static config graph",
      shows: [S.modeSwitcher, S.legend, S.terminalPill, ...VIEWER_EDGES],
      // The overlay controls are named absent, not merely unmentioned. Design
      // was pinned in one direction only, so the way *back* from Live and
      // Replay — both of which return here — asserted that the design graph had
      // arrived and nothing about the run controls having gone. A `waitGone` is
      // the only thing that noticed, and a `waitGone` on a selector that never
      // matches passes instantly.
      hides: [S.runControls, S.replayControls],
    },
    {
      id: "live",
      summary: "one live run, streamed",
      enter: [{ op: "click", selector: S.modeLive }],
      shows: [S.runControls, S.runPickerLive],
    },
    {
      id: "replay",
      summary: "any run, scrubbed on a timeline",
      enter: [{ op: "click", selector: S.modeReplay }],
      shows: [S.replayControls, S.runPickerReplay],
    },
  ],
};

/**
 * D10 — which run the overlay is showing, and how far through it is.
 *
 * Each value asserts something only the *selected* run's log can produce: the
 * step decoration live folds out of it, and the span replay's timeline takes
 * from it. Naming the run alone asserted nothing — three runs rendered under
 * three ids with one set of expectations between them, so an overlay that
 * ignored the selection entirely would have passed.
 */
const run = {
  id: "run",
  title: "Run selection",
  why: "the dry run reports; it never predicts — every overlay state comes from the log",
  values: [
    /*
     * Not merely "no decoration": a replay that auto-selected a run would
     * still draw neither overlay class, and this state would have passed. The
     * negative is taken per mode from the thing a selection is what produces —
     * the timeline in replay, the pause control in live.
     */
    {
      id: "none",
      summary: "no run picked",
      hides: [S.overlayRunning, S.overlayPaused],
      derive: (combo) => (combo.mode === "replay"
        ? { hides: [S.replayTimeline] }
        : { hides: [S.runPause, S.runResume] }),
    },
    { id: "running", summary: "a run still appending events", derive: (combo) => overlay(combo, "running") },
    { id: "paused", summary: "a run paused at a step", derive: (combo) => overlay(combo, "paused") },
    { id: "finished", summary: "a run that reached a terminal", derive: (combo) => overlay(combo, "finished") },
    /*
     * The screen a reader is left on when the run they picked ends under them.
     *
     * Not the same state as `finished`, which is a finished run *chosen from
     * the replay listing* — there the chrome is a timeline and nothing was
     * ever offered to act on. Here the transport was offered a moment ago and
     * has been withdrawn, which is the whole subject: `runActions` takes Pause,
     * Resume and Cancel once nothing can act on the run, and the status is what
     * says why.
     *
     * The picker's own label still reads `live` here, and that is the state
     * rather than a defect: the listing is polled every four seconds, so for
     * that long it names the run as it was when it was picked while the status
     * beside it reports where the run actually got to. What happens after the
     * poll is that the run leaves the live listing — and `runsOffered` keeps
     * the watched run in it, because a `<select>` whose value matches no option
     * draws blank beside an overlay that is still up. That half is a fact about
     * a listing rather than a screen a click reaches, so `test/overlay.test.mjs`
     * holds it directly instead of the matrix waiting out a timer for it.
     */
    {
      id: "ended",
      summary: "a run picked while live that has since reached its terminal",
      shows: [S.runStatusFinished, S.runPickerLive],
      hides: [S.runPause, S.runResume, S.runCancel, S.overlayRunning, S.overlayPaused],
      copy: ["finished"],
    },
    /*
     * A run control the host refused. Cancel is sent against a live running run
     * with `./intent` routed in the browser, so no real run is acted on. The
     * status is unchanged by a refusal, so the transport stays offered — the
     * reader has to be able to try again, and a refusal that withdrew the
     * controls would strand them.
     */
    {
      id: "refused",
      summary: "a cancel the host refused, with the transport still offered",
      shows: [S.runControlError, S.overlayRunning, S.runPause, S.runCancel, S.runStatus],
      copy: ["intent failed"],
    },
  ],
};

/**
 * Live decorates the graph from the log it just folded; replay spans the log
 * on its timeline. Replay rests at the start of the run, so every replayed run
 * decorates identically — the span is what distinguishes them, and it comes
 * straight from the last event in the chosen log.
 *
 * Live also asserts the transport itself. Cancel is offered for as long as the
 * run can be acted on and withdrawn once it cannot, and `.run-status` is the
 * element that reports which of those it is — both were drawn by every live
 * state and asserted by none, so deleting Cancel changed no verdict.
 */
function overlay(combo, runValue) {
  if (combo.mode === "replay") {
    return { shows: [S.replayTimeline, replaySpanFor(RUN_END[runValue])] };
  }
  return runValue === "paused"
    ? { shows: [S.overlayPaused, S.pausedBadge, S.runResume, S.runCancel, S.runStatus], hides: [S.runPause], copy: ["paused"] }
    : { shows: [S.overlayRunning, S.runPause, S.runCancel, S.runStatus], hides: [S.runResume], copy: ["running"] };
}

/**
 * D10b — where the replay transport has moved the run.
 *
 * The timeline is not decoration: step, play and the speed buttons are the
 * whole of what replay does, and every one of them was unasserted — a replay
 * that never advanced passed every state. `rest` is the position a freshly
 * picked run parks at, so the two acting values are what tell a working
 * transport from a drawn one.
 *
 * Play is deliberately a declared-and-excluded value rather than a silence:
 * it advances on a 100ms interval, so a state holding it would assert a
 * position that depends on when the screenshot was taken.
 */
const transport = {
  id: "transport",
  title: "Replay transport",
  why: "replay's controls have to move the run; a timeline that only draws is the defect they hide — and Play asserts its label rather than its position, because a timer decides the second and not the first",
  values: [
    {
      id: "rest",
      summary: "parked at the first event, running at 1x",
      derive: (combo) => ({ shows: [replayPositionAt(RUN_STEP[combo.run]?.start), replaySpeedActive(1), S.replayPlay, S.replayStepForward, S.replayStepBack], copy: ["+0.0s"] }),
    },
    {
      id: "stepped",
      summary: "stepped forward to the next event in the log",
      enter: [{ op: "click", selector: S.replayStepForward }],
      derive: (combo) => ({ shows: [replayPositionAt(RUN_STEP[combo.run]?.next)], copy: [RUN_STEP[combo.run]?.readout] }),
    },
    {
      id: "speed-16x",
      summary: "the 16x speed taken, with 1x given up",
      enter: [{ op: "click", selector: replaySpeed(16) }],
      shows: [replaySpeedActive(16)],
      hides: [replaySpeedActive(1)],
    },
    /*
     * Playing. The position it reaches depends on the clock, so this state
     * asserts the one thing that does not: the button's own label and aria
     * name flip to Pause, and the rest of the transport stays offered. The
     * matrix gallery is a browsable render rather than a pinned baseline —
     * only the ten `@visual` screens are compared — so a scrubber that has
     * moved on by a frame costs nothing, while an unasserted Play button was
     * a control the timeline shipped and no state exercised.
     */
    {
      id: "playing",
      summary: "playing, so the button offers Pause and the position is the clock's",
      enter: [{ op: "click", selector: S.replayPlay }, { op: "wait", selector: S.replayPause }],
      shows: [S.replayPause, S.replayStepForward, S.replayStepBack],
      copy: ["Pause"],
    },
  ],
};

/** D11 — the editor's two tabs. */
const tab = {
  id: "tab",
  title: "Editor tab",
  why: "one shared relation renderer, one tab away from the pipeline it explains",
  values: [
    {
      id: "pipeline",
      summary: "the step graph editor",
      // The drawing's own semantics, not merely that a drawing appeared: an
      // edge class per outcome and per relation, and the terminals the steps
      // route into. Both were named by the design system and asserted nowhere,
      // so an editor that drew every edge alike passed all 21 of these states.
      shows: [S.editorToolbar, S.editorPanel, S.editorTerminal, ...EDITOR_EDGES],
      /*
       * `EditorApp` keeps both panes mounted and separates them with `hidden`
       * alone. The leak was pinned in one direction only — the two Relations
       * probes assert the editor panel is gone — so dropping `hidden` from the
       * relation pane drew the graph under the editor in all 21 pipeline-tab
       * states with nothing failing. This is the other direction.
       */
      hides: [S.relationFlow],
    },
    {
      id: "relations",
      summary: "the shared read-only relation graph",
      enter: [{ op: "click", selector: S.editorTabRelations }],
      shows: [S.relationFlow],
    },
  ],
};

/** D12 — which step kind the editor has selected. */
const pick = {
  id: "pick",
  title: "Step selection",
  why: "each step kind owns a different field set; no selection has to say what to do",
  values: [
    { id: "none", summary: "nothing selected", shows: [S.editorEmpty], copy: ["Edit a step"] },
    { id: "deterministic", summary: "a shell step", copy: ["run"] },
    { id: "agent", summary: "a delegated step", shows: [S.stepRole, S.stepTrust] },
    // A decision step is the one the sample has to grow: it carries an `over`,
    // which is the only thing that draws an `observes` edge. Asserting it here
    // is what holds the relation edge the viewer's own graph never wires.
    { id: "decision", summary: "a four-way branch", shows: [".editor-edge--observes"], copy: ["on (all four outcomes required)"] },
    { id: "concurrent", summary: "a member group", copy: ["completion", "maximum concurrent members"] },
  ],
};

/**
 * D13 — what the editor has done to the draft, and what save did about it.
 *
 * Each value asserts the Save button's own state, which is the only thing that
 * tells several of these apart: `created` and `invalid` both report issues and
 * both offer Discard, so without it two states shared one set of assertions.
 *
 * Save is withheld for a draft the editor cannot even render — a non-numeric
 * attempt count — and offered otherwise. It is deliberately *not* withheld for
 * an inline hint: `lib/edit.mjs` calls those "hints, not verdicts", and
 * `bureau validate` is the authority. `save-pipeline` writes, re-validates and
 * reverts on a finding, so an unwired new step is saveable and then refused by
 * the CLI — which is the rule that the canvas never authors an error.
 */
const edit = {
  id: "edit",
  title: "Editor mutation",
  why: "the editor never leaves an unloadable config: dirty, invalid and reverted are distinct and truthful",
  values: [
    { id: "rest", summary: "no edits", copy: ["saved"], shows: [withheld(S.editorSave)], hides: [S.editorDiscard] },
    // A new step is unreachable until it is wired, so the toolbar truthfully
    // reports an issue rather than a bare "unsaved edits". Dirtiness is
    // asserted through the Discard button, which only exists while dirty.
    { id: "created", summary: "a step added but not saved", shows: [S.editorDiscard, S.editorIssues, offered(S.editorSave)] },
    { id: "renamed", summary: "a step renamed, cascading to its referrers", shows: [S.editorDiscard, offered(S.editorSave)] },
    {
      id: "delete-confirm",
      summary: "the delete confirmation open",
      /*
       * The confirmation itself looks the same either way; the draft behind it
       * does not. A decision or concurrent step has to be *added* before it can
       * be deleted, so the editor is dirty and Save is offered; a step the
       * fixture already draws leaves it clean and Save withheld. Without this
       * the two screens shared one assertion — the danger zone — and a
       * confirmation that silently dirtied the draft would have passed.
       */
      derive: (combo) => (SAMPLE_STEPS[combo.pick]
        ? { shows: [withheld(S.editorSave)], hides: [S.editorDiscard] }
        : { shows: [S.editorDiscard, offered(S.editorSave)] }),
      // The confirm button itself, not only the zone that holds it: a danger
      // zone that asked and offered no way to answer would have passed.
      shows: [S.editorDangerZone, S.editorDeleteConfirm],
    },
    {
      id: "layout-moved",
      summary: "only a node position changed",
      shows: [S.editorDiscard, offered(S.editorSave)],
      // A move acts on a step the fixture already draws, so the graph stays
      // valid and the status reads "unsaved edits" — which is what tells this
      // apart from `created`, whose new step is unwired and reports an issue.
      // Reaching a decision or concurrent selection means adding one first, so
      // "moved" there would really be "created and then moved"; a rule keeps
      // those kinds out rather than letting one state stand for two.
      copy: ["unsaved edits"],
      hides: [S.editorIssues],
    },
    {
      id: "deleted",
      summary: "the delete confirmed: the step and its edges are gone and nothing is selected",
      /*
       * The screen the confirmation leads to, which nothing rendered.
       *
       * `delete-confirm` stopped at the question. Answering it is one click,
       * `onDelete` drops the step and its edges and calls `setSelected(null)`,
       * and the result is a different screen in three ways at once: the card is
       * gone from the graph, the panel falls back to its empty prompt, and the
       * draft is dirty whichever step was chosen — including the fixture steps,
       * whose confirmation left it clean. No rule excluded it and no value
       * described it, which is the one thing this registry may not do.
       */
      derive: (combo) => ({ hides: [editorCardFor(stepFor(combo.pick))] }),
      shows: [S.editorEmpty, S.editorDiscard, offered(S.editorSave)],
      copy: ["Select a step to edit its fields and outcomes."],
      /*
       * `pick` describes the *selected* step's editor — the run field of a
       * deterministic step, the outcome map of a decision. Deleting the step
       * clears the selection, so none of that is on screen any more and the
       * axis has nothing left to assert. The axis still carries which step was
       * chosen, which is what the `hides` above is derived from, so the choice
       * is recorded rather than dropped.
       */
      suppress: ["pick"],
    },
    // The one edit Save itself refuses: an attempt count the editor cannot
    // render. That refusal is the whole difference from `created`.
    { id: "invalid", summary: "an edit the editor refuses to save", shows: [S.editorIssues, S.editorDiscard, withheld(S.editorSave)] },
    /*
     * The two ends of a pipeline save, rendered with `./intent` routed in the
     * browser so nothing is written. `saving` is the state that was hiding a
     * defect: the registry claimed the button was withheld while it was in
     * flight, the rule excluding it meant nobody ever rendered the claim, and
     * `EditorToolbar` in fact left Save live for the whole round trip — a
     * second click racing a second write against the first one's revert.
     */
    {
      id: "saving",
      summary: "save-pipeline in flight, so neither button takes a second click",
      shows: [withheld(S.editorSave), withheld(S.editorDiscard)],
      copy: ["Saving…"],
    },
    {
      id: "save-error",
      summary: "the write was refused, and the draft is still there to retry",
      shows: [S.editorSaveReverted, S.editorStatusError, S.editorDiscard, offered(S.editorSave)],
      // The findings are the point of this state: a refused `save-pipeline`
      // always carries the reasons the write was reverted, so a panel that
      // announced the refusal without printing them would be the whole defect.
      copy: ["save reverted", "no step in this pipeline is called `implement`"],
    },
  ],
};

export const DIMENSIONS = [surface, data, draft, section, orphans, disclosure, card, field, fieldState, fieldPair, mode, run, transport, tab, pick, edit];

export const DIMENSION_BY_ID = Object.fromEntries(DIMENSIONS.map((item) => [item.id, item]));

/** Every dimension except `surface` can be absent; `n/a` says so. */
export const OPTIONAL = ["data", "draft", "section", "orphans", "disclosure", "card", "field", "fieldState", "fieldPair", "mode", "run", "transport", "tab", "pick", "edit"];

for (const id of OPTIONAL) {
  DIMENSION_BY_ID[id].values = [NA, ...DIMENSION_BY_ID[id].values];
}

export function valuesOf(dimensionId) {
  return DIMENSION_BY_ID[dimensionId].values;
}

export function valueOf(dimensionId, valueId) {
  return valuesOf(dimensionId).find((value) => value.id === valueId);
}
