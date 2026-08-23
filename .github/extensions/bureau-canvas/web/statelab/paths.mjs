// How each state in the matrix is reached, as data.
//
// An entry path is an ordered list of operations. The same list is executed by
// the lab (against an iframe holding the real page) and by the browser suite
// (against a Playwright page), so "reachable" means one thing: a real user
// could get here by doing exactly this. There is no back door that sets a
// component's internal state directly.
//
// Operations:
//   { op: "page",     value }             load index.html or editor.html
//   { op: "fixture",  value }             publish a fixture payload to the page
//   { op: "click",    selector }
//   { op: "fill",     selector, value }
//   { op: "select",   selector, value }
//   { op: "press",    selector, value }   send one key to a focused control
//   { op: "drag",     selector, dx, dy }  move a node on a graph canvas
//   { op: "wait",     selector }          wait until visible
//   { op: "present",  selector }          wait until attached, visible or not
//   { op: "waitGone", selector }          wait until hidden
//
// `driver.mjs` implements exactly this set, and an offline test fails if a
// path uses a verb that is not on it.

import { SELECTORS as S, cleanEditor, dirtyEditor, draftMarkIn, editorCardFor, withheld } from "./selectors.mjs";
const FILTER = '[data-testid="wr-filter"]';
const BRANCH = '[data-testid="wr-branch"]';
const ABORT = '[data-testid="sig-abort"]';
const ESCALATE = '[data-testid="sig-escalate"]';
const CONCURRENT_LIMIT = '[aria-label="Concurrent runs"]';
const BOARD_URL = "https://github.com/TheLarkInn/bureau/issues?q=is%3Aopen+label%3Aready";
/**
 * A repository the `multi-repo` fixture has not registered, so pasting it
 * really does reach the resolved preview with its Add offered. Naming one the
 * fixture already holds would draw the "already names a different repository"
 * refusal instead, which is a different screen from the one this covers.
 */
export const REPO_ADD_URL = "https://github.com/microsoft/rushstack";
/**
 * The other two answers `deriveWorkSource` can give, which the paste has to
 * report differently. A host it does not know is a refusal — no derivation, no
 * save. A label page is a derivation Bureau had to *infer*, which is offered
 * but must say so, because a filter that silently means something else is the
 * hazard `lib/worksource.mjs` exists to prevent.
 */
export const UNKNOWN_HOST_URL = "https://example.com/board";
export const INFERRED_FILTER_URL = "https://github.com/TheLarkInn/bureau/labels/bug";

/** The steps the bundled sample's pipeline actually contains, by kind. */
export const SAMPLE_STEPS = { deterministic: "verify", agent: "implement" };

/**
 * The sample pipeline's step count, and therefore the name `addStep` gives the
 * next one (`step-<count + 1>`). `test/statelab.test.mjs` reads the committed
 * payload and fails if this drifts, so the registry cannot quietly address a
 * step that is not there.
 */
export const SAMPLE_STEP_COUNT = 3;
export const ADDED_STEP = `step-${SAMPLE_STEP_COUNT + 1}`;

/**
 * Per-field lifecycle: how an open field editor is driven from `rest` into a
 * `dirty` draft or an `invalid` one, and which fixture makes that meaningful.
 *
 * A lifecycle value names its own fixture — nothing is inherited from `rest`,
 * because `contentLayer` reads this one entry. A `dirty` that leaned on
 * `rest`'s fixture would type the values the payload already held, leave the
 * form unchanged, and render a *clean* editor under the name `dirty`.
 *
 * An entry contributes `shows` and `hides` as well as `copy`, because for
 * several fields the refusal is a *treatment* and not only a sentence: an
 * error rendered in the ordinary note class reads as advice.
 */
