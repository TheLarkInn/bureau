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

import { SELECTORS as S, editorCardFor } from "./selectors.mjs";
const FILTER = '[data-testid="wr-filter"]';
const BRANCH = '[data-testid="wr-branch"]';
const ABORT = '[data-testid="sig-abort"]';
const ESCALATE = '[data-testid="sig-escalate"]';
const CONCURRENT_LIMIT = '[aria-label="Concurrent runs"]';
const BOARD_URL = "https://github.com/TheLarkInn/bureau/issues?q=is%3Aopen+label%3Aready";
const REPO_URL = "https://github.com/TheLarkInn/bureau-docs";

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
 */
export const FIELD_LIFECYCLE = {
  "work-source": {
    rest: {},
    dirty: {
      ops: [{ op: "fill", selector: S.workSourceUrl, value: BOARD_URL }, { op: "wait", selector: S.workSourceDerived }],
      copy: ["derived exactly from the URL"],
    },
  },
  "work-rules": {
    rest: {},
    dirty: { ops: [{ op: "fill", selector: FILTER, value: "is:open label:ready" }] },
    invalid: { ops: [{ op: "fill", selector: BRANCH, value: "" }], copy: ["Filter and branch prefix cannot be empty."] },
  },
  "forge-signals": {
    rest: { fixture: "no-signals" },
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
  "repos-add": { rest: { fixture: "multi-repo" } },
  limits: {
    rest: {},
    dirty: { ops: [{ op: "fill", selector: CONCURRENT_LIMIT, value: "3" }], copy: ["unsaved changes"] },
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
