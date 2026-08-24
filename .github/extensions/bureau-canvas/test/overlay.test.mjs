// Offline tests for the run-event overlay reducer (web/live/overlay.js).
// Each fixture is an event sequence shaped exactly like the run log the
// server parses (crates/bureau runlog): seq + at_ms + kind + data. No
// network, no DOM, no React — the same reducer live and replay both use.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  applyEvent,
  applyEvents,
  emptyOverlay,
  mergeRunEvents,
  newRunSince,
  reconcileReason,
  resolveOverlay,
  runActions,
  runsForPipeline,
  runsOffered,
  unattributedRuns,
  stateUpTo,
} from "../web/live/overlay.js";

let seq = 0;
function at(atMs, kind, data) {
  seq += 1;
  return { seq, at_ms: atMs, kind, data };
}
function resetSeq() {
  seq = 0;
}

const CHAIN = () => [
  at(1000, "run_started", { run_id: "r1", assignment: "fix" }),
  at(1100, "step_started", { step: "propose" }),
  at(1400, "step_finished", { step: "propose", outcome: "success" }),
  at(1500, "step_started", { step: "verify" }),
  at(1900, "step_finished", { step: "verify", outcome: "success" }),
  at(2000, "run_finished", { outcome: "success" }),
];

test("simple step chain: running, transitions, and completion", () => {
  resetSeq();
  const events = CHAIN();
  const mid = applyEvents(events.slice(0, 2));
  const done = applyEvents(events);

  assert.deepEqual(
    {
      midStatus: mid.status,
      midCurrent: mid.current,
      midPending: mid.steps.verify ?? null,
      doneStatus: done.status,
      doneCurrent: done.current,
      propose: { state: done.steps.propose?.state, outcome: done.steps.propose?.outcome },
      verify: { state: done.steps.verify?.state, outcome: done.steps.verify?.outcome },
      transitions: done.transitions,
    },
    {
      midStatus: "running",
      midCurrent: "propose",
      midPending: null,
      doneStatus: "finished",
      doneCurrent: null,
      propose: { state: "completed", outcome: "success" },
      verify: { state: "completed", outcome: "success" },
      transitions: [
        { from: null, to: "propose", outcome: null },
        { from: "propose", to: "verify", outcome: "success" },
      ],
    },
  );
});

test("failure edge: the animated transition names the failing outcome", () => {
  resetSeq();
  const events = [
    at(1000, "run_started", { run_id: "r2", assignment: "fix" }),
    at(1100, "step_started", { step: "verify" }),
    at(1500, "step_finished", { step: "verify", outcome: "failure" }),
    at(1600, "step_started", { step: "propose" }),
  ];
  const overlay = applyEvents(events);
  const pipeline = {
    layout: {
      steps: [
        { id: "propose", name: "propose", kind: "agent", fields: {} },
        { id: "verify", name: "verify", kind: "deterministic", fields: {} },
      ],
      terminals: [],
      edges: [
        { id: "verify->propose:failure", source: "verify", target: "propose", relation: "control", outcome: "failure", route: "back" },
        { id: "propose->verify:success", source: "propose", target: "verify", relation: "control", outcome: "success", route: "spine" },
      ],
    },
  };
  const resolved = resolveOverlay(pipeline, overlay);

  assert.deepEqual(
    {
      verifyOutcome: overlay.steps.verify.outcome,
      current: overlay.current,
      animated: [...resolved.animatedEdges],
      verifyClass: resolved.nodes.find((node) => node.id === "verify").className,
      proposeClass: resolved.nodes.find((node) => node.id === "propose").className,
    },
    {
      verifyOutcome: "failure",
      current: "propose",
      animated: ["verify->propose:failure"],
      verifyClass: "overlay-failure",
      proposeClass: "overlay-running",
    },
  );
});