const FIELD_DRAFTS = {
  "work-source": {
    rest: {},
    dirty: {
      ops: [{ op: "fill", selector: S.workSourceUrl, value: BOARD_URL }, { op: "wait", selector: S.workSourceDerived }],
      copy: ["derived exactly from the URL"],
    },
    // A URL the deriver refuses. The preview must be *gone*, not stale: the
    // editor may not keep showing what the last good paste would have written
    // while telling the user this one failed.
    invalid: {
      ops: [{ op: "fill", selector: S.workSourceUrl, value: UNKNOWN_HOST_URL }, { op: "wait", selector: S.workSourceRefused }],
      shows: [S.workSourceRefused],
      hides: [S.workSourceDerived],
      copy: ["unrecognized host"],
    },
  },
  "work-rules": {
    rest: {},
    dirty: { ops: [{ op: "fill", selector: FILTER, value: "is:open label:ready" }] },
    invalid: { ops: [{ op: "fill", selector: BRANCH, value: "" }], copy: ["Filter and branch prefix cannot be empty."] },
  },
  "forge-signals": {
    // Rest carries no fixture, and that is load-bearing rather than incidental.
    // A lifecycle entry's fixture is part of the state's entry path, so a
    // `rest` that publishes its own payload diverges from the resting card
    // *before* the click — and the prefix relation that builds the transition
    // DAG never forms, taking this disclosure's open and close edges with it.
    // Opening and closing the signals editor does not need unset labels; only
    // a draft that must differ from what the payload already holds does.
    //
    // Nothing is lost by that here, because this field's `dirty` fills *two*
    // inputs and a prefix edge is a single operation: rest and dirty were never
    // an edge, whatever fixture rest carried. `repos` below is the case where
    // that is not true, and it keeps its fixture for exactly that reason.
    rest: {},
    dirty: {
      fixture: "no-signals",
      ops: [{ op: "fill", selector: ABORT, value: "bureau:failed" }, { op: "fill", selector: ESCALATE, value: "bureau:needs-human" }],
    },
    invalid: {
      fixture: "no-signals",
      ops: [{ op: "fill", selector: ABORT, value: "same" }, { op: "fill", selector: ESCALATE, value: "same" }],
      copy: ["Both labels are required and must differ."],
    },
  },
  repos: {
    // Unlike forge-signals, the resting repos editor keeps its fixture. Its
    // `dirty` is a *single* click — the reorder — so rest and dirty are a
    // prefix pair and the edge between them is walked; dropping the fixture
    // here to buy a card→field edge would have paid for it by deleting that
    // one, which is the interaction that makes this editor dirty at all. The
    // `.repos-value` disclosure gets its open and close edges from the
    // findings probe pair instead.
    rest: { fixture: "multi-repo" },
    dirty: {
      fixture: "multi-repo",
      ops: [{ op: "click", selector: '[aria-label="Move bureau-docs up"]' }],
    },
    invalid: {
      fixture: "read-only-primary",
      // The same reorder `dirty` performs, against a registry where the repo
      // being promoted cannot take a branch. Without the op the fixture's own
      // order would disable Save through `sameOrder` and the withheld-save
      // assertion would hold with the read-only gate deleted.
      ops: [{ op: "click", selector: '[aria-label="Move bureau-docs up"]' }],
      copy: ["is read-only, so no branch can land there"],
    },
  },
  "repos-add": { "n/a": { fixture: "multi-repo" } },
  limits: {
    rest: {},
    dirty: { ops: [{ op: "fill", selector: CONCURRENT_LIMIT, value: "3" }] },
    invalid: { ops: [{ op: "fill", selector: CONCURRENT_LIMIT, value: "0" }], copy: ["need whole numbers of at least 1"] },
  },
  delete: { "n/a": {} },
};

/**
 * The button each field editor saves through. This is what makes the lifecycle
 * axis assertable: `dirty` means the save is *offered* and `rest` and
 * `invalid` mean it is withheld, which is the whole of draft safety at the
 * field level. The repo adder and the delete confirmation are not draft
 * editors and have no save of their own.
 */
export const FIELD_SAVE = {
  "work-source": S.workSourceSave,
  "work-rules": S.workRulesSave,
  "forge-signals": S.signalsSave,
  repos: S.reposSave,
  limits: S.limitsSave,
};

/** The editor each field opens, so a refusal can be scoped to the field it names. */
const FIELD_EDITOR = {
  "work-source": S.workSourceEditor,
  "work-rules": S.workRulesEditor,
  "forge-signals": S.signalsEditor,
  repos: S.reposEditor,
  limits: S.limitsEditor,
};

