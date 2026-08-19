// Run observation and control: liveness classification, event replay, the
// run-event SSE channel, and the pause/resume/cancel intents. Everything
// runs against fixture `runs/` directories; the real engine never runs.

import assert from "node:assert/strict";
import { appendFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";

const canvas = await import("../extension.mjs");

const RUN_STARTED = { seq: 0, at_ms: 1700000000000, kind: "run_started", data: { run_id: "run-live", assignment: "triage" } };
const STEP_STARTED = { seq: 1, at_ms: 1700000001000, kind: "step_started", data: { step: "implement" } };
const STEP_FINISHED = { seq: 2, at_ms: 1700000002000, kind: "step_finished", data: { step: "implement", outcome: "success" } };
const RUN_FINISHED = { seq: 3, at_ms: 1700000003000, kind: "run_finished", data: { outcome: "success" } };

function log(...events) {
  return `${events.map((event) => JSON.stringify(event)).join("\n")}\n`;
}

/** A fixture `runs/` dir: one live run mid-step, one finished run. */
async function fixtureRuns() {
  const dir = await mkdtemp(join(tmpdir(), "bureau-canvas-runs-"));
  await mkdir(join(dir, "run-live"), { recursive: true });
  await mkdir(join(dir, "run-done"), { recursive: true });
  await writeFile(join(dir, "run-live", "events.jsonl"), log(RUN_STARTED, STEP_STARTED));
  await writeFile(join(dir, "run-done", "events.jsonl"), log(RUN_STARTED, STEP_STARTED, STEP_FINISHED, RUN_FINISHED));
  return dir;
}

async function json(url) {
  const response = await fetch(url);
  return { status: response.status, body: await response.json() };
}

async function openCanvas(instanceId, options) {
  return canvas.openBureauCanvas({ instanceId, input: {} }, options);
}

test("lists runs with liveness and current step", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const opened = await openCanvas("bureau-runs-list-test", { runsDir: dir });

  try {
    const { body } = await json(new URL("/runs", opened.url));
    assert.deepStrictEqual(body.runs, [
      {
        run_id: "run-done",
        assignment: "triage",
        started_at: new Date(RUN_STARTED.at_ms).toISOString(),
        live: false,
        current_step: null,
      },
      {
        run_id: "run-live",
        assignment: "triage",
        started_at: new Date(RUN_STARTED.at_ms).toISOString(),
        live: true,
        current_step: "implement",
      },
    ]);
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-runs-list-test" });
  }
});

test("replays a run's events from its log", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const opened = await openCanvas("bureau-run-events-test", { runsDir: dir, exec: () => null });

  try {
    const { status, body } = await json(new URL("/runs/run-done/events", opened.url));
    assert.deepStrictEqual([status, body.run_id, body.events.length, body.source], [200, "run-done", 4, "log"]);
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-run-events-test" });
  }
});

test("reports a missing run as not found", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const opened = await openCanvas("bureau-run-missing-test", { runsDir: dir, exec: () => null });

  try {
    const response = await fetch(new URL("/runs/run-absent/events", opened.url));
    assert.equal(response.status, 404);
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-run-missing-test" });
  }
});

test("forwards appended run events over SSE until the run finishes", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const opened = await openCanvas("bureau-run-tail-test", { runsDir: dir, runTailIntervalMs: 25 });
  const response = await fetch(new URL("/events", opened.url));
  const reader = response.body.getReader();
  const decoder = new TextDecoder();

  /** Reads chunks until `needle` appears or five seconds pass. */
  async function readUntil(needle) {
    let seen = "";
    const until = Date.now() + 5000;
    while (!seen.includes(needle) && Date.now() < until) {
      const { value } = await reader.read();
      seen += decoder.decode(value, { stream: true });
    }
    return seen;
  }

  try {
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 150));
    await appendFile(join(dir, "run-live", "events.jsonl"), `${JSON.stringify(STEP_FINISHED)}\n`);
    const step = await readUntil('"kind":"step_finished"');
    assert.match(step, /event: run-event\ndata: .*"run_id":"run-live".*"kind":"step_finished"/);

    await appendFile(join(dir, "run-live", "events.jsonl"), `${JSON.stringify(RUN_FINISHED)}\n`);
    const finish = await readUntil('"kind":"run_finished"');
    assert.match(finish, /event: run-event\ndata: .*"run_id":"run-live".*"kind":"run_finished"/);
  } finally {
    reader.releaseLock();
    await canvas.closeBureauCanvas({ instanceId: "bureau-run-tail-test" });
  }
});

test("maps run control intents to bureau pause/resume/cancel", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const calls = [];
  const exec = (args) => {
    calls.push(args);
    return { code: 0, stdout: `run marked\n`, stderr: "" };
  };
  const opened = await openCanvas("bureau-run-control-test", { runsDir: dir, exec });

  try {
    for (const [kind, verb] of [["pause-run", "pause"], ["resume-run", "resume"], ["cancel-run", "cancel"]]) {
      const response = await fetch(new URL("/intent", opened.url), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ kind, run_id: "run-live" }),
      });
      const body = await response.json();
      assert.deepStrictEqual([body.ok, calls.at(-1)], [true, [verb, "run-live", "--runs", dir]]);
    }
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-run-control-test" });
  }
});

test("rejects a control intent without a run id", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const opened = await openCanvas("bureau-run-control-400-test", { runsDir: dir, exec: () => null });

  try {
    const response = await fetch(new URL("/intent", opened.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "pause-run" }),
    });
    assert.equal(response.status, 400);
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-run-control-400-test" });
  }
});
