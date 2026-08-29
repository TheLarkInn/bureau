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

import { SELECTORS as S, cleanEditor, dirtyEditor, draftMarkIn, editorCardFor, offered, replayPositionAt, replaySpanFor, replaySpeed, replaySpeedActive, viewerCardFor, withheld } from "./selectors.mjs";
import { INFERRED_FILTER_URL, REPO_ADD_URL, RUN_END, RUN_STEP, SAMPLE_STEPS, fixtureFor, runOps } from "./paths.mjs";
import { PASS_RUN } from "./intercept.mjs";

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
 * The payload the editor matrix states are enumerated with, taken from the same
 * function rather than spelled again.
 *
 * A crossing that publishes a different fixture is not a perturbation of a
 * state, it is a separate screen: `buildTransitions` compares whole `fixture`
 * ops, so a hand-picked payload silently detaches the probe from the DAG. Both
 * tab crossings did exactly that, and what it cost them was the return edge —
 * the only assertion able to fail if the editor drops what it was holding.
 */
const EDITOR_FIXTURE = fixtureFor(EDITOR_BASE);

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
  /*
   * The other end of a refusal: what happens to it when the reader says no.
   *
   * `error` was cleared in exactly one place — the start of the *next* request —
   * so every dismissal path left it set. Neither component is unmounted by its
   * own dismissal: `CreateBar` only hides the disclosure, and `DeleteControl`'s
   * `!preflight` branch draws the note itself. So a refused write followed by
   * Cancel put the reader back on a resting screen that still reported a failure
   * — for a request that was not in flight, and that they had already dismissed.
   *
   * These are the two states that make that fail. Both are `sample()`s rather
   * than crossings: no rule excluded them, and no axis names them, because
   * `disclosure` and `fieldState` each describe a refusal that is *present*.
   * Neither is reachable as a return edge either — Cancel here lands two screens
   * back, not on the parent the refusal was entered from, which is exactly the
   * shape `REVERSIBLE` cannot express.
   */
  sample({
    id: "probe--create-refusal-dismissed",
    covers: "a refused create that the reader cancelled, then opened again — the refusal must not come back with the form",
    summary: "the create bar reopened after a refusal was dismissed: an empty form with nothing claiming to have failed",
    fixture: "validated",
    intercept: "fail-intent",
    ops: [
      { op: "click", selector: S.createOpen },
      { op: "fill", selector: S.createName, value: "second-reviewer" },
      { op: "click", selector: S.createSubmit },
      { op: "wait", selector: S.createRefused },
      { op: "click", selector: S.createCancel },
      { op: "waitGone", selector: S.createBar },
      { op: "click", selector: S.createOpen },
      { op: "wait", selector: S.createBar },
    ],
    expect: {
      // The form is back and empty, so Create is withheld until a name is typed.
      // The refusal is the assertion that matters: it is `hides`, because the
      // bug leaves it present rather than merely stale.
      shows: [S.createBar, withheld(S.createSubmit), offered(S.createCancel)],
      hides: [S.createRefused],
      copy: [],
    },
  }),
  sample({
    id: "probe--delete-refusal-dismissed",
    covers: "a refused delete that the reader cancelled — the card must not keep reporting a removal that never happened",
    summary: "an assignment card after a refused delete was dismissed: the prompt is gone and so is its refusal",
    fixture: "validated",
    intercept: "fail-intent",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.deleteStart },
      { op: "wait", selector: S.preflight },
      { op: "click", selector: S.deleteConfirm },
      { op: "wait", selector: S.deleteRefused },
      { op: "click", selector: S.deleteCancel },
      { op: "waitGone", selector: S.preflight },
    ],
    expect: {
      shows: [S.assignmentDetail, offered(S.deleteStart)],
      // Both places the refusal could be. `deleteRefused` is scoped to the
      // prompt and would pass by default once the prompt has gone, so on its own
      // it asserts nothing here; `deleteRefusedResting` is where the surviving
      // one actually renders, and is the half that fails against the bug.
      hides: [S.deleteRefused, S.deleteRefusedResting],
      copy: [],
    },
  }),
  /*
   * The pass that worked, which is a screen and was rendered by nothing.
   *
   * The refusal and the in-flight button were both modelled; the *report* was
   * not, so `reportPass` — three distinct sentences about what a pass did — was
   * asserted by one Playwright spec and by no state, and `reconcileResult` was a
   * selector with no reference in the repository.
   *
   * "Claimed no work" is the deterministic one: `pass-intent` synthesises the
   * answer in-frame and writes nothing, so the listing the report re-reads is
   * the host's own, unchanged, and no run can have appeared since the click.
   * The other two sentences depend on a listing that changes underneath the
   * pass, which is a race rather than a state.
   */
  sample({
    id: "probe--reconcile-now-reported",
    covers: "a reconcile pass that finished and said what it did — the success half of a control whose only modelled ends were busy and refused",
    summary: "a completed reconcile pass reporting that it claimed no work, with the button returned and nothing moved",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "pass-intent",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "click", selector: S.reconcileNow },
      { op: "wait", selector: S.reconcileResult },
    ],
    expect: {
      // The report is a *result*, not an error: drawn in the success class, and
      // the error class asserted absent. A pass that reported through
      // `run-control-error` would read as a failure while saying it finished.
      shows: [S.reconcileNow, S.reconcileResult, S.runActivityAvailable],
      hides: [S.reconcileNowPending, S.runControlError, S.overlayRunning],
      copy: ["Reconcile pass finished. It claimed no work for this pipeline."],
    },
  }),
  /*
   * The other end of the same control, and the one screen on Live that no
   * resting state can draw.
   *
   * `reportPass` has three sentences; "claimed no work" was the only one under
   * test, and it is the only one that draws no button. The sentence that names
   * a run also offers **Open in Replay** — a shipped control that, until this
   * state existed, appeared in none of the renders, was named by no selector,
   * and was walked by no edge. Dropping the run argument that wires it would
   * have made it a silent no-op with nothing to fail.
   *
   * Replay rather than Live is the whole point of the hand-off: `reconcile
   * --now` drains before it returns, so the run it started has already
   * finished, and a finished run is not something the live-only picker will
   * offer.
   */
  sample({
    id: "probe--reconcile-now-started-a-run",
    covers: "the reconcile report that names a run, and the hand-off it offers — the only state in which `open-run-replay` is drawn",
    summary: "a reconcile pass that started a run and says which: the run had already finished by the time the pass returned, so the way on is Replay",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "pass-starts-run",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "click", selector: S.reconcileNow },
      { op: "wait", selector: S.openRunReplay },
    ],
    expect: {
      shows: [S.reconcileNow, S.reconcileResult, S.openRunReplay],
      // Still Live, and still a *result* rather than an error: a pass that
      // reported through `run-control-error` would read as a failure while
      // saying what it accomplished.
      hides: [S.reconcileNowPending, S.runControlError, S.replayControls],
      copy: [`Reconcile pass finished. It ran ${PASS_RUN}, which has already finished.`],
    },
  }),
  /*
   * Pressing it. The delta is one click, so the DAG carries this as an edge out
   * of the state above, and the suite walks it rather than re-entering here.
   *
   * What it asserts is the hand-off's whole claim: the reader lands in Replay,
   * on that run — `replaySpanFor` addresses the timeline by the run's own last
   * event, so a Replay surface showing some *other* run fails by selector. The
   * Live controls and the report go with the move, because leaving Live ends
   * what that visit said.
   */
  sample({
    id: "probe--replay-opened-from-a-pass",
    covers: "the cross-surface hand-off itself: the button moves the reader to Replay, on the run the pass started, without taking anything away",
    summary: "Replay, opened from a reconcile report — the run the pass started is the run the timeline spans",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "pass-starts-run",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "click", selector: S.reconcileNow },
      { op: "wait", selector: S.openRunReplay },
      { op: "click", selector: S.openRunReplay },
      { op: "wait", selector: replaySpanFor(RUN_END.finished) },
    ],
    expect: {
      shows: [S.replayControls, S.replayTimeline, replaySpanFor(RUN_END.finished)],
      // The report is speech of a visit to Live that is over, and the button
      // belongs to it. Both must go, or Replay would carry a sentence about a
      // pass the reader has already been moved off.
      hides: [S.runControls, S.openRunReplay, S.reconcileResult],
      copy: [],
    },
  }),
  /*
   * Pausing a run that is playing.
   *
   * `transport:playing` presses Play and declares `settles: false`, which is
   * what holds Play to actually advancing the run. Nothing pressed the button
   * again. So Pause was drawn by that state, named by its own selector, and
   * asserted by nothing — replacing `onPlay` with `() => setPlaying(true)`
   * left the whole matrix green.
   *
   * It cannot be a return edge, and the reason is the point: a return edge
   * holds the child to the *parent's* expectations, and pausing does not put
   * the page back. `transport:rest` is parked at the run's first event, and a
   * paused run is wherever the clock reached — stopping is not rewinding, and a
   * Pause that returned to the start would be a worse defect than one that did
   * nothing.
   *
   * The Pause claim is the label round trip in the path, and it is bounded by
   * the position: `waitGone` on the Pause spelling is *not* enough on its own,
   * which was measured rather than assumed. Replacing `onPlay` with
   * `() => setPlaying(true)` left the button offering Pause — but the interval
   * clamps at `range.end` and clears `playing` by itself, so the wait was
   * satisfied ten seconds later by the run finishing, and the probe passed on
   * the mutation it exists to catch. So the next op requires the run to be
   * *short of its end* at the moment Pause was honoured: a Pause that did
   * nothing can only have got there by playing to the end, and fails here.
   *
   * Then it plays on at 16x to the end, and that is about the *render* rather
   * than the control. Where a pause lands is the clock's: `TICK_MS` is 100 and
   * the run spans 10,000ms, so pausing immediately leaves the scrubber either
   * exactly at the start or one tick — a full percent — past it, depending on
   * how many intervals fired between two clicks. Screenshotted there, this
   * state was an undeclared twin of `probe--replay-opened-from-a-pass` on the
   * runs where no tick fired and not on the runs where one did, which is a
   * gallery claim that would have flickered rather than held. `positionRef`
   * clamps to `range.end` and clears `playing` when the run runs out, so the
   * end is the one position on this timeline that does not depend on timing at
   * all — and it is a screen nothing else in the matrix draws.
   *
   * That timing is also why the run is *stepped* before it is played. "Short of
   * its end" catches a Pause that did nothing; it does not catch a Pause that
   * rewound, which is the defect this probe exists rather than a return edge to
   * avoid asserting. Played from the start the two are indistinguishable — the
   * start is where an honest pause may legitimately be sitting — so a Pause
   * that reset the position passed. One forward step moves the run onto its
   * second event on the log's own clock, and play only ever advances from
   * there, so the first event is a position an honoured Pause cannot be at and
   * a rewinding one always is. The read is placed *before* the play-out, since
   * a rewound run reaches the same end a moment later and the end would excuse
   * it.
   */
  sample({
    id: "probe--replay-paused-after-playing",
    covers: "Pause: the only replay control whose press was drawn by a state and asserted by none",
    summary: "a replayed run paused by its own control while still short of its end, then played out to its last event",
    fixture: "pipeline",
    surface: "pipeline",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeReplay },
      ...runOps("replay", "finished"),
      // Play from a position that is not the start, so that a Pause which
      // rewound is a thing this path can tell apart from one that never moved.
      // Stepping is the deterministic half of the transport: it lands on the
      // run's second event exactly, on a clock nothing here shares.
      { op: "click", selector: S.replayStepForward },
      { op: "wait", selector: replayPositionAt(RUN_STEP.finished.next) },
      { op: "click", selector: S.replayPlay },
      { op: "wait", selector: S.replayPause },
      { op: "click", selector: S.replayPause },
      { op: "waitGone", selector: S.replayPause },
      // Stopping is not rewinding, and this is where that is read. Play only
      // ever advances, so a Pause that honoured its own meaning cannot be at
      // the run's first event — while a Pause that reset the position is
      // parked there, and is held here rather than excused by the end it would
      // still go on to reach.
      { op: "waitGone", selector: replayPositionAt(RUN_STEP.finished.start) },
      { op: "waitGone", selector: replayPositionAt(RUN_END.finished) },
      { op: "click", selector: replaySpeed(16) },
      { op: "click", selector: S.replayPlay },
      { op: "wait", selector: replayPositionAt(RUN_END.finished) },
    ],
    expect: {
      shows: [
        S.replayControls,
        S.replayTimeline,
        // The button offers Play again, by its own label rather than by the
        // testid both conditions share.
        S.replayResumed,
        S.replayStepForward,
        S.replayStepBack,
        replaySpeedActive(16),
        replayPositionAt(RUN_END.finished),
        replaySpanFor(RUN_END.finished),
      ],
      hides: [S.replayPause],
      copy: ["Play"],
    },
  }),
  /*
   * The badge before it has a number.
   *
   * Every other run-listing state is an answer — empty, failed, failed-later.
   * This is the wait, and it is the state `liveCountLoading` was declared for:
   * the selector had exactly one reference in the repository, its own
   * declaration. It matters because the badge's `data-count` is asserted by
   * every pipeline state, so the screen where that attribute is legitimately
   * absent is the one that says what the reader sees in the meantime.
   */
  sample({
    id: "probe--live-count-loading",
    covers: "the Live badge while its first run listing is still in flight",
    summary: "the pipeline toolbar before the run listing has answered: the badge reports itself as loading rather than as zero",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "stall-runs",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "wait", selector: S.liveCountLoading },
    ],
    expect: {
      shows: [S.modeSwitcher, S.liveCountLoading],
      // Neither an answer nor a zero. A badge that fell back to `0` while the
      // read was outstanding would be reporting "no runs in progress" as a fact
      // it does not have, which is the one thing this state exists to deny.
      hides: [S.liveCountSettled, S.liveCountZero, S.liveCountUnavailable],
      copy: [],
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
    // The same three regions the enumerated `disclosure:relation-open` value
    // promises, plus the expanded card this crossing is about. It used to
    // promise only the flow and the detail, which was a weaker claim than the
    // state it crosses — and `.relation-section[open]` in particular is the
    // region the summary toggle declares it reveals, so leaving it out meant
    // the way back out of this state was a claim about a region this screen
    // never mentioned.
    expect: { shows: [S.relationSection, S.relationOpen, S.relationFlow, S.assignmentDetail], hides: [], copy: [] },
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
  //
  // Both are entered from the matrix state they perturb rather than from a
  // fixture of their own, and that is what holds their claim to account. On
  // this tab the crossing is *supposed* to be indistinguishable — a selection
  // must not leak onto the relation graph — so every expectation a single
  // render can carry is equally true of the tab with nothing selected, and the
  // desktop shot is byte-identical to it. The claim that is not vacuous is what
  // happens next: `S.editorTabRelations` declares its undo, so attaching to the
  // DAG earns a return edge that presses Pipeline again and holds the result to
  // the *parent's* expectations — the selection, and the unsaved rename, still
  // there. An editor that dropped either on a tab switch fails by name.
  crossing({
    id: "probe--selection-behind-relations-tab",
    rule: "selection-needs-the-pipeline-tab",
    base: EDITOR_BASE,
    dimensions: { tab: "relations", pick: "deterministic", edit: "n/a" },
    summary: "a step selected, then Relations shown — the selection is held, and none of it may leak onto the relation graph",
    page: "editor",
    fixture: EDITOR_FIXTURE,
    ops: [
      { op: "wait", selector: S.editorTabs },
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
    fixture: EDITOR_FIXTURE,
    ops: [
      { op: "wait", selector: S.editorTabs },
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
   * The two ends of the registration itself.
   *
   * Registering writes through a control that is not a field editor's Save, so
   * neither end of it could be reached through the `fieldState` lifecycle:
   * `FIELD_SAVE` has no entry for the adder, and `field: repos-add` declares
   * the single lifecycle value `n/a`. That left "Add to registry and this
   * assignment" with a `busy` flag, an in-flight verb and a refusal that
   * nothing in the registry had ever drawn — the last of that family still
   * missing it, after the draft bar's Saving… and the delete confirmation's
   * Deleting…
   *
   * The URL resolves in both: `resolve-repo` is a `READ_INTENT`, so it reaches
   * the host under `stall-intent` and under `fail-intent` alike, and only the
   * `set-repos` that carries the registration is claimed. That is what makes
   * these two screens of the adder rather than two screens of a dead host.
   */
  sample({
    id: "probe--repos-add-registering",
    covers: "the registration in flight — the last write with no field-editor Save behind it left undrawn",
    summary: "a repo being registered: the button names the work it is doing and takes no second press, so one paste cannot write repos.yaml twice",
    fixture: "multi-repo",
    intercept: "stall-intent",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "click", selector: S.reposAdd },
      { op: "fill", selector: S.reposUrl, value: REPO_ADD_URL },
      { op: "wait", selector: S.reposPreview },
      { op: "click", selector: S.reposRegister },
      { op: "wait", selector: withheld(S.reposRegister) },
    ],
    expect: {
      shows: [S.reposUrl, S.reposPreview, withheld(S.reposRegister)],
      hides: [S.reposAddRefused],
      copy: [{ selector: S.reposRegister, text: "Adding…" }],
    },
  }),
  /*
   * The refusal, and the sentence it had to be given.
   *
   * Reordering and registering post the same `set-repos`, and `ReposEditor`
   * answered both with one fallback — "could not save those repos". A reader
   * who pressed "Add to registry and this assignment" was therefore told that
   * their ranked list had failed to save, an edit they had not made and which
   * is not what was refused. The refusal now names the action that was taken,
   * and this is the state that holds it to that.
   */
  sample({
    id: "probe--repos-add-refused",
    covers: "a registration the host refused, and the words it is refused in",
    summary: "a refused registration: the button comes back, the paste is still there to retry, and the failure names registering rather than a repo list nobody reordered",
    fixture: "multi-repo",
    intercept: "fail-intent",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.reposValue },
      { op: "click", selector: S.reposAdd },
      { op: "fill", selector: S.reposUrl, value: REPO_ADD_URL },
      { op: "wait", selector: S.reposPreview },
      { op: "click", selector: S.reposRegister },
      { op: "wait", selector: S.reposAddRefused },
    ],
    expect: {
      shows: [S.reposUrl, S.reposPreview, S.reposAddRefused, offered(S.reposRegister)],
      copy: [{ selector: S.reposAddRefused, text: "could not register rushstack" }],
    },
  }),
  /*
   * The two screens on which the canvas describes an absence.
   *
   * Every other state in this registry renders a config that has something in
   * it, so the sentences the card draws when a field is unset — `no source`,
   * `no pipeline`, `no filter`, `no approval label`, `branches: not set`,
   * `no repos`, and the dash `PipelineLink` draws instead of a door — were
   * rendered by nothing and asserted by nothing. Deleting any of them and
   * letting the glance read `undefined · undefined · 0 repos · 0 limits` would
   * have passed all 502 renders, and that is the screen a reader lands on
   * immediately after writing an `assignments/*.yaml` and before filling it in.
   *
   * The glance is asserted as the *whole* line rather than as substrings, and
   * that is the point of scoping it: a card that dropped one fallback and kept
   * the other three would satisfy every substring it still drew.
   */
  sample({
    id: "probe--bare-assignment",
    covers: "an assignment that names nothing yet — the one screen whose glance line is built entirely from fallbacks",
    summary: "a newly written assignment beside a configured one: the glance says what is missing rather than reading blank",
    fixture: "bare-assignment",
    ops: [{ op: "wait", selector: S.bareGlance }],
    expect: {
      shows: [S.assignmentCardSecond, S.bareHead],
      hides: [S.bareDetail],
      copy: [{ selector: S.bareGlance, text: "no source · no pipeline · 0 repos · 0 limits" }],
    },
  }),
  sample({
    id: "probe--bare-assignment-expanded",
    covers: "the unset fields inside the card — the chips and rows that have to say what is missing",
    summary: "the unconfigured card opened: every row names what it has not been given, and it offers no door into a pipeline that does not exist",
    fixture: "bare-assignment",
    ops: [
      { op: "click", selector: S.bareHead },
      { op: "wait", selector: S.bareDetail },
    ],
    expect: {
      shows: [S.bareDetail, S.bareRepos, S.bareWorkSource, S.barePipelineUnset],
      copy: [
        "no filter",
        "no approval label",
        "branches: not set",
        // The row that used to answer with punctuation. Compared exactly and
        // scoped, so a return to `? · ?` fails here by name rather than being
        // absorbed by a substring of the card's own body.
        { selector: S.bareWorkSource, text: "no work source" },
        { selector: S.bareRepos, text: "no repos" },
        { selector: S.barePipelineUnset, text: "—" },
      ],
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
   *
   * The step's name is asserted *in the log's own heading*, not as a word
   * somewhere on the page. The path reaches this state by clicking the graph
   * node whose card renders that same name, so the unscoped form was satisfied
   * by the element the probe had just pressed: delete the heading from
   * `LogHead` and `.step-log-head` still renders — the kind pill keeps it
   * non-empty — while the log stops naming its step and every expectation here
   * stayed green. The summary claims the step "names itself", so the assertion
   * has to be about the thing doing the naming.
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
      shows: [S.stepLog, S.stepLogHead, S.stepLogTitle, S.stepLogEmpty],
      hides: [S.stepLogIdle],
      copy: ["No output captured for this step yet.", { selector: S.stepLogTitle, text: SAMPLE_STEPS.deterministic }],
    },
  }),
  sample({
    id: "probe--step-agent-agrees",
    covers: "a step whose logged agent identity is still the one the config selects",
    summary: "the run names the qualified agent its role selected, and the config still selects it, so nothing is flagged",
    fixture: "pipeline",
    surface: "pipeline",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      ...runOps("live", "running"),
      { op: "wait", selector: S.stepAgent },
    ],
    expect: {
      shows: [S.stepAgent],
      hides: [S.stepAgentMismatch],
      copy: [{ selector: S.stepAgent, text: "agent bureau:implementer" }],
    },
  }),
  /*
   * The disagreement, and what it can honestly be about.
   *
   * Both names are projections of config — the log holds what the role selected
   * when the run started, `validate --json` holds what it selects now — so the
   * only thing that can put them apart is the config moving underneath a
   * finished run. `run-paused` is that shape: a `/bureau:implementer` reference
   * logged through the `claude` adapter, which takes the bare agent name, read
   * against a config whose role is `copilot` and keeps the qualifier.
   *
   * It is deliberately *not* "the spawn used the wrong agent". Nothing on this
   * path observes a spawn: the run log is written before the worktree guard has
   * captured anything, so it may only use the pure identity. The copy said
   * "invoked", which claimed an observation that was never made, on a
   * comparison that against an unchanged config cannot fail.
   */
  sample({
    id: "probe--step-agent-mismatch",
    covers: "a finished run read against a config that has since changed which agent the role selects",
    summary: "the run names the agent it used and the one the config now selects, rather than silently showing whichever it had",
    fixture: "pipeline",
    surface: "pipeline",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      ...runOps("live", "paused"),
      { op: "wait", selector: S.stepAgentMismatch },
    ],
    expect: {
      shows: [S.stepAgent, S.stepAgentMismatch],
      copy: [{
        selector: S.stepAgentMismatch,
        text: "this run used implementer; the config now selects bureau:implementer",
      }],
    },
  }),
  sample({
    id: "probe--run-activity-idle",
    covers: "an honest zero-run listing, distinct from a stopped or unreadable reconciler",
    summary: "no run is writing to this pipeline: Live explains that a reconcile process is not itself a run",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "empty-runs",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "wait", selector: S.runActivityIdle },
    ],
    expect: {
      // The badge at its honest zero. The two states that deny it —
      // `probe--live-badge-pending` and `probe--live-listing-refused` — both
      // assert `liveCountZero` absent, and neither meant anything while no
      // state required it present: a badge hard-wired to "0" would have
      // satisfied this screen and been caught only by the two that are about
      // *not* being able to say zero.
      shows: [S.reconcileNow, S.runPickerLiveDisabled, S.runActivityIdle, S.liveCountZero],
      hides: [S.runActivityAvailable, S.runActivityUnavailable],
      copy: [{ selector: S.runActivityTitle, text: "No runs in progress" }, "A reconcile loop is not itself a run."],
    },
  }),
  /*
   * The delete preflight refused, which is the read that has to fail before
   * `DeleteControl` can draw a note beside an intact Delete.
   *
   * `probe--delete-refusal-dismissed` above asserts that note *gone* after a
   * refused confirm is cancelled, and that was the only state in the registry
   * naming it — so the assertion held against a control that had been deleted
   * from the product entirely. This is the screen where it belongs: the host
   * could not say what deleting would break, so nothing is removed, nothing is
   * asked, and the card says so with the Delete still offered.
   *
   * It needs its own route because the preflight is deliberately not a write:
   * `reachesHost` passes it through so the prompt has referrers to draw, which
   * is exactly why `fail-intent` cannot refuse it.
   */
  sample({
    id: "probe--delete-preflight-refused",
    covers: "the delete preflight itself failing — the read that has to answer before a confirmation can be asked for",
    summary: "the host could not report what deleting this assignment would break: nothing is asked, nothing is removed, and Delete is still offered",
    fixture: "validated",
    intercept: "refuse-preflight",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.deleteStart },
      { op: "wait", selector: S.deleteRefusedResting },
    ],
    expect: {
      // The refusal sits with an intact Delete and no prompt. `preflight`
      // absent is the half that matters: a card that drew the confirmation
      // anyway would be offering to remove something on the strength of a
      // referrer report that never arrived.
      shows: [S.assignmentDetail, S.deleteRefusedResting, offered(S.deleteStart)],
      hides: [S.preflight, S.deleteRefused],
      copy: [{ selector: S.deleteRefusedResting, text: "could not inspect agent-eligible" }],
    },
  }),
  /*
   * The same read, still outstanding.
   *
   * Pressing Delete is a question to the host before it is a prompt, and this
   * is the screen between the press and the answer: the button names the work
   * it is doing and stops accepting presses, which is the whole of what stops
   * three preflights being queued against one card. `DeleteControl` has four
   * screens — asking, asked, removing, refused — and the registry drew three.
   *
   * The prompt is asserted absent for the same reason the refusal asserts it
   * absent: a card that drew a confirmation before the referrer report arrived
   * would be offering to remove something on the strength of an answer it does
   * not have.
   */
  sample({
    id: "probe--delete-preflight-checking",
    covers: "the delete preflight in flight — the round trip a confirmation is asked for through",
    summary: "Delete pressed and the host not yet answered: the button says what it is doing and refuses a second press, so one card cannot queue three preflights",
    fixture: "validated",
    intercept: "stall-preflight",
    ops: [
      { op: "click", selector: S.assignmentHead },
      { op: "click", selector: S.deleteStart },
      { op: "wait", selector: withheld(S.deleteStart) },
    ],
    expect: {
      shows: [S.assignmentDetail, withheld(S.deleteStart)],
      hides: [S.preflight, S.deleteRefusedResting],
      copy: [{ selector: S.deleteStart, text: "Checking…" }],
    },
  }),
  sample({
    id: "probe--reconcile-now-running",
    covers: "the immediate reconcile pass while its process is still running",
    summary: "a reconcile pass in flight: its button is disabled and names the work so a second pass cannot be started",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "stall-intent",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "click", selector: S.reconcileNow },
      { op: "wait", selector: S.reconcileNowPending },
    ],
    expect: {
      shows: [S.reconcileNowPending],
      copy: [{ selector: S.reconcileNow, text: "Reconciling…" }],
    },
  }),
  sample({
    id: "probe--reconcile-now-refused",
    covers: "an immediate reconcile pass the host could not start",
    summary: "a refused reconcile pass: the button returns, the failure is named, and nothing the reader was looking at has moved",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "fail-intent",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "click", selector: S.reconcileNow },
      { op: "wait", selector: S.runControlError },
    ],
    expect: {
      // The activity panel is asserted still standing. A refused pass used to
      // select a live run anyway, which drew that run's overlay beside the
      // sentence saying the pass could not be started — two claims about one
      // click that cannot both be true, and neither of which this state's
      // controls-and-copy alone would have caught.
      shows: [S.reconcileNow, S.runControlError, S.runActivityAvailable],
      hides: [S.reconcileNowPending, S.overlayPaused, S.overlayRunning],
      copy: ["Could not run reconcile now."],
    },
  }),
  /*
   * The run transport's refusal, walked back out.
   *
   * Every other modelled run refusal enters and stops, so the registry only
   * ever asserted that the sentence is *present*. That made the transport the
   * one refusal on this canvas with no exit: the create bar clears its error
   * on Cancel and the delete prompt clears its own, each with a probe, and the
   * run controls had neither. Deselecting the run withdrew the buttons and
   * left the red sentence beside a picker naming no run — a failure attributed
   * to a request that no longer has a subject, and that nothing can retry.
   */
  sample({
    id: "probe--run-refusal-dismissed",
    covers: "a refused run control whose run was then deselected — the refusal must leave with the run it was about",
    summary: "the Live surface after a refused pause and a deselect: no run chosen, no transport, and nothing claiming to have failed",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "fail-intent",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      ...runOps("live", "running"),
      { op: "click", selector: S.runPause },
      { op: "wait", selector: S.runControlError },
      { op: "select", selector: S.runPickerLive, value: "" },
      { op: "waitGone", selector: S.runControlError },
    ],
    expect: {
      // The refusal is `hides` because the bug leaves it present, not stale.
      // The transport is asserted gone too: a Pause still drawn beside no
      // selected run would be a second way to make the same claim.
      shows: [S.runControls, S.runPickerLive, S.reconcileNow],
      hides: [S.runControlError, S.runPause, S.runCancel, S.overlayRunning],
      copy: [],
      // Leaving a run closes its event stream, and the browser reports the
      // `./events` request it aborted. That abort *is* this state — a deselect
      // that left the stream open would be the defect — so it is declared here
      // rather than read as an unrelated failure.
      allowErrors: ["/events", "net::ERR_ABORTED"],
    },
  }),
  /*
   * The one screen where this surface can contradict itself.
   *
   * A run is selected from a listing that answered; the listing then fails. The
   * overlay keeps streaming from `./runs/<id>/events`, which is a different
   * endpoint and still works — so the panel must not claim the run log cannot
   * be read while that run''s events are visibly arriving, and the picker must
   * not go dead over a run the reader is watching.
   *
   * Both were wrong: the copy said "run log", and `RunPicker` disabled itself
   * whenever the listing was not ready, stranding the reader on a run they
   * could neither leave nor see listed. Nothing modelled this combination, so
   * nothing failed.
   */
  sample({
    id: "probe--run-under-failed-listing",
    covers: "a run being watched while the listing that offered it has failed - the two halves of Live disagreeing",
    summary: "the run streams on, the picker still names it, and the failure is scoped to the list rather than to the log",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "fail-runs-later",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      ...runOps("live", "running"),
      { op: "wait", selector: S.runActivityUnavailable },
    ],
    expect: {
      shows: [S.overlayRunning, S.runActivityUnavailable, S.runPickerLive],
      hides: [S.runPickerLiveDisabled],
      copy: [
        { selector: S.runActivityTitle, text: "Run list unavailable" },
        "a run already open keeps streaming",
      ],
      // The listing, by its own address, and anchored there. Unanchored it
      // would also cover `./runs/<id>/events` — the endpoint this state exists
      // to prove is still working, whose failure it must not be able to excuse.
      allowErrors: ["/runs$"],
    },
  }),
  sample({
    id: "probe--run-activity-unavailable",
    covers: "a run listing failure, distinct from the honest zero-runs state",
    summary: "the run log cannot be read: Live says so and promises its automatic retry rather than reporting zero",
    fixture: "pipeline",
    surface: "pipeline",
    intercept: "fail-runs",
    ops: [
      { op: "wait", selector: S.pipelineView },
      { op: "click", selector: S.modeLive },
      { op: "wait", selector: S.runActivityUnavailable },
    ],
    expect: {
      // The badge is asserted at its failed value, not merely present. Reading
      // a refused listing as `0` is the exact confusion this state exists to
      // rule out: it would draw the same chrome as an idle reconciler and tell
      // a reviewer that nothing is running when nothing is known.
      shows: [S.liveCountUnavailable, S.runPickerLiveDisabled, S.runActivityUnavailable],
      hides: [S.runActivityIdle, S.liveCountZero],
      copy: [{ selector: S.runActivityTitle, text: "Run list unavailable" }, "a run already open keeps streaming"],
      // The refused request is the state. The browser logs the 503 itself, and
      // a state that stages a failing response has to own the console line it
      // causes — the way the blocked-renderer states do. By the address it was
      // for: the sentence that row carries names no endpoint, so allowing it by
      // text would have excused every other resource that failed here too.
      allowErrors: ["/runs$"],
    },
  }),
];
