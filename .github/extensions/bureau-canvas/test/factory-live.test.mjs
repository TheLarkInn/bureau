import assert from "node:assert/strict";
import test from "node:test";

import { summarize } from "../lib/runs.mjs";
import { applyEvents, stateUpTo } from "../web/live/overlay.js";
import { factoryDetails, factoryEvents, factoryForStep, factoryResumeBlocked } from "../web/live/copilot-factory.mjs";
import { readHistory } from "../web/live/history.mjs";

const INTENT = {
  session_id: "sdk-session", step: "review", step_attempt: 1,
  factory: { name: "review-factory", extension: "project:review", runtime: { profile: "C", version: "qualified" } },
  workspace: { directory: "/private/original-worktree" },
};
const local = (data) => ({ kind: "copilot_factory", data: { session_id: INTENT.session_id, ...data } });
const sequence = (events) => events.map((event, seq) => ({ ...event, seq, at_ms: seq * 1000 }));
const BASE = [
  { kind: "run_started", data: { run_id: "bureau-run", assignment: "review" } },
  { kind: "step_started", data: { step: "review" } },
  local({ event: "prepared", intent: INTENT }),
  local({ event: "runtime_opened", purpose: "execution" }),
  local({ event: "session_accepted" }),
  local({ event: "dispatch", operation: "start" }),
  local({ event: "accepted", run_id: "native-run", attempt: 1 }),
];
const close = (purpose = "execution", clean = true) => local({ event: "runtime_closed", purpose, clean, message: "closed" });
const allowed = (record) => ({
  session_id: record.sessionId, event_seq: record.eventSeq, allowed: true, reason: null,
});
function observed(status, canResume = false, nanoAiu = 2e9) {
  return local({
    event: "observed", run: { runId: "native-run", status },
    summary: { runId: "native-run", factoryName: "review-factory", status, canResume,
      consumed: { activeMs: 500, subagents: 2, nanoAiu }, terminal: null },
  });
}

test("paused native factories are displayed with exact separate identities and resumability", () => {
  const overlay = applyEvents(sequence([...BASE, observed("paused", true), close()]));
  const record = factoryForStep(overlay.factories, "review");
  assert.deepEqual([overlay.status, record.sessionId, record.runId, record.attempt, factoryResumeBlocked(record, allowed(record))],
    ["paused", "sdk-session", "native-run", 1, false]);
  assert.match(JSON.stringify(factoryDetails(record)), /private\/original-worktree/u);
});

test("inspection shutdown cannot certify the original execution or offer native resume", () => {
  const state = factoryEvents([...BASE, observed("paused", true), close("inspection")]);
  assert.equal(factoryResumeBlocked(factoryForStep(state, "review")), true);
});

test("native completed status does not complete the Bureau step or advertise resume", () => {
  const overlay = applyEvents(sequence([...BASE, observed("completed"), close()]));
  assert.deepEqual([overlay.status, overlay.steps.review.outcome, factoryForStep(overlay.factories, "review").canResume],
    ["running", null, false]);
});

test("Bureau may reconcile an already-completed clean result without requesting native resume", () => {
  const factory = factoryForStep(factoryEvents(sequence([...BASE, observed("completed"), close()])), "review");
  assert.deepEqual([factory.canResume, factoryResumeBlocked(factory, allowed(factory))], [false, false]);
});

test("browser replay alone cannot approve a paused factory or an unadmitted bootstrap", () => {
  const paused = factoryForStep(factoryEvents(sequence([...BASE, observed("paused", true), close()])), "review");
  const bootstrap = factoryForStep(factoryEvents(sequence([...BASE.slice(0, 5), close()])), "review");
  assert.deepEqual([factoryResumeBlocked(paused), factoryResumeBlocked(bootstrap)], [true, true]);
  assert.equal(factoryResumeBlocked(bootstrap, allowed(bootstrap)), false);
});

test("resume eligibility must bind to the exact session and native event prefix", () => {
  const factory = factoryForStep(factoryEvents(sequence([...BASE, observed("paused", true), close()])), "review");
  const control = allowed(factory);
  for (const stale of [{ ...control, session_id: "different" }, { ...control, event_seq: control.event_seq - 1 },
    { ...control, allowed: false, reason: "Not safe to continue." }]) {
    assert.equal(factoryResumeBlocked(factory, stale), true);
  }
});