test("concurrent group: fan-out on start, member outcomes, collapse after finish", () => {
  resetSeq();
  const events = [
    at(1000, "run_started", { run_id: "r3", assignment: "fix" }),
    at(1100, "step_started", { step: "propose" }),
    at(1400, "step_finished", { step: "propose", outcome: "success" }),
    at(1500, "step_started", { step: "run-checks" }),
    at(1500, "group_started", { group: "run-checks", members: ["apply", "review"], completion: "all", max_concurrent: 2, snapshot: "abc" }),
    at(1600, "group_member_started", { group: "run-checks", member: "apply", attempt: 1 }),
    at(1650, "group_member_started", { group: "run-checks", member: "review", attempt: 1 }),
    at(2000, "group_member_finished", { group: "run-checks", member: "apply", result: { outcome: "success" }, usage: {}, halted: false }),
    at(2100, "group_member_finished", { group: "run-checks", member: "review", result: { outcome: "failure" }, usage: {}, halted: false }),
    at(2200, "group_finished", { group: "run-checks", result: { outcome: "failure" }, usage: {}, halted: false }),
    at(2300, "step_finished", { step: "run-checks", outcome: "failure" }),
  ];
  const started = applyEvents(events.slice(0, 5));
  const finished = applyEvents(events);
  const pipeline = {
    layout: {
      steps: [
        { id: "propose", name: "propose", kind: "agent", fields: {} },
        { id: "run-checks", name: "run-checks", kind: "concurrent", fields: { members: ["apply", "review"] } },
        { id: "apply", name: "apply", kind: "agent", parentId: "run-checks", fields: {} },
        { id: "review", name: "review", kind: "agent", parentId: "run-checks", fields: {} },
      ],
      terminals: [],
      edges: [
        { id: "propose->run-checks:success", source: "propose", target: "run-checks", relation: "control", outcome: "success", route: "spine" },
      ],
    },
  };
  const beforeStart = resolveOverlay(pipeline, applyEvents(events.slice(0, 4)));
  const duringRun = resolveOverlay(pipeline, started);
  const afterFinish = resolveOverlay(pipeline, finished);
  const collapsed = resolveOverlay(pipeline, finished, { collapsed: new Set(["run-checks"]) });

  assert.deepEqual(
    {
      beforeStart: beforeStart.nodes.map((node) => node.id),
      duringRun: duringRun.nodes.map((node) => node.id),
      duringExpanded: [...duringRun.expandedGroups],
      applyOutcome: finished.groups["run-checks"].members.apply.outcome,
      reviewOutcome: finished.groups["run-checks"].members.review.outcome,
      finishedGroup: finished.groups["run-checks"].state,
      afterFinish: afterFinish.nodes.map((node) => node.id),
      collapsedNodes: collapsed.nodes.map((node) => node.id),
      collapsedExpanded: [...collapsed.expandedGroups],
      // A running group's fold does nothing, so it is not offered one; a
      // finished group keeps its control after collapsing, which is the only
      // way back to its members.
      duringFoldable: [...duringRun.foldableGroups],
      afterFinishFoldable: [...afterFinish.foldableGroups],
      collapsedFoldable: [...collapsed.foldableGroups],
    },
    {
      beforeStart: ["propose", "run-checks"],
      duringRun: ["propose", "run-checks", "apply", "review"],
      duringExpanded: ["run-checks"],
      applyOutcome: "success",
      reviewOutcome: "failure",
      finishedGroup: "finished",
      afterFinish: ["propose", "run-checks", "apply", "review"],
      collapsedNodes: ["propose", "run-checks"],
      collapsedExpanded: [],
      duringFoldable: [],
      afterFinishFoldable: ["run-checks"],
      collapsedFoldable: ["run-checks"],
    },
  );
});

test("paused run: the boundary output line sets the paused status and badge", () => {
  resetSeq();
  const events = [
    at(1000, "run_started", { run_id: "r4", assignment: "fix" }),
    at(1100, "step_started", { step: "propose" }),
    at(1400, "step_finished", { step: "propose", outcome: "success" }),
    at(1500, "output", { step: null, stream: "run", data: "run paused at a step boundary; remove the PAUSE marker and resume" }),
  ];
  const overlay = applyEvents(events);
  const pipeline = {
    layout: {
      steps: [{ id: "propose", name: "propose", kind: "agent", fields: {} }],
      terminals: [],
      edges: [],
    },
  };
  const resolved = resolveOverlay(pipeline, overlay);

  assert.deepEqual(
    {
      status: overlay.status,
      current: overlay.current,
      pausedBadge: resolved.nodes.find((node) => node.id === "propose").paused,
      finished: overlay.steps.propose.state,
    },
    { status: "paused", current: null, pausedBadge: true, finished: "completed" },
  );
});

