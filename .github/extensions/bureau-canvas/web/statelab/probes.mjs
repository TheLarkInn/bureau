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

import { SELECTORS as S, cleanEditor, dirtyEditor, draftMarkIn, editorCardFor, offered, viewerCardFor, withheld } from "./selectors.mjs";
import { INFERRED_FILTER_URL, REPO_ADD_URL, SAMPLE_STEPS, runOps } from "./paths.mjs";

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
  fieldPair: "n/a",
  mode: "n/a",
  run: "n/a",
  transport: "n/a",
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
  fieldPair: "n/a",
  mode: "n/a",
  run: "n/a",
  transport: "n/a",
  tab: "pipeline",
  pick: "none",
  edit: "rest",
};

/**
 * The way to a finished concurrent group: publish the pipeline that has one,
 * then watch the run that ran it.
 *
 * Live rather than replay, and that is the whole reason `run-group` has no
 * `run_finished` event. Live backfills the entire log before it renders, so the
 * group is already done; replay parks at the first event, where no group has
 * started yet and there is nothing to fold. A group that has finished inside a
 * run that has not is an ordinary shape — `completion: all` returns and the
 * pipeline moves on — and it is the one the fold is offered on.
 */
const GROUP_RUN_OPS = [
  { op: "wait", selector: S.pipelineView },
  { op: "click", selector: S.modeLive },
  { op: "present", selector: `${S.runPickerLive} option[value="run-group"]` },
  { op: "select", selector: S.runPickerLive, value: "run-group" },
  { op: "wait", selector: S.groupMembers },
];

/**
 * A route belongs on the state as well as on the op that installs it.
 *
 * The op is what the walkers read, so a probe riding a route always *worked* —
 * and every consumer that asks the state itself saw `undefined`. Three probes
 * were published as "a hand-assembled crossing" when the fact that actually
 * keeps them unreachable is the route; `abort-intent` is asked for by probes
 * alone, so the test that checks every requested kind is one this module names
 * never saw it; and the lab's refusal to render a route it cannot install read
 * `undefined` for all 29 probes and so could never refuse. That last one is
 * latent rather than wrong today — both kinds in play are servable in-frame —
 * which is exactly why it had to be closed before a probe asks for one that is
 * not, and the lab draws a screen it did not produce.
 */