/**
 * What each editor says when the host refuses without a message of its own.
 *
 * `postIntent` returns `null` for any non-`ok` response, so a refused save
 * falls back to the editor's own sentence — which makes the copy deterministic
 * under interception, and is why `save-error` can assert words at all.
 */
const FIELD_ERROR = {
  "work-source": "could not save that work source",
  "work-rules": "could not save those work rules",
  "forge-signals": "could not save those forge signals",
  repos: "could not save those repos",
  limits: "could not save those limits",
};

/**
 * The intercept each save lifecycle value needs, and the one thing that makes
 * these two states reviewable at all.
 *
 * A field save posts `set-*` to `./intent`, and the matrix shares one
 * read-only host across every state on the worker — so performing the save
 * would record a plan the following states would then inherit. That is a real
 * constraint, and it used to be discharged by excluding the states outright.
 * But "the harness may not press this button" is not the same claim as "this
 * screen cannot exist": a save in flight and a save refused are two of the
 * most ordinary things this UI does, and neither was ever rendered.
 *
 * Routing `./intent` in the browser settles both. The request is answered — or
 * deliberately never answered — before it leaves the page, so the host is
 * untouched and the states are exact rather than timing-dependent: stalling
 * pins `busy` on, and a refusal returns no `ok`, which is precisely the branch
 * that renders the editor's fallback sentence.
 */
export const SAVE_INTERCEPTS = { saving: "stall-intent", discarding: "stall-intent", "save-error": "fail-intent", "discard-error": "fail-intent" };

/**
 * Every axis whose values are answered by a routed `./intent`, and the one
 * place that decides which route a combination needs.
 *
 * `fieldState` was the first axis to get this treatment and for a while the
 * only one, which left four families — the plan bar's save, the pipeline
 * editor's save, a refused run control and a refused create — excluded by rules
 * that still gave the pre-interception reason: "the matrix may not write to the
 * host". That reason had stopped being true. `matrix-fixtures.mjs` holds every
 * intent that writes, under every state and not only the ones that ask for a
 * route, so none of those clicks reaches the host whether a rule excludes them
 * or not, and the rules were excluding screens that the harness could by then
 * render perfectly safely. The single exception is the delete preflight, which
 * writes nothing and is named as an exception by the harness rule that owns it.
 *
 * That is the failure mode this registry exists to prevent, so it is worth
 * naming exactly: a `structural` rule claims a screen *cannot be rendered*, and
 * kinding a harness limitation as one waives every obligation the registry has
 * — no probe is owed, and `test/statelab.test.mjs` only demands probes of
 * `scoping` rules. Two of the six screens were consequently asserted nowhere in
 * the repository, and one of them (`edit: saving`) was hiding a real defect:
 * the editor's Save stayed live for the whole round trip, so a second click
 * raced a second write against the first one's revert.
 */
export function interceptFor(combo) {
  const refusedRun = String(combo.run ?? "").startsWith("refused");
  const wanted = [
    SAVE_INTERCEPTS[combo.fieldState],
    SAVE_INTERCEPTS[combo.draft],
    SAVE_INTERCEPTS[combo.edit],
    refusedRun ? "fail-intent" : null,
    combo.run === "ended" ? "offer-ended-run" : null,
    combo.disclosure === "create-error" ? "fail-intent" : null,
  ].filter(Boolean);
  // One page gets one route. Two axes wanting different ones would mean a
  // state whose render depends on which was installed, so it is a registry
  // error rather than a preference — `test/statelab.test.mjs` asserts that no
  // state asks for two, and `registry.mjs` takes the first of these.
  return [...new Set(wanted)];
}

/**
 * The two ends of a save, derived from each field's own `dirty` path.
 *
 * They are derived rather than written out because they are the *same* draft:
 * a save that is only reachable from a different edit than the one `dirty`
 * makes would be asserting a screen no user reaches by the route the matrix
 * claims. So each takes `dirty`'s fixture and its keystrokes, and adds the one
 * click that is the whole difference.
 */