test("replay scrubbing: stateUpTo applies exactly the prefix at or before T", () => {
  resetSeq();
  const events = CHAIN();
  const at1300 = stateUpTo(events, 1300);
  const at1500 = stateUpTo(events, 1500);

  assert.deepEqual(
    {
      at1300: { current: at1300.current, proposeState: at1300.steps.propose?.state, verify: at1300.steps.verify ?? null },
      at1500: { current: at1500.current, proposeOutcome: at1500.steps.propose?.outcome, verifyState: at1500.steps.verify?.state },
    },
    {
      at1300: { current: "propose", proposeState: "running", verify: null },
      at1500: { current: "verify", proposeOutcome: "success", verifyState: "running" },
    },
  );
});

test("concurrent group: expanded members resolve to their own outcome classes", () => {
  resetSeq();
  const events = [
    at(1000, "run_started", { run_id: "r5", assignment: "fix" }),
    at(1100, "group_started", { group: "checks", members: ["unit", "lint"], completion: "all", max_concurrent: 2, snapshot: "s" }),
    at(1200, "group_member_finished", { group: "checks", member: "unit", result: { outcome: "success" }, usage: {}, halted: false }),
    at(1300, "group_member_finished", { group: "checks", member: "lint", result: { outcome: "no-work" }, usage: {}, halted: false }),
    at(1400, "group_finished", { group: "checks", result: { outcome: "success" }, usage: {}, halted: false }),
  ];
  const overlay = applyEvents(events);
  const pipeline = {
    layout: {
      steps: [
        { id: "checks", name: "checks", kind: "concurrent", fields: { members: ["unit", "lint"] } },
        { id: "unit", name: "unit", kind: "deterministic", parentId: "checks", fields: {} },
        { id: "lint", name: "lint", kind: "deterministic", parentId: "checks", fields: {} },
      ],
      terminals: [],
      edges: [],
    },
  };
  const resolved = resolveOverlay(pipeline, overlay);

  assert.deepEqual(
    resolved.nodes.map((node) => [node.id, node.className]),
    [["checks", "overlay-success"], ["unit", "overlay-success"], ["lint", "overlay-no-work"]],
  );
});

test("applyEvent is pure: unknown kinds and group events for other runs leave state untouched", () => {
  resetSeq();
  const base = applyEvents(CHAIN().slice(0, 2));
  const unknown = applyEvent(base, { seq: 99, at_ms: 9999, kind: "checkpoint", data: { step: "propose", base_commit: "a", commit: "b" } });
  const strayMember = applyEvent(base, at(9999, "group_member_started", { group: "nope", member: "nope", attempt: 1 }));

  assert.deepEqual(
    { unknownSame: unknown === base, unknownSeq: unknown.lastSeq, straySame: strayMember.groups === base.groups },
    { unknownSame: true, unknownSeq: base.lastSeq, straySame: true },
  );
});

test("a run control is offered only while the run can still be acted on", () => {
  // Branching on `paused` alone offered Pause on a run that had already
  // reached its terminal — a control whose only possible outcome is a refusal.
  // The screen it produces is `run: ended` in the state matrix, which renders
  // the withdrawal; this is the same claim without a browser.
  assert.deepEqual(
    ["idle", "running", "paused", "finished"].map((status) => runActions(status)),
    [
      { transport: "pause", cancel: true },
      { transport: "pause", cancel: true },
      { transport: "resume", cancel: true },
      { transport: null, cancel: false },
    ],
  );
});

