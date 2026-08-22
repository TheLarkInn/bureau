// Crossing probes: the combinations the two scoping rules keep out of the
// product, rendered anyway.
//
// `chrome-is-orthogonal-to-body` and `draft-is-orthogonal-to-body` claim that
// two regions share no state. That claim is true of React state and false of
// layout — a draft bar and an expanded card are stacked in the same flex
// column, and a findings strip pushes everything below it. So each scoping
// rule owes the matrix a probe that renders the crossing it excluded and lets
// the overlap and clipping checks run against it.
//
// A probe is an ordinary state: same shape, same driver, same assertions. It
// carries `kind: "probe"` and the rule it is holding to account.

import { SELECTORS as S } from "./selectors.mjs";

function probe({ id, rule, summary, page = "index", fixture, ops, expect }) {
  return {
    id,
    kind: "probe",
    rule,
    summary,
    page,
    fixture,
    surface: page === "editor" ? "editor" : "config",
    dimensions: {},
    ops: [{ op: "page", value: page }, { op: "fixture", value: fixture }, ...ops],
    expect,
  };
}

export const PROBES = [
  probe({
    id: "probe--draft-over-expanded-card",
    rule: "draft-is-orthogonal-to-body",
    summary: "unsaved changes while an assignment card is expanded — the two stack, so neither may clip the other",
    fixture: "draft-pending",
    ops: [
      { op: "wait", selector: S.draftBar },
      { op: "click", selector: S.assignmentHead },
      { op: "wait", selector: S.assignmentDetail },
    ],
    expect: { shows: [S.draftBar, S.assignmentDetail, S.draftSave], hides: [], copy: ["3 unsaved changes"] },
  }),
  probe({
    id: "probe--draft-over-open-limits",
    rule: "draft-is-orthogonal-to-body",
    summary: "a draft bar above an open limits editor: the tallest body region under the tallest chrome region",
    fixture: "draft-pending",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.limitsValue },
      { op: "wait", selector: S.limitsEditor },
    ],
    expect: { shows: [S.draftBar, S.limitsEditor, S.limitRow], hides: [], copy: [] },
  }),
  probe({
    id: "probe--findings-over-expanded-card",
    rule: "chrome-is-orthogonal-to-body",
    summary: "validation findings above and inside an expanded card — the card owns its own finding list too",
    fixture: "invalid",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "wait", selector: S.assignmentDetail },
    ],
    expect: { shows: [S.assignmentDetail, ".general-findings"], hides: [], copy: ["Validation findings"] },
  }),
  probe({
    id: "probe--findings-over-open-repos",
    rule: "chrome-is-orthogonal-to-body",
    summary: "a findings strip above a repo editor that draws its own notes",
    fixture: "invalid",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "wait", selector: S.reposEditor },
    ],
    expect: { shows: [S.reposEditor, S.reposSave], hides: [], copy: [] },
  }),
  probe({
    id: "probe--create-bar-over-expanded-card",
    rule: "one-body-variation-at-a-time",
    summary: "the create form open above an expanded card — both are landing regions and both are tall",
    fixture: "validated",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.createOpen },
      { op: "wait", selector: S.createBar },
    ],
    expect: { shows: [S.createBar, S.assignmentDetail], hides: [], copy: [] },
  }),
  probe({
    id: "probe--relation-open-under-expanded-card",
    rule: "one-body-variation-at-a-time",
    summary: "the relation graph expanded below an expanded card, so the graph gets a real height",
    fixture: "validated",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.relationSummary },
      { op: "wait", selector: S.relationFlow },
    ],
    expect: { shows: [S.relationFlow, S.assignmentDetail], hides: [], copy: [] },
  }),
  probe({
    id: "probe--two-disclosures-open",
    rule: "fields-need-an-open-card",
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
  probe({
    id: "probe--editor-missing-pipeline",
    rule: "tabs-are-editor-only",
    summary: "the editor opened on a pipeline that no longer exists",
    page: "editor",
    fixture: "pipeline-missing",
    ops: [{ op: "wait", selector: S.editorMissing }],
    expect: { shows: [S.editorTabs], hides: [S.editorShell], copy: ["No pipeline named"] },
  }),
  probe({
    id: "probe--editor-no-pipeline",
    rule: "tabs-are-editor-only",
    summary: "the editor opened with nothing selected at all",
    page: "editor",
    fixture: "no-pipeline",
    ops: [{ op: "wait", selector: S.editorMissing }],
    expect: { shows: [S.editorTabs], hides: [S.editorShell], copy: ["Open a pipeline from the config view first."] },
  }),
  probe({
    id: "probe--pipeline-missing-in-viewer",
    rule: "mode-is-pipeline-only",
    summary: "the viewer pointed at a pipeline that is gone",
    fixture: "pipeline-missing",
    ops: [{ op: "wait", selector: S.shell }],
    expect: { shows: [S.shell], hides: [], copy: ["No pipeline named"] },
  }),
  probe({
    id: "probe--no-credential-registry",
    rule: "field-declares-its-lifecycle",
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
  probe({
    id: "probe--unknown-primary-repo",
    rule: "fields-need-an-open-card",
    summary: "the primary repo is missing from repos.yaml, which is a different fix from read-only",
    fixture: "unknown-primary",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "wait", selector: S.reposEditor },
    ],
    expect: { shows: [S.reposEditor], hides: [], copy: ["is not in repos.yaml"] },
  }),
  probe({
    id: "probe--all-limits-capped",
    rule: "field-declares-its-lifecycle",
    summary: "every limit capped, so the summary is chips rather than the unbounded pill",
    fixture: "all-limits",
    ops: [{ op: "click", selector: S.assignmentHead }, { op: "wait", selector: S.limitsValue }],
    expect: { shows: [S.limitsValue], hides: [], copy: ["2 at once", "12 h/run"] },
  }),
  probe({
    id: "probe--no-limits-set",
    rule: "field-declares-its-lifecycle",
    summary: "no limit set at all — the summary has to say unbounded out loud",
    fixture: "no-limits",
    ops: [{ op: "click", selector: S.assignmentHead }, { op: "wait", selector: S.limitsValue }],
    expect: { shows: [S.limitsValue], hides: [], copy: ["unbounded — no limits set"] },
  }),
];