function saveStates(field, dirty) {
  const save = FIELD_SAVE[field];
  const refusal = `${FIELD_EDITOR[field]} .note--err`;
  const draft = [...(dirty.ops ?? []), { op: "click", selector: save }];
  return {
    saving: {
      fixture: dirty.fixture,
      ops: [...draft, { op: "wait", selector: withheld(save) }],
      copy: ["Saving…"],
    },
    // The refusal is asserted as a *treatment*, not only as words: an editor
    // that reported a failed save in the ordinary note class would read as
    // advice, and the draft would look saved.
    "save-error": {
      fixture: dirty.fixture,
      ops: [...draft, { op: "wait", selector: refusal }],
      shows: [refusal],
      copy: [FIELD_ERROR[field]],
    },
  };
}

/**
 * The draft contract every field editor now publishes, folded into the
 * lifecycle so all five fields make the same claim rather than one of them.
 *
 * `data-dirty` is what the two controls that navigate away from an open editor
 * read, so a field whose root said `false` while holding typed work would take
 * the reader off the screen without a word — and one that said `true` at rest
 * would demand a confirmation for changes nobody made. Both are silent
 * failures on a screen that otherwise looks perfect, which is exactly the class
 * of defect the marker makes visible to a human and this makes visible to CI.
 *
 * `rest` also names the marker absent. An editor that drew "unsaved changes"
 * before anything was typed satisfies every other expectation this state has.
 */
function draftContract(field, name, state) {
  const editor = FIELD_EDITOR[field];
  if (!editor || !["rest", "dirty", "invalid"].includes(name)) {
    return state;
  }
  const held = name !== "rest";
  return {
    ...state,
    shows: [...(state.shows ?? []), held ? dirtyEditor(editor) : cleanEditor(editor), ...(held ? [draftMarkIn(editor)] : [])],
    hides: [...(state.hides ?? []), ...(held ? [cleanEditor(editor)] : [draftMarkIn(editor), dirtyEditor(editor)])],
    copy: [...(state.copy ?? []), ...(held ? ["unsaved changes"] : [])],
  };
}

/** Every field's drafts, plus the two save states for the fields that save. */
export const FIELD_LIFECYCLE = Object.fromEntries(
  Object.entries(FIELD_DRAFTS).map(([field, states]) => [
    field,
    Object.fromEntries(Object.entries(FIELD_SAVE[field] ? { ...states, ...saveStates(field, states.dirty) } : states)
      .map(([name, state]) => [name, draftContract(field, name, state)])),
  ]),
);

/** How the editor is driven into each mutation state, per selected step kind. */
const renamedPath = (kind) => [
  ...selectStep(kind),
  { op: "fill", selector: S.editorStepName, value: `${kind}-renamed` },
  { op: "press", selector: S.editorStepName, value: "Enter" },
];

export const EDIT_PATHS = {
  rest: () => [],
  created: (kind) => [
    { op: "select", selector: S.editorAddKind, value: kind },
    { op: "click", selector: S.editorAddStep },
  ],
  renamed: renamedPath,
  "delete-confirm": (kind) => [...selectStep(kind), { op: "click", selector: S.editorDeleteStep }],
  // Answering the confirmation. The wait is on the card going away rather than
  // on the panel, because the card is the thing the click was about — a panel
  // that cleared its selection without removing the step would pass a wait on
  // the empty prompt.
  deleted: (kind) => [
    ...EDIT_PATHS["delete-confirm"](kind),
    { op: "click", selector: S.editorDeleteConfirm },
    { op: "waitGone", selector: editorCardFor(stepFor(kind)) },
  ],
  "layout-moved": (kind) => [...selectStep(kind), { op: "drag", selector: editorCardFor(stepFor(kind)), dx: 80, dy: 60 }],
  invalid: (kind) => [...selectStep(kind), { op: "fill", selector: S.editorMaxAttempts, value: "0" }],
  /*
   * The two ends of a pipeline save, derived from `renamed` for the same reason
   * the field saves derive from their own `dirty`: a save reachable only from a
   * different edit than the one the path claims would assert a screen no user
   * reaches by that route. `renamed` is the dirty edit that leaves the graph
   * valid, so Save is offered and the click is the whole difference.
   */
  saving: (kind) => [
    ...EDIT_PATHS.renamed(kind),
    { op: "click", selector: S.editorSave },
    { op: "wait", selector: withheld(S.editorSave) },
  ],
  "save-error": (kind) => [
    ...EDIT_PATHS.renamed(kind),
    { op: "click", selector: S.editorSave },
    { op: "wait", selector: S.editorSaveReverted },
  ],
};

