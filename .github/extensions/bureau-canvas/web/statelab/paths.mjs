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

import { SELECTORS as S, editorCardFor, withheld } from "./selectors.mjs";
const FILTER = '[data-testid="wr-filter"]';
const BRANCH = '[data-testid="wr-branch"]';
const ABORT = '[data-testid="sig-abort"]';
const ESCALATE = '[data-testid="sig-escalate"]';
const CONCURRENT_LIMIT = '[aria-label="Concurrent runs"]';
const BOARD_URL = "https://github.com/TheLarkInn/bureau/issues?q=is%3Aopen+label%3Aready";
const REPO_URL = "https://github.com/TheLarkInn/bureau-docs";
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
    // As above: the ranked list opens and closes on whatever the config holds,
    // so rest stays on the resting card's payload and keeps its edges. The
    // reorder that `dirty` performs is what needs a second repo.
    rest: {},
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
  "repos-add": { "n/a": {} },
  limits: {
    rest: {},
    dirty: { ops: [{ op: "fill", selector: CONCURRENT_LIMIT, value: "3" }], shows: [S.limitsDirty], copy: ["unsaved changes"] },
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
export const SAVE_INTERCEPTS = { saving: "stall-intent", "save-error": "fail-intent" };

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

/** Every field's drafts, plus the two save states for the fields that save. */
export const FIELD_LIFECYCLE = Object.fromEntries(
  Object.entries(FIELD_DRAFTS).map(([field, states]) => [
    field,
    FIELD_SAVE[field] ? { ...states, ...saveStates(field, states.dirty) } : states,
  ]),
);

/** How the editor is driven into each mutation state, per selected step kind. */
export const EDIT_PATHS = {
  rest: () => [],
  created: (kind) => [
    { op: "select", selector: S.editorAddKind, value: kind },
    { op: "click", selector: S.editorAddStep },
  ],
  renamed: (kind) => [
    ...selectStep(kind),
    { op: "fill", selector: S.editorStepName, value: `${kind}-renamed` },
    { op: "press", selector: S.editorStepName, value: "Enter" },
  ],
  "delete-confirm": (kind) => [...selectStep(kind), { op: "click", selector: S.editorDeleteStep }],
  "layout-moved": (kind) => [...selectStep(kind), { op: "drag", selector: editorCardFor(stepFor(kind)), dx: 80, dy: 60 }],
  invalid: (kind) => [...selectStep(kind), { op: "fill", selector: S.editorMaxAttempts, value: "0" }],
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

/** Live folds the log into step decoration; that is what proves it landed. */
function liveArrival(runValue) {
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
  return { pending: "draft-pending", "pending-one": "draft-single" }[combo.draft] ?? null;
}

function selectionLayer(combo) {
  return ["pipeline", "editor"].includes(combo.surface) ? "pipeline" : null;
}
