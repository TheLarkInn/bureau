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
  resolveOverlay,
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
