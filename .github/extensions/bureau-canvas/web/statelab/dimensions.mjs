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

import { SELECTORS as S, offered, relationCardFor, replaySpanFor, withheld } from "./selectors.mjs";
import { FIELD_SAVE, RUN_END } from "./paths.mjs";

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
       * never arrived" are reachable there too, and look nothing like the
       * index fallback — a `.status` line in an ordinary shell rather than the
       * dedicated fallback shell.
       */
      id: "boot-editor",
      summary: "editor.html before the editor exists",
      page: "editor",
      derive: (combo) => (combo.data === "render-error"
        ? { shows: [S.loading], copy: ["Editor could not start"], allowErrors: ["index.mjs", "Failed to load resource"] }
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
      hides: [S.configView],
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
const ORPHAN_ITEMS = ["role:retired-reviewer", "pipeline:retired-pipeline"];

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
      summary: "a role and a pipeline nothing references",
      fixture: "orphans",
      shows: [S.orphanStrip],
      // Naming the items, not just the heading: a strip that rendered its
      // frame and dropped its entries satisfied "Unreferenced" alone.
      copy: ["Unreferenced", "retired-reviewer", "retired-pipeline"],
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
      // The create form is absent from the DOM until it is opened. The relation
      // graph is not: it lives inside a closed `<details>`, which keeps its
      // subtree mounted, so "closed" is not something a selector count can say.
      hides: [S.createBar],
    },
    {
      id: "create",
      summary: "the create form open",
      enter: [{ op: "click", selector: S.createOpen }],
      shows: [S.createBar, S.createKind, S.createName, S.createSubmit, S.createCancel],
      copy: ["New reusable config"],
    },
    {
      id: "relation-open",
      summary: "the shared relation graph expanded",
      enter: [{ op: "click", selector: S.relationSummary }],
      shows: [S.relationSection, S.relationFlow],
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
      shows: [S.createBar, S.createSubmit, S.relationSection, S.relationFlow],
      copy: ["New reusable config"],
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
      shows: [S.preflight],
      /*
       * The preflight is a real intent, so the host answers it by republishing
       * its own state over SSE — which replaces the injected payload outright,
       * not merely the status line it wrote. So every axis whose claim comes
       * from that payload is suppressed here: `data` for the status and
       * findings, `section` for the assignments the fixture added. Asserting
       * either would assert a payload the page no longer holds.
       *
       * `section` joined this list only once `two-cards` began asserting its
       * second card; while that value claimed nothing the stack alone gave,
       * the loss was real and invisible.
       */
      suppress: ["data", "section"],
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
const fieldPair = {
  id: "fieldPair",
  title: "Second field editor",
  why: "field disclosures are siblings, so a second one open at once is a state rather than a gap",
  values: [
    {
      id: "none",
      summary: "only the field under review is open",
      // The limits disclosure is what `second-open` adds, so its absence is
      // what "only one" means — except where limits is itself the field under
      // review, and its editor is the first rather than a second.
      derive: (combo) => (combo.field === "limits" ? {} : { hides: [S.limitsEditor] }),
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

/** D9 — the pipeline viewer's three graph modes. */
const mode = {
  id: "mode",
  title: "Graph mode",
  why: "design is the config graph; live and replay restyle it from the run log, never from config edges",
  values: [
    { id: "design", summary: "the static config graph", shows: [S.modeSwitcher, S.legend] },
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
  ],
};

/**
 * Live decorates the graph from the log it just folded; replay spans the log
 * on its timeline. Replay rests at the start of the run, so every replayed run
 * decorates identically — the span is what distinguishes them, and it comes
 * straight from the last event in the chosen log.
 */
function overlay(combo, runValue) {
  if (combo.mode === "replay") {
    return { shows: [S.replayTimeline, replaySpanFor(RUN_END[runValue])] };
  }
  return runValue === "paused"
    ? { shows: [S.overlayPaused, S.pausedBadge, S.runResume], hides: [S.runPause], copy: ["paused"] }
    : { shows: [S.overlayRunning, S.runPause], hides: [S.runResume], copy: ["running"] };
}

/** D11 — the editor's two tabs. */
const tab = {
  id: "tab",
  title: "Editor tab",
  why: "one shared relation renderer, one tab away from the pipeline it explains",
  values: [
    { id: "pipeline", summary: "the step graph editor", shows: [S.editorToolbar, S.editorPanel] },
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
    { id: "decision", summary: "a four-way branch", copy: ["on (all four outcomes required)"] },
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
    { id: "delete-confirm", summary: "the delete confirmation open", shows: [S.editorDangerZone] },
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
    // The one edit Save itself refuses: an attempt count the editor cannot
    // render. That refusal is the whole difference from `created`.
    { id: "invalid", summary: "an edit the editor refuses to save", shows: [S.editorIssues, S.editorDiscard, withheld(S.editorSave)] },
  ],
};

export const DIMENSIONS = [surface, data, draft, section, orphans, disclosure, card, field, fieldState, fieldPair, mode, run, tab, pick, edit];

export const DIMENSION_BY_ID = Object.fromEntries(DIMENSIONS.map((item) => [item.id, item]));

/** Every dimension except `surface` can be absent; `n/a` says so. */
export const OPTIONAL = ["data", "draft", "section", "orphans", "disclosure", "card", "field", "fieldState", "fieldPair", "mode", "run", "tab", "pick", "edit"];

for (const id of OPTIONAL) {
  DIMENSION_BY_ID[id].values = [NA, ...DIMENSION_BY_ID[id].values];
}

export function valuesOf(dimensionId) {
  return DIMENSION_BY_ID[dimensionId].values;
}

export function valueOf(dimensionId, valueId) {
  return valuesOf(dimensionId).find((value) => value.id === valueId);
}
