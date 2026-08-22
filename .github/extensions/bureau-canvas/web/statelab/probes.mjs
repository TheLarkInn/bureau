// Crossing probes and content samples: the renders the product does not hold.
//
// Two different things live here, and conflating them was a defect.
//
// A CROSSING probe renders a combination a `scoping` rule excluded.
// `chrome-is-orthogonal-to-body` and `draft-is-orthogonal-to-body` claim that
// two regions share no state. That claim is true of React state and false of
// layout — a draft bar and an expanded card are stacked in the same flex
// column, and a findings strip pushes everything below it. So each scoping
// rule owes the matrix a probe that renders the crossing it excluded and lets
// the overlap and clipping checks run against it. A crossing carries the full
// dimension tuple it stands for, so `statelab.test.mjs` can check that the
// tuple really is rejected by the rule it names. A `rule` here is a claim
// under test, not a label.
//
// A CONTENT sample renders a payload shape the dimensions do not model at all
// — a repo with no credential, every limit capped, a pipeline that is gone.
// No rule excluded it, so it names none; it says what it `covers` instead.
// These used to carry a `rule` field picked for looks, which the lab and the
// gallery then printed as fact.
//
// Both are ordinary states: same shape, same driver, same assertions.

import { SELECTORS as S, editorCardFor } from "./selectors.mjs";
import { SAMPLE_STEPS } from "./paths.mjs";

/** A resting, reachable config landing — the baseline every crossing perturbs. */
const CONFIG_BASE = {
  surface: "config",
  data: "validated",
  draft: "none",
  section: "stack",
  orphans: "none",
  disclosure: "none",
  card: "collapsed",
  field: "n/a",
  fieldState: "n/a",
  mode: "n/a",
  run: "n/a",
  tab: "n/a",
  pick: "n/a",
  edit: "n/a",
};

/** A resting, reachable pipeline editor — the baseline the tab crossings perturb. */
const EDITOR_BASE = {
  surface: "editor",
  data: "n/a",
  draft: "n/a",
  section: "n/a",
  orphans: "n/a",
  disclosure: "n/a",
  card: "n/a",
  field: "n/a",
  fieldState: "n/a",
  mode: "n/a",
  run: "n/a",
  tab: "pipeline",
  pick: "none",
  edit: "rest",
};

function state({ id, summary, page = "index", surface, fixture, ops, expect, ...rest }) {
  return {
    id,
    kind: "probe",
    summary,
    page,
    fixture,
    surface: surface ?? (page === "editor" ? "editor" : "config"),
    ops: [{ op: "page", value: page }, { op: "fixture", value: fixture }, ...ops],
    expect,
    ...rest,
  };
}

/** A combination a scoping rule kept out of the product, rendered anyway. */
function crossing({ rule, dimensions, base = CONFIG_BASE, ...spec }) {
  return state({ ...spec, rule, dimensions: { ...base, ...dimensions } });
}

/** A payload shape the dimensions do not model; it excludes nothing. */
function sample({ covers, ...spec }) {
  return state({ ...spec, covers, dimensions: {} });
}