test("resuming a paused execution clears the paused overlay while retaining cumulative usage", () => {
  const events = [...BASE, observed("paused", true), close(),
    local({ event: "runtime_opened", purpose: "execution" }),
    local({ event: "session_accepted" }), local({ event: "dispatch", operation: "resume" }),
    local({ event: "accepted", run_id: "native-run", attempt: 2 }), observed("running")];
  const overlay = applyEvents(sequence(events));
  const factory = factoryForStep(overlay.factories, "review");
  assert.deepEqual([overlay.status, factory.nativeStatus, factory.attempt, factory.accounting.nanoAiu],
    ["running", "running", 2, 2e9]);
});

test("ambiguous acceptance is preserved rather than adopting notification identity", () => {
  const events = sequence([
    ...BASE.slice(0, -1),
    local({ event: "notification", notification: { runId: "orphan", status: "completed" } }),
    local({ event: "indeterminate", message: "Start reply lost. Preserve workspace; do not retry." }),
    { kind: "output", data: { stream: "run", data: "paused at a step boundary" } },
  ]);
  const overlay = applyEvents(events);
  const factory = factoryForStep(overlay.factories, "review");
  assert.deepEqual([overlay.status, factory.runId, factoryResumeBlocked(factory)], ["indeterminate", null, true]);
  assert.equal(summarize("bureau-run", events).copilot_factories.records["sdk-session"].workspace, INTENT.workspace.directory);
});

test("unknown status, mismatched identity and missing accounting produce visible errors", () => {
  const unknown = observed("new-upstream-state");
  const wrong = observed("running");
  wrong.data.summary.runId = "another-run";
  const missing = observed("completed");
  delete missing.data.summary.consumed;
  for (const event of [unknown, wrong, missing]) {
    const overlay = applyEvents(sequence([...BASE, event]));
    assert.equal(overlay.status, "indeterminate");
  }
});

test("hard interruption refuses resume even if an inconsistent response advertises it", () => {
  const event = observed("error", true);
  event.data.run.reason = "interrupted";
  const factory = factoryForStep(factoryEvents([...BASE, event, close()]), "review");
  assert.deepEqual([factory.canResume, factoryResumeBlocked(factory)], [false, true]);
  assert.match(factory.problem, /hard-crashed/u);
});

test("cumulative and out-of-order observations use a high-water mark, not duplicate charges", () => {
  const state = factoryEvents([...BASE, observed("running"), observed("running"), observed("running", false, 1e9)]);
  const factory = factoryForStep(state, "review");
  assert.deepEqual(factory.accounting, { activeMs: 500, subagents: 2, nanoAiu: 2e9 });
});

test("incomplete native accounting is labeled as a floor, not an exact zero", () => {
  const event = observed("error", true);
  event.data.run.failure = { code: "factory_accounting_incomplete" };
  const factory = factoryForStep(factoryEvents([...BASE, event, close()]), "review");
  assert.match(JSON.stringify(factoryDetails(factory)), /2 \(incomplete floor\)/u);
  assert.equal(factoryResumeBlocked(factory), true);
});

test("replay is prefix-exact and does not fold future native status into the past", () => {
  const events = sequence([...BASE, observed("running"), observed("paused", true), close()]);
  const earlier = factoryForStep(stateUpTo(events, 7000).factories, "review");
  const latest = factoryForStep(stateUpTo(events, 9000).factories, "review");
  assert.deepEqual([earlier.nativeStatus, latest.nativeStatus], ["running", "paused"]);
});

test("malformed factory events latch diagnostics and cloud events never become local runs", () => {
  const malformed = local({ event: "observed", session_id: "unknown" });
  const state = factoryEvents([{ kind: "github_cloud", data: { event: "prepared", intent: INTENT } }, malformed, ...BASE]);
  assert.deepEqual(Object.keys(state.records), []);
  assert.match(state.error, /no matching durable intent/u);
});

test("unreadable native history is an error, never an empty successful replay", async () => {
  await assert.rejects(readHistory(new Response("not found", { status: 404 })), /Run history unavailable/u);
  await assert.rejects(readHistory({ ok: true, json: async () => ({ result: [] }) }), /no event array/u);
  assert.deepEqual(await readHistory({ ok: true, json: async () => ({ events: [] }) }), { events: [] });
});