test("the run being watched stays listed after its log stops being live", () => {
  // A run is live exactly while its log holds no `run_finished` event, so one
  // that ends under the reader leaves the live listing on the next poll. A
  // `<select>` whose value matches no option draws blank while its overlay is
  // still on screen, so the watched run is offered whatever the filter says —
  // and only that one, so the live tab does not quietly become the replay tab.
  const runs = [
    { run_id: "run-live", live: true },
    { run_id: "run-finished", live: false },
    { run_id: "run-other", live: false },
  ];
  const ids = (options) => runsOffered(runs, options).map((run) => run.run_id);

  assert.deepEqual(
    {
      liveOnly: ids({ liveOnly: true, watching: null }),
      watchingEnded: ids({ liveOnly: true, watching: "run-finished" }),
      watchingLive: ids({ liveOnly: true, watching: "run-live" }),
      replay: ids({ liveOnly: false, watching: null }),
    },
    {
      liveOnly: ["run-live"],
      watchingEnded: ["run-live", "run-finished"],
      watchingLive: ["run-live"],
      replay: ["run-live", "run-finished", "run-other"],
    },
  );
});

test("pipeline activity excludes other drawings, and only a new run is followed", () => {
  const runs = [
    { run_id: "old", assignment: "first", pipeline: "shared", started_at: "2026-08-23T01:00:00Z", live: true },
    { run_id: "new", assignment: "second", pipeline: null, started_at: "2026-08-23T02:00:00Z", live: true },
    { run_id: "done", assignment: "first", pipeline: "shared", started_at: "2026-08-23T03:00:00Z", live: false },
    { run_id: "other", assignment: "other", pipeline: "other-pipeline", started_at: "2026-08-23T04:00:00Z", live: true },
  ];
  const assignments = [
    { name: "first", pipeline: "shared" },
    { name: "second", pipeline: "shared" },
    { name: "other", pipeline: "other-pipeline" },
  ];
  const offered = runsForPipeline(runs, "shared", assignments);

  assert.deepEqual(
    {
      offered: offered.map((run) => run.run_id),
      // A pass that started nothing must name nothing, even with live runs on
      // screen: every run here was already known when the pass began.
      startedNothing: newRunSince(offered, new Set(["old", "new", "done"])),
      startedOne: newRunSince(offered, new Set(["old", "new"]))?.run_id,
    },
    { offered: ["old", "new", "done"], startedNothing: null, startedOne: "done" },
  );
});

/*
 * The two ways a run can fail to be this click's doing.
 *
 * `reconcile --now` drains before it returns, so the pass can be open for
 * minutes. A background reconciler's run that lands in that window is absent
 * from the runs known at the click and would otherwise be reported as the run
 * the pass started. Being newer, it would even outrank the real one.
 */
test("only a run that is both new and later than the click is the pass's own", () => {
  const known = new Set(["before"]);
  const runs = [
    { run_id: "before", started_at: "2026-08-23T01:00:00Z", live: false },
    { run_id: "raced", started_at: "2026-08-23T01:30:00Z", live: true },
    { run_id: "ours", started_at: "2026-08-23T03:00:00Z", live: false },
  ];
  const clicked = Date.parse("2026-08-23T02:00:00Z");

  assert.deepEqual(
    {
      unbounded: newRunSince(runs, known)?.run_id,
      bounded: newRunSince(runs, known, clicked)?.run_id,
      noneAfter: newRunSince(runs, known, Date.parse("2026-08-23T04:00:00Z")),
    },
    { unbounded: "ours", bounded: "ours", noneAfter: null },
  );
});

/*
 * A run on disk that no pipeline can claim is still a run on disk.
 *
 * `run_started` names the pipeline only inside its snapshot, so a log written
 * before snapshots — or one whose assignment has since been renamed away — is
 * unattributable. Dropping those silently let Replay report "no runs recorded"
 * over a directory holding them, which is the same untruth as reading a failed
 * listing as zero.
 */
/*
 * The refusal a host gives without giving a reason.
 *
 * `runBureau` reports a non-zero exit as `output: `${stdout}${stderr}`.trim()`,
 * which is the empty string when the command failed silently. `""` is not
 * nullish, so a `??` chain selects it and the surface renders a refusal as an
 * empty paragraph — red, `role="status"`, and announcing nothing at all. The
 * distinction is invisible to every fixture that omits the key entirely, which
 * is why it is pinned here on the value rather than through a rendered state.
 */