export const PROBES = [
  crossing({
    id: "probe--draft-over-expanded-card",
    rule: "draft-is-orthogonal-to-body",
    dimensions: { draft: "pending", card: "expanded", field: "none" },
    summary: "unsaved changes while an assignment card is expanded — the two stack, so neither may clip the other",
    fixture: "draft-pending",
    ops: [
      { op: "wait", selector: S.draftBar },
      { op: "click", selector: S.assignmentHead },
      { op: "wait", selector: S.assignmentDetail },
    ],
    expect: { shows: [S.draftBar, S.assignmentDetail, S.draftSave], hides: [], copy: ["3 unsaved changes"] },
  }),
  crossing({
    id: "probe--draft-over-open-limits",
    rule: "draft-is-orthogonal-to-body",
    dimensions: { draft: "pending", card: "expanded", field: "limits", fieldState: "rest" },
    summary: "a draft bar above an open limits editor: the tallest body region under the tallest chrome region",
    fixture: "draft-pending",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.limitsValue },
      { op: "wait", selector: S.limitsEditor },
    ],
    expect: { shows: [S.draftBar, S.limitsEditor, S.limitRow], hides: [], copy: [] },
  }),
  crossing({
    id: "probe--findings-over-expanded-card",
    rule: "chrome-is-orthogonal-to-body",
    dimensions: { data: "invalid", card: "expanded", field: "none" },
    summary: "validation findings above and inside an expanded card — the card owns its own finding list too",
    fixture: "invalid",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "wait", selector: S.assignmentDetail },
    ],
    expect: { shows: [S.assignmentDetail, ".general-findings"], hides: [], copy: ["Validation findings"] },
  }),
  crossing({
    id: "probe--findings-over-open-repos",
    rule: "chrome-is-orthogonal-to-body",
    dimensions: { data: "invalid", card: "expanded", field: "repos", fieldState: "rest" },
    summary: "a findings strip above a repo editor that draws its own notes",
    fixture: "invalid",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "wait", selector: S.reposEditor },
    ],
    expect: { shows: [S.reposEditor, S.reposSave], hides: [], copy: [] },
  }),
  crossing({
    id: "probe--create-bar-over-expanded-card",
    rule: "a-disclosure-is-reviewed-against-a-resting-card",
    dimensions: { disclosure: "create", card: "expanded", field: "none" },
    summary: "the create form open above an expanded card — both are landing regions and both are tall",
    fixture: "validated",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.createOpen },
      { op: "wait", selector: S.createBar },
    ],
    expect: { shows: [S.createBar, S.assignmentDetail], hides: [], copy: [] },
  }),
  crossing({
    id: "probe--relation-open-under-expanded-card",
    rule: "a-disclosure-is-reviewed-against-a-resting-card",
    dimensions: { disclosure: "relation-open", card: "expanded", field: "none" },
    summary: "the relation graph expanded below an expanded card, so the graph gets a real height",
    fixture: "validated",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.relationSummary },
      { op: "wait", selector: S.relationFlow },
    ],
    expect: { shows: [S.relationFlow, S.assignmentDetail], hides: [], copy: [] },
  }),
  crossing({
    id: "probe--orphans-under-expanded-card",
    rule: "one-body-variation-at-a-time",
    dimensions: { orphans: "present", card: "expanded", field: "none" },
    summary: "the orphan strip below an expanded card — the leftovers keep their own heading under a tall card",
    fixture: "orphans",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "wait", selector: S.assignmentDetail },
    ],
    expect: { shows: [S.orphanStrip, S.assignmentDetail], hides: [], copy: ["Unreferenced"] },
  }),
  // The Relations tab keeps `PipelineEditor` mounted and merely `hidden`, so
  // both tab rules are `scoping`, not structural — and each owes a crossing.
  crossing({
    id: "probe--selection-behind-relations-tab",
    rule: "selection-needs-the-pipeline-tab",
    base: EDITOR_BASE,
    dimensions: { tab: "relations", pick: "deterministic", edit: "n/a" },
    summary: "a step selected, then Relations shown — the selection is held, and none of it may leak onto the relation graph",
    page: "editor",
    fixture: "pipeline",
    ops: [
      { op: "click", selector: editorCardFor(SAMPLE_STEPS.deterministic) },
      { op: "wait", selector: S.editorStepName },
      { op: "click", selector: S.editorTabRelations },
      { op: "wait", selector: S.relationFlow },
    ],
    expect: { shows: [S.relationFlow, S.editorTabs], hides: [S.editorStepName, S.editorPanel], copy: [] },
  }),
  crossing({
    id: "probe--dirty-editor-behind-relations-tab",
    rule: "mutation-needs-the-pipeline-tab",
    base: EDITOR_BASE,
    dimensions: { tab: "relations", pick: "deterministic", edit: "renamed" },
    summary: "an unsaved rename held behind the Relations tab — draft safety says it is kept, and the graph must not show it",
    page: "editor",
    fixture: "pipeline",
    ops: [
      { op: "click", selector: editorCardFor(SAMPLE_STEPS.deterministic) },
      { op: "fill", selector: S.editorStepName, value: "deterministic-renamed" },
      { op: "press", selector: S.editorStepName, value: "Enter" },
      { op: "wait", selector: S.editorDiscard },
      { op: "click", selector: S.editorTabRelations },
      { op: "wait", selector: S.relationFlow },
    ],
    expect: { shows: [S.relationFlow, S.editorTabs], hides: [S.editorStepName, S.editorPanel], copy: [] },
  }),
  sample({
    id: "probe--draft-survives-a-tab-round-trip",
    covers: "draft safety across a tab switch, which no single tuple can express: leaving and returning must not discard an unsaved rename",
    summary: "an unsaved rename survives a trip to Relations and back — the editor may never discard a draft the user did not discard",
    page: "editor",
    fixture: "pipeline",
    ops: [
      { op: "click", selector: editorCardFor(SAMPLE_STEPS.deterministic) },
      { op: "fill", selector: S.editorStepName, value: "deterministic-renamed" },
      { op: "press", selector: S.editorStepName, value: "Enter" },
      { op: "wait", selector: S.editorDiscard },
      { op: "click", selector: S.editorTabRelations },
      { op: "waitGone", selector: S.editorDiscard },
      { op: "click", selector: S.editorTabPipeline },
      { op: "wait", selector: S.editorDiscard },
    ],
    expect: { shows: [S.editorDiscard, S.editorSave], hides: [], copy: ["deterministic-renamed"] },
  }),
  sample({
    id: "probe--two-disclosures-open",
    covers: "two field editors open at once, which the single-valued `field` axis cannot express",
    summary: "two field disclosures open at once — each field owns its own state, so nothing closes the first",
    fixture: "multi-repo",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.limitsValue },
      { op: "click", selector: S.reposValue },
      { op: "wait", selector: S.reposEditor },
    ],
    expect: { shows: [S.limitsEditor, S.reposEditor], hides: [], copy: [] },
  }),
  sample({
    id: "probe--editor-missing-pipeline",
    covers: "the editor opened on a pipeline name the config no longer has",
    summary: "the editor opened on a pipeline that no longer exists",
    page: "editor",
    fixture: "pipeline-missing",
    ops: [{ op: "wait", selector: S.editorMissing }],
    expect: { shows: [S.editorTabs], hides: [S.editorShell], copy: ["No pipeline named"] },
  }),
  sample({
    id: "probe--editor-no-pipeline",
    covers: "the editor opened with no pipeline selected at all",
    summary: "the editor opened with nothing selected at all",
    page: "editor",
    fixture: "no-pipeline",
    ops: [{ op: "wait", selector: S.editorMissing }],
    expect: { shows: [S.editorTabs], hides: [S.editorShell], copy: ["Open a pipeline from the config view first."] },
  }),
  sample({
    id: "probe--pipeline-missing-in-viewer",
    covers: "the viewer pointed at a pipeline the config no longer has",
    summary: "the viewer pointed at a pipeline that is gone",
    surface: "pipeline",
    fixture: "pipeline-missing",
    ops: [{ op: "wait", selector: S.shell }],
    expect: { shows: [S.shell], hides: [], copy: ["No pipeline named"] },
  }),
  sample({
    id: "probe--no-credential-registry",
    covers: "a repo registry in which nothing names a credential",
    summary: "registering a repo when no registered repo names a credential",
    fixture: "no-credential",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "click", selector: S.reposAdd },
      { op: "wait", selector: S.reposUrl },
    ],
    expect: { shows: [S.reposUrl], hides: [], copy: ["add a repo"] },
  }),
  sample({
    id: "probe--unknown-primary-repo",
    covers: "a primary repo absent from repos.yaml, which is a different fix from read-only",
    summary: "the primary repo is missing from repos.yaml, which is a different fix from read-only",
    fixture: "unknown-primary",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "wait", selector: S.reposEditor },
    ],
    expect: { shows: [S.reposEditor], hides: [], copy: ["is not in repos.yaml"] },
  }),
  sample({
    id: "probe--all-limits-capped",
    covers: "every limit set, so the summary is chips rather than the unbounded pill",
    summary: "every limit capped, so the summary is chips rather than the unbounded pill",
    fixture: "all-limits",
    ops: [{ op: "click", selector: S.assignmentHead }, { op: "wait", selector: S.limitsValue }],
    expect: { shows: [S.limitsValue], hides: [], copy: ["2 at once", "12 h/run"] },
  }),
  sample({
    id: "probe--no-limits-set",
    covers: "no limit set at all, so the summary has to say unbounded out loud",
    summary: "no limit set at all — the summary has to say unbounded out loud",
    fixture: "no-limits",
    ops: [{ op: "click", selector: S.assignmentHead }, { op: "wait", selector: S.limitsValue }],
    expect: { shows: [S.limitsValue], hides: [], copy: ["unbounded — no limits set"] },
  }),
];