/**
 * The sample pipeline ships a deterministic and an agent step. A decision or
 * concurrent selection is reached by adding one — which is what a user does,
 * so the path is the honest one rather than a doctored fixture.
 */
export function selectStep(kind) {
  if (SAMPLE_STEPS[kind]) {
    return [{ op: "click", selector: editorCardFor(SAMPLE_STEPS[kind]) }];
  }
  return [
    { op: "select", selector: S.editorAddKind, value: kind },
    { op: "click", selector: S.editorAddStep },
  ];
}

export function stepFor(kind) {
  return SAMPLE_STEPS[kind] ?? ADDED_STEP;
}

/** Which run the overlay picks, per run equivalence class. */
export const RUN_IDS = {
  running: "run-live",
  paused: "run-paused",
  finished: "run-finished",
  // The same committed log the replay side calls `finished`. What differs is
  // where it is picked from: `offer-ended-run` reports it to the live listing,
  // which is the instant after a watched run reaches its terminal.
  ended: "run-finished",
};

/**
 * The `at_ms` of the last event in each committed log. The replay timeline
 * takes its `max` straight from it, so this is what ties a replay render to
 * the run it claims to be showing. `test/statelab.test.mjs` reads the logs and
 * fails if these drift, so the registry cannot address a span that is not there.
 */
export const RUN_END = {
  running: 1740000002000,
  paused: 1740000105000,
  finished: 1740000210000,
};

/**
 * Where the transport parks, per run: the first event's stamp, the next one a
 * forward step lands on, and the readout that position produces. Stepping is
 * the deterministic half of replay — play advances on a timer, this does not —
 * so these are what tell a transport that moves from one that only draws.
 * `test/statelab.test.mjs` derives all three from the same logs the way
 * `stepBy` does, and fails if either drifts.
 */
export const RUN_STEP = {
  running: { start: 1740000000000, next: 1740000001000, readout: "+1.0s" },
  paused: { start: 1740000100000, next: 1740000101000, readout: "+1.0s" },
  finished: { start: 1740000200000, next: 1740000201000, readout: "+1.0s" },
};

/**
 * Which run the overlay picks, per run equivalence class. The option only
 * exists once `GET /runs` has answered, so the path waits for it: relying on a
 * host's own retry would make the state reachable in one host and not the
 * other.
 *
 * Selecting is not arriving. Both modes then fetch the run's log and fold it
 * through the overlay reducer, so the controls render before the run they
 * describe exists — live shows `idle` and replay spans nothing. Each mode
 * therefore waits for the first thing that can only be true once the log has
 * been applied: the step decoration in live, the timeline's own span in
 * replay. Both are `wait` verbs, so neither adds an action to the path.
 */
export function runOps(mode, runValue) {
  if (["n/a", "none"].includes(runValue)) {
    return [];
  }
  const picker = mode === "live" ? S.runPickerLive : S.runPickerReplay;
  return [
    { op: "present", selector: `${picker} option[value="${RUN_IDS[runValue]}"]` },
    { op: "select", selector: picker, value: RUN_IDS[runValue] },
    ...(mode === "live" ? liveArrival(runValue) : [{ op: "wait", selector: S.replayLoaded }]),
  ];
}

/**
 * Live folds the log into step decoration; that is what proves it landed.
 *
 * An ended run has no running step to wait on — every step in its log has
 * already completed — so what proves the fold landed there is the status the
 * fold produced, which is also the thing that withdrew the transport.
 */
function liveArrival(runValue) {
  if (runValue === "ended") {
    return [{ op: "wait", selector: S.runStatusFinished }];
  }
  return [{ op: "wait", selector: runValue === "paused" ? S.overlayPaused : S.overlayRunning }];
}

/**
 * The fixtures a combination needs, one per layer, in application order. One
 * state gets one composed payload, chosen here rather than by merging every
 * dimension's preference, so the choice stays readable and testable.
 */