function state({ id, summary, page = "index", surface, fixture, ops, expect, intercept, ...rest }) {
  return {
    id,
    kind: "probe",
    summary,
    page,
    fixture,
    surface: surface ?? (page === "editor" ? "editor" : "config"),
    intercept: intercept ?? null,
    ops: [{ op: "page", value: page, ...(intercept ? { intercept } : {}) }, { op: "fixture", value: fixture }, ...ops],
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
    dimensions: { draft: "pending", card: "expanded", field: "limits", fieldState: "rest", fieldPair: "none" },
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
    dimensions: { data: "invalid", card: "expanded", field: "repos", fieldState: "rest", fieldPair: "none" },
    summary: "a findings strip above a repo editor that draws its own notes",
    fixture: "invalid",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "wait", selector: S.reposEditor },
    ],
    /*
     * The strip is the half of this crossing the rule is actually about. Every
     * matrix tuple that would put a findings strip over an open field editor is
     * pruned by `chrome-is-orthogonal-to-body`, so this is the one state in the
     * registry that can see the pair — and asserting only the editor left the
     * chrome unobserved at the single point that exists to observe it. Gating
     * `GeneralFindings` on "no disclosure open" would have passed.
     */
    expect: { shows: [S.reposEditor, S.reposSave, ".general-findings"], hides: [], copy: ["Validation findings"] },
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
  /*
   * The create form with its request in flight.
   *
   * Every other write on this surface models both ends of its round trip; this
   * one modelled neither, and the renderer matched the model — `submit` posted
   * with the button live and applied whatever came back. So a second press sent
   * a second create, and a Cancel pressed while the first was still out closed
   * the form and then had the answer installed over whatever the reader opened
   * next. Both are the defect the field editors and the delete prompt were
   * already fixed for, on the one write nothing had looked at.
   *
   * A sample rather than a value on `fieldState`: the create bar is a landing
   * disclosure, not a field on a card, so the `field` axis cannot name it and a
   * new axis value would pair with nothing else.
   *
   * The held name input is asserted beside the held button because they fail
   * apart: the button alone comes back true the moment `busy` reaches it, while
   * a form that kept taking keystrokes would still throw them away — it posts
   * the name it captured when Create was pressed.
   */
  sample({
    id: "probe--create-saving",
    covers: "the create form while its request is in flight — the one write on this surface with no in-flight state at all",
    summary: "a create in flight: the form holds its own controls so the write cannot be sent twice, and names the verb that is running",
    fixture: "validated",
    intercept: "stall-intent",
    ops: [
      { op: "click", selector: S.createOpen },
      { op: "wait", selector: S.createBar },
      { op: "fill", selector: S.createName, value: "second-pipeline" },
      { op: "click", selector: S.createSubmit },
      { op: "wait", selector: withheld(S.createSubmit) },
    ],
    expect: {
      // Cancel stays offered for the same reason it does in every field editor:
      // a host that never answers must not trap the reader on the form. What
      // makes it safe here is the ticket, not a disabled attribute.
      shows: [S.createBar, withheld(S.createSubmit), withheld(S.createName), offered(S.createCancel)],
      hides: [],
      copy: ["Creating…"],
    },
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
  crossing({
    id: "probe--two-disclosures-open",
    rule: "a-second-open-field-is-probed-not-crossed",
    dimensions: { card: "expanded", field: "repos", fieldState: "rest", fieldPair: "second-open" },
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
  /*
   * The concurrent group, which the dimensions cannot reach.
   *
   * A group card is drawn by the run viewer for a `concurrent` step, and the
   * bundled sample has none — so no combination of the sixteen axes produces
   * one, and the whole family (the member rows, the outcome per member, and the
   * control that folds them away) was rendered by nothing. That is the one
   * thing this registry may not do, and a rule could not fix it: an axis whose
   * every value is excluded has nowhere for a `harness` rule to stand.
   *
   * So it is a payload the dimensions do not model, which is what a content
   * sample is for. `concurrent-run` carries a pipeline the *host* laid out and
   * `run-group` is a committed log in which the group has finished while the
   * run carries on — which is what makes the fold offered, since `groupHidden`
   * honours a collapse only once a group is done.
   *
   * The pair is deliberate rather than one state with two assertions. They
   * differ by exactly one click, so the registry derives an edge between them,
   * and `REVERSIBLE` gives the way back. That is the defect this control
   * already had once: collapsing used to remove the members and the only button
   * that could restore them in the same click, and nothing here could have
   * failed. Now the door has to open both ways or the transition fails by name.
   */
  sample({
    id: "probe--group-expanded",
    covers: "a finished concurrent group in a run, listing an outcome per member — a card the sixteen axes cannot produce, because the bundled sample has no concurrent step",
    summary: "a finished concurrent group with its members listed, each carrying its own outcome",
    surface: "pipeline",
    fixture: "concurrent-run",
    ops: [...GROUP_RUN_OPS],
    expect: {
      shows: [S.groupCard, S.groupMembers, S.groupMemberRow, S.groupFoldOpen, S.groupMemberCard],
      hides: [],
      // The members disagreed, and the card says so per member rather than
      // reporting only the group's own verdict.
      copy: ["read-diff", "read-tests", "success", "failure"],
    },
  }),
  sample({
    id: "probe--group-collapsed",
    covers: "the same group folded away, and the control that folds it surviving its own collapse",
    summary: "the group folded shut — the members are gone and the control that brings them back is not",
    surface: "pipeline",
    fixture: "concurrent-run",
    ops: [
      ...GROUP_RUN_OPS,
      { op: "click", selector: S.groupFold },
      { op: "waitGone", selector: S.groupMembers },
    ],
    expect: {
      shows: [S.groupCard, S.groupFoldShut],
      // Both halves of the fold: the rows go from the card and the member
      // cards go from the canvas. Asserting only the rows would pass on a
      // collapse that left two orphaned cards behind.
      hides: [S.groupMembers, S.groupMemberRow, S.groupMemberCard],
      copy: ["run-checks"],
    },
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
    id: "probe--work-source-inferred-filter",
    covers: "the third answer a paste can get: a derivation Bureau had to infer, which is offered but must say so",
    summary: "a URL with no search query — the filter is inferred, the save is still offered, and the note says which",
    fixture: "validated",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.workSourceValue },
      { op: "fill", selector: S.workSourceUrl, value: INFERRED_FILTER_URL },
      { op: "wait", selector: S.workSourceInferred },
    ],
    /*
     * `exact: false` is neither of the lifecycle's two answers: the save is
     * offered like `dirty` and the copy warns like `invalid`. Asserting the
     * exact-derivation note *absent* is the half that matters — an inferred
     * filter presented as an exact one is a filter that silently means
     * something else, which is the hazard the deriver exists to prevent.
     */
    expect: {
      shows: [S.workSourceDerived, S.workSourceInferred, offered(S.workSourceSave)],
      hides: [S.workSourceExact],
      copy: ["built from the label in the URL"],
    },
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
  /*
   * The third end of a save, on each surface that has one.
   *
   * `saving` and `save-error` are the two the `draft` and `edit` axes model: a
   * request in flight, and one the host answered with a refusal. A request that
   * never gets answered *at all* is a third outcome, and it was the one screen
   * on this surface that no rule excluded and no state rendered — so it went
   * unasserted while rendering as a permanent "Working…" with both controls
   * disabled and nothing said. A stuck draft bar is indistinguishable from a
   * slow one in a screenshot, which is why this had to be closed in the
   * renderer and then pinned here rather than found by looking.
   *
   * They are samples rather than a fourth value on `draft` and `edit`: the fix
   * makes a dead transport land in the same rendered class as a refusal, and
   * two routes that produce one screen are one equivalence class, not two. What
   * is worth asserting is exactly that convergence — the refusal is drawn, and
   * the controls come back.
   */
  sample({
    id: "probe--draft-save-transport-lost",
    covers: "the draft bar's save when the request is never answered — the third end of a save, which must refuse rather than hang",
    summary: "the host went away mid-save: the draft bar says so and returns both controls instead of sitting on Working…",
    fixture: "draft-pending",
    intercept: "abort-intent",
    ops: [
      { op: "wait", selector: S.draftBar },
      { op: "click", selector: S.draftSave },
      { op: "wait", selector: S.draftRefused },
    ],
    expect: {
      shows: [S.draftBar, S.draftRefused, offered(S.draftSave), offered(S.draftDiscard)],
      hides: [],
      copy: ["could not save changes"],
      allowErrors: ["Failed to fetch", "net::ERR_FAILED", "/intent"],
    },
  }),
  sample({
    id: "probe--editor-save-transport-lost",
    covers: "the pipeline editor's save when the request is never answered — the panel already draws the error, so it has to be given one, in this surface's words rather than the browser's",
    summary: "the host went away mid-save: the editor says so in its own sentence, and keeps the draft rather than falling back to unsaved edits",
    page: "editor",
    fixture: "pipeline",
    intercept: "abort-intent",
    ops: [
      { op: "click", selector: editorCardFor(SAMPLE_STEPS.deterministic) },
      { op: "fill", selector: S.editorStepName, value: "deterministic-renamed" },
      { op: "press", selector: S.editorStepName, value: "Enter" },
      { op: "wait", selector: S.editorSave },
      { op: "click", selector: S.editorSave },
      { op: "wait", selector: S.editorSaveReverted },
    ],
    expect: {
      shows: [S.editorSaveReverted, S.editorStatusError, offered(S.editorSave), S.editorDiscard],
      hides: [],
      // The reason is asserted, not just the heading. A panel that echoed the
      // rejection's own `message` passed the heading check while printing
      // "Failed to fetch" — wording this harness supplies, that varies by
      // browser, and that says nothing about the pipeline or the draft.
      copy: ["Save reverted", "could not save this pipeline — nothing was written"],
      allowErrors: ["Failed to fetch", "net::ERR_FAILED", "/intent"],
    },
  }),
  /*
   * The one optional field on the card, and the only production control the
   * registry did not name.
   *
   * Every other input in the work-rules editor is required, and the axis drives
   * `dirty` and `invalid` through those. The approval label is the exception —
   * emptying it is legal, and `save` writes `null` for it — so it is the one
   * field where "changed" and "valid" come apart from each other. Typing into
   * it alone has to leave the editor dirty, the save offered and *no* refusal
   * drawn; a copy-pasted `form-control--invalid` on this input, or an `invalid`
   * that read all three fields, would be a form that refuses to save a value it
   * documents as optional, and nothing in the matrix would have noticed.
   */
  sample({
    id: "probe--work-rules-optional-only",
    covers: "the one work-rules field whose emptiness is legal, edited on its own",
    summary: "only the optional approval label was changed — the draft is held and offered, and nothing is called invalid",
    fixture: "validated",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.workRulesValue },
      { op: "fill", selector: '[data-testid="wr-approval"]', value: "bureau:approved" },
      { op: "wait", selector: offered(S.workRulesSave) },
    ],
    expect: {
      shows: [S.workRulesEditor, dirtyEditor(S.workRulesEditor), draftMarkIn(S.workRulesEditor), offered(S.workRulesSave)],
      hides: [`${S.workRulesEditor} .note--err`, cleanEditor(S.workRulesEditor)],
      copy: ["unsaved changes", "Approval label (optional)"],
    },
  }),
  /*
   * The repo adder with nothing left to add.
   *
   * `RepoAdder` lists the registered repos this assignment does not already
   * hold, and that list can be empty — which is not a failure and must not read
   * as one. The paste box stays, because registering a new repo is still the
   * way forward; what changes is that the pick list is replaced by a sentence
   * saying why there is nothing to pick.
   */
  sample({
    id: "probe--repos-registry-exhausted",
    covers: "the repo adder when the assignment already holds every registered repo",
    summary: "nothing left to pick — the adder says so and still offers the paste box that registers a new one",
    fixture: "multi-repo",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "click", selector: S.reposAdd },
      { op: "wait", selector: S.reposUrl },
    ],
    expect: {
      shows: [S.reposUrl, cleanEditor(S.reposEditor)],
      hides: [draftMarkIn(S.reposEditor)],
      copy: ["Every registered repo is already listed."],
    },
  }),
  /*
   * The adder holding a draft of its own.
   *
   * Its `data-dirty` is the list's edits *or* its own pasted URL, and the list
   * half is what the browser suite drives. This is the other half, rendered:
   * an untouched list under a box with a URL in it still has to read as unsaved
   * work, because "Add to registry and this assignment" is offered from it.
   *
   * The wait is on the resolved preview rather than the marker, and that is the
   * point of the state as well as its timing. The marker appears the instant a
   * character is typed, so waiting on it would let the render be judged before
   * the preview arrived — and the preview is the only place in this UI where a
   * `.detail-row` is drawn inside another `.detail-row`, which is the
   * containment `checks.mjs` had been reporting as five overlapping rows.
   */
  sample({
    id: "probe--repos-add-url-typed",
    covers: "the repo adder's own draft — a pasted URL over a list nobody edited, resolved into the nested preview",
    summary: "a URL pasted into the adder: unsaved work even though the ranked list is untouched",
    fixture: "multi-repo",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "click", selector: S.reposAdd },
      { op: "fill", selector: S.reposUrl, value: REPO_ADD_URL },
      { op: "wait", selector: S.reposPreview },
    ],
    expect: {
      shows: [S.reposUrl, S.reposPreview, dirtyEditor(S.reposEditor), draftMarkIn(S.reposEditor)],
      hides: [cleanEditor(S.reposEditor)],
      copy: ["unsaved changes", "rushstack"],
    },
  }),
  /*
   * The step log with nothing selected, and with a step that produced nothing.
   *
   * The log occupies the full width below the graph on every overlay screen, so
   * it is never not on the page — but until now no state said what it should
   * contain, and an empty region that says nothing is indistinguishable from a
   * region that failed to render. Both of these are ordinary: a reader arrives
   * at Live before picking a run, and a reader on a live run clicks a step the
   * run has not reached yet.
   *
   * The two share the `.step-log-empty` paragraph and are told apart by what
   * surrounds it, which is exactly the distinction worth asserting: the idle
   * log has no head because there is no step to name, and the unwritten one
   * has a head and no body. A log that answered "select a step" while a step
   * was selected would pass either assertion alone.
   */
  sample({
    id: "probe--step-log-idle",
    covers: "the step log before anything is selected — a region that is always drawn and had no expectation on it",
    summary: "no step selected: the log invites a selection rather than sitting blank",
    fixture: "pipeline",
    surface: "pipeline",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "wait", selector: S.stepLogIdle },
    ],
    expect: {
      shows: [S.stepLog, S.stepLogIdle],
      hides: [S.stepLogHead],
      copy: ["Select a step to see what it did."],
    },
  }),
  /*
   * A step the live run has not written to. `verify` is in the sample pipeline
   * and absent from `run-live`'s log, so it has a head and an empty body —
   * which is the honest answer, and distinct from the step having failed.
   */
  sample({
    id: "probe--step-log-unwritten",
    covers: "a selected step the run has not reached: a log head with nothing under it, said in words rather than left blank",
    summary: "a step with no captured output names itself and says so",
    fixture: "pipeline",
    surface: "pipeline",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      ...runOps("live", "running"),
      { op: "click", selector: viewerCardFor(SAMPLE_STEPS.deterministic) },
      { op: "wait", selector: S.stepLogEmpty },
    ],
    expect: {
      shows: [S.stepLog, S.stepLogHead, S.stepLogEmpty],
      hides: [S.stepLogIdle],
      copy: ["No output captured for this step yet.", SAMPLE_STEPS.deterministic],
    },
  }),
];