test("a refusal with no reason still says something", () => {
  assert.deepEqual(
    [
      reconcileReason({ ok: false, error: "bureau binary not available" }),
      reconcileReason({ ok: false, output: "exit 2" }),
      reconcileReason({ ok: false, output: "" }),
      reconcileReason({ ok: false }),
      reconcileReason(undefined),
    ],
    [
      "bureau binary not available",
      "exit 2",
      "Could not run reconcile now.",
      "Could not run reconcile now.",
      "Could not run reconcile now.",
    ],
  );
});

test("runs no pipeline can claim are counted, not discarded", () => {
  const runs = [
    { run_id: "owned", assignment: "first", pipeline: "shared" },
    { run_id: "legacy", assignment: "gone", pipeline: null },
    { run_id: "other", assignment: "other", pipeline: "other-pipeline" },
  ];
  const assignments = [{ name: "first", pipeline: "shared" }, { name: "other", pipeline: "other-pipeline" }];

  assert.deepEqual(
    {
      owned: runsForPipeline(runs, "shared", assignments).map((run) => run.run_id),
      orphans: unattributedRuns(runs, assignments).map((run) => run.run_id),
    },
    { owned: ["owned"], orphans: ["legacy"] },
  );
});

test("step start retains configured and resolved agent identity", () => {
  const event = {
    seq: 1,
    at_ms: 1000,
    kind: "step_started",
    data: {
      step: "implement",
      role: "implementer",
      configured_agent: "/bureau:implementer",
      resolved_agent: "bureau:implementer",
    },
  };
  const record = applyEvent(emptyOverlay(), event).steps.implement;
  assert.deepEqual(
    [record.role, record.configuredAgent, record.resolvedAgent],
    ["implementer", "/bureau:implementer", "bureau:implementer"],
  );
});

/*
 * The backfill race, reduced to a fact about two lists.
 *
 * Live subscribes to the tail in the same tick as it asks for the history, so
 * a run writing right now can deliver its frame first. Keeping only the
 * history drops that frame for good — the tail began at end-of-file and
 * nothing replays it — and appending the history after it replays the run
 * backwards, because `run_started` puts a run that has just finished back to
 * `running`.
 *
 * Asserted through the reducer rather than on the array, because the array is
 * not what a reader sees: a merge that keeps the frame but leaves it out of
 * order still ends the run as `running`, and only replaying it says so.
 */
test("a tail frame that beat the history is kept, ordered, and never applied twice", () => {
  const started = { seq: 0, at_ms: 1000, kind: "run_started", data: { run_id: "r1", assignment: "fix" } };
  const stepped = { seq: 1, at_ms: 1100, kind: "step_started", data: { step: "propose" } };
  const ended = { seq: 2, at_ms: 1200, kind: "run_finished", data: { outcome: "success" } };
  // What really happens: the ending arrived first, and the history that
  // answered afterwards carried an event the tail had already delivered.
  const merged = mergeRunEvents([started, stepped], [ended, stepped]);
  // The defensive half: a frame belonging *inside* the history still lands in
  // log order, so the reducer is never handed a run running backwards.
  const gapped = mergeRunEvents([started, ended], [stepped]);
  // A log written before events carried a sequence replays exactly as it did.
  const unsequenced = [{ kind: "run_started", data: { run_id: "r0" } }, { kind: "step_started", data: { step: "propose" } }];

  assert.deepEqual(
    {
      order: merged.map((event) => event.kind),
      status: applyEvents(merged).status,
      repeated: mergeRunEvents(merged, [ended]).length,
      gapped: gapped.map((event) => event.kind),
      legacy: mergeRunEvents(unsequenced, []).map((event) => event.kind),
    },
    {
      order: ["run_started", "step_started", "run_finished"],
      status: "finished",
      repeated: merged.length,
      gapped: ["run_started", "step_started", "run_finished"],
      legacy: ["run_started", "step_started"],
    },
  );
});