export function fixtureFor(combo) {
  // Both boot surfaces publish nothing: one is waiting for a payload, the
  // other never mounted a renderer to receive one.
  if (combo.surface === "boot" || combo.surface === "boot-editor") {
    return [];
  }
  return [statusLayer(combo), ...contentLayer(combo), planLayer(combo), selectionLayer(combo)].filter(Boolean);
}

function statusLayer(combo) {
  return { fixture: "sample", invalid: "invalid", advisory: "advisory", "invalid-advisory": "invalid-advisory" }[combo.data] ?? "validated";
}

/**
 * Two independent content transforms can both be needed: the landing section
 * decides how many assignments exist, the orphan strip decides what nothing
 * references, and the open field decides what that assignment holds. They were
 * competing for one slot, which made `section:stack` and `section:two-cards`
 * resolve to the same payload — two registry states with one render between
 * them. The section applies first so a cloned assignment inherits the shape the
 * section intended, and the orphan layer applies after it because an empty
 * config clears the strip on its way through.
 */
function contentLayer(combo) {
  const section = { empty: "empty", "two-cards": "two-assignments" }[combo.section] ?? null;
  const unreferenced = combo.orphans === "present" ? "orphans" : null;
  const field = FIELD_LIFECYCLE[combo.field]?.[combo.fieldState]?.fixture ?? null;
  return [section, unreferenced, field];
}

function planLayer(combo) {
  // The in-flight and refused screens need a plan to be acting on, and it is
  // the same plan `pending` publishes — a save state that invented its own plan
  // would be asserting a bar no user reaches from the bar next to it.
  return { pending: "draft-pending", "pending-one": "draft-single", saving: "draft-pending", discarding: "draft-pending", "save-error": "draft-pending", "discard-error": "draft-pending" }[combo.draft] ?? null;
}

/**
 * The draft bar's own save, driven to whichever end the intercept has set up.
 *
 * The bar is an index-level region drawn above whatever body the surface has,
 * so these ride at the end of the entry path rather than inside a surface's own
 * ops: the click is on the bar, and the body is at rest underneath it.
 */
export function draftOps(draftValue) {
  if (!SAVE_INTERCEPTS[draftValue]) {
    return [];
  }
  // Each in-flight and each refusal is pressed on the button whose verb it
  // names, so the sentence under review is the one that button owns.
  if (draftValue === "saving" || draftValue === "discarding") {
    const button = draftValue === "saving" ? S.draftSave : S.draftDiscard;
    return [{ op: "click", selector: button }, { op: "wait", selector: withheld(button) }];
  }
  const selector = draftValue === "discard-error" ? S.draftDiscard : S.draftSave;
  return [{ op: "click", selector }, { op: "wait", selector: S.draftRefused }];
}

/**
 * Refusing a run control, per verb.
 *
 * `live.js` names three — pause, resume and cancel — and it names them
 * separately on purpose: "could not pause" and "could not cancel" leave the run
 * in opposite places, so a single shared sentence would be a lie about one of
 * them. Only cancel was ever under test, which meant two thirds of that claim
 * rested on nothing: collapsing pause and resume back into one message, or into
 * cancel's, would have failed no assertion in the repository.
 *
 * Each verb is pressed on the run that actually offers it. Pause and cancel are
 * offered on a running run; resume needs a paused one, so it enters through a
 * different run and is a genuinely different screen rather than the same one
 * with a different button pressed. `fail-intent` answers every one of them in
 * the browser, so no real run is acted on.
 */
export const RUN_REFUSALS = {
  "refused-cancel": { run: "running", control: S.runCancel, verb: "cancel" },
  "refused-pause": { run: "running", control: S.runPause, verb: "pause" },
  "refused-resume": { run: "paused", control: S.runResume, verb: "resume" },
};

export function runRefusalOps(runValue) {
  const refusal = RUN_REFUSALS[runValue];
  return [
    ...runOps("live", refusal.run),
    { op: "click", selector: refusal.control },
    { op: "wait", selector: S.runControlError },
  ];
}

function selectionLayer(combo) {
  return ["pipeline", "editor"].includes(combo.surface) ? "pipeline" : null;
}
