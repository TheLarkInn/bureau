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
//   { op: "wait",     selector }
//   { op: "waitGone", selector }

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
    dirty: { ops: [{ op: "fill", selector: ABORT, value: "bureau:failed" }, { op: "fill", selector: ESCALATE, value: "bureau:needs-human" }] },
    invalid: {
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
    invalid: { fixture: "read-only-primary", copy: ["is read-only, so no branch can land there"] },
  },
  "repos-add": { rest: { fixture: "multi-repo" } },
  limits: {
    rest: {},
    dirty: { ops: [{ op: "fill", selector: CONCURRENT_LIMIT, value: "3" }], copy: ["unsaved changes"] },
    invalid: { ops: [{ op: "fill", selector: CONCURRENT_LIMIT, value: "0" }], copy: ["need whole numbers of at least 1"] },
  },
  delete: { "n/a": {} },
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
 * Which run the overlay picks, per run equivalence class. The option only
 * exists once `GET /runs` has answered, so the path waits for it: relying on a
 * host's own retry would make the state reachable in one host and not the
 * other.
 */
export function runOps(mode, runValue) {
  if (["n/a", "none"].includes(runValue)) {
    return [];
  }
  const picker = mode === "live" ? S.runPickerLive : S.runPickerReplay;
  return [
    { op: "present", selector: `${picker} option[value="${RUN_IDS[runValue]}"]` },
    { op: "select", selector: picker, value: RUN_IDS[runValue] },
    { op: "wait", selector: ".run-status, .replay-timeline" },
  ];
}

/**
 * The fixtures a combination needs, one per layer, in application order. One
 * state gets one composed payload, chosen here rather than by merging every
 * dimension's preference, so the choice stays readable and testable.
 */
export function fixtureFor(combo) {
  if (combo.surface === "boot") {
    return [];
  }
  return [statusLayer(combo), ...contentLayer(combo), planLayer(combo), selectionLayer(combo)].filter(Boolean);
}

function statusLayer(combo) {
  return { fixture: "sample", invalid: "invalid", advisory: "advisory" }[combo.data] ?? "validated";
}

/**
 * Two independent content transforms can both be needed: the landing section
 * decides how many assignments exist, and the open field decides what that
 * assignment holds. They were competing for one slot, which made
 * `section:stack` and `section:two-cards` resolve to the same payload — two
 * registry states with one render between them. The section applies first so
 * a cloned assignment inherits the shape the section intended.
 */
function contentLayer(combo) {
  const section = { empty: "empty", "two-cards": "two-assignments", orphans: "orphans" }[combo.section] ?? null;
  const field = FIELD_LIFECYCLE[combo.field]?.[combo.fieldState]?.fixture ?? null;
  return [section, field];
}

function planLayer(combo) {
  return { pending: "draft-pending", "pending-one": "draft-single" }[combo.draft] ?? null;
}

function selectionLayer(combo) {
  return ["pipeline", "editor"].includes(combo.surface) ? "pipeline" : null;
}
