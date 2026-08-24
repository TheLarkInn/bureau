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
const { resolveRunsDir, runsDir, summarize } = await import("../lib/runs.mjs");

/*
 * `run_started` as bureau actually writes it.
 *
 * `RunStartedData` (crates/bureau/src/runlog/event.rs) is `run_id`,
 * `assignment`, `item` and `snapshot` — there is no flat `pipeline` field, and
 * the pipeline identity is pinned inside the snapshot. A fixture that invented
 * one read back green while every real run summarised to `pipeline: null`.
 */
const SNAPSHOT = {
  run_id: "run-live",
  assignment: { name: "triage", pipeline: "triage-pipeline" },
  pipeline: { name: "triage-pipeline" },
};
const RUN_STARTED = { seq: 0, at_ms: 1700000000000, kind: "run_started", data: { run_id: "run-live", assignment: "triage", item: "item-1", snapshot: SNAPSHOT } };
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
        pipeline: "triage-pipeline",
        started_at: new Date(RUN_STARTED.at_ms).toISOString(),
        live: false,
        current_step: null,
      },
      {
        run_id: "run-live",
        assignment: "triage",
        pipeline: "triage-pipeline",
        started_at: new Date(RUN_STARTED.at_ms).toISOString(),
        live: true,
        current_step: "implement",
      },
    ]);
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-runs-list-test" });
  }
});

/*
 * The three ways a run's pipeline can be read, and the one way it cannot.
 *
 * The reader walks `snapshot.pipeline.name`, then `snapshot.assignment.pipeline`,
 * then a flat `data.pipeline` that bureau does not currently write. A log old
 * enough to predate snapshots carries none of them and is unattributable — it
 * must summarise to `null` rather than be silently assigned somewhere.
 */
test("a run's pipeline is read from the snapshot bureau pins, not an invented field", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-canvas-pipeline-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const cases = {
    "from-pipeline": { snapshot: { pipeline: { name: "by-pipeline" }, assignment: { name: "a", pipeline: "ignored" } } },
    "from-assignment": { snapshot: { assignment: { name: "a", pipeline: "by-assignment" } } },
    "from-legacy-log": {},
  };
  for (const [runId, data] of Object.entries(cases)) {
    await mkdir(join(dir, runId), { recursive: true });
    await writeFile(join(dir, runId, "events.jsonl"), log({ seq: 0, at_ms: 1700000000000, kind: "run_started", data: { run_id: runId, assignment: "a", ...data } }));
  }
  const opened = await openCanvas("bureau-runs-pipeline-test", { runsDir: dir });

  try {
    const { body } = await json(new URL("/runs", opened.url));
    assert.deepStrictEqual(
      Object.fromEntries(body.runs.map((run) => [run.run_id, run.pipeline])),
      { "from-pipeline": "by-pipeline", "from-assignment": "by-assignment", "from-legacy-log": null },
    );
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-runs-pipeline-test" });
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

test("runs one reconcile pass in the run root this server reads", async (t) => {
  const dir = await fixtureRuns();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const calls = [];
  const opened = await openCanvas("bureau-reconcile-now-test", {
    runsDir: dir,
    exec: (args) => {
      calls.push(args);
      return { code: 0, stdout: "no eligible work\n", stderr: "" };
    },
  });

  try {
    const response = await fetch(new URL("/intent", opened.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "reconcile-now" }),
    });
    const body = await response.json();
    // The run root is asserted, not just the verb: a pass that wrote its log
    // anywhere else would be invisible to the listing this same server serves.
    assert.deepStrictEqual([body.ok, body.output, calls], [true, "no eligible work", [["reconcile", "--now", "--runs", dir]]]);
  } finally {
    await canvas.closeBureauCanvas({ instanceId: "bureau-reconcile-now-test" });
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

// A canvas host on Windows watching a workspace inside WSL must read the
// distro's bureau home, not its own user profile — otherwise `GET /runs` is
// always empty and the live and replay pickers have nothing to select.
const SHARE = String.raw`\\wsl.localhost\Ubuntu-26.04\home\me\src\bureau\.bureau`;
const SHARE_RUNS = String.raw`\\wsl.localhost\Ubuntu-26.04\home\me\.bureau\runs`;

test("resolves the runs root where bureau actually keeps it", async () => {
  const probed = [];
  const probe = (distro) => {
    probed.push(distro);
    return "/home/me/.bureau\n";
  };
  // An explicit binary that cannot exist keeps the lookup hermetic: no PATH
  // scan, no stray `target/debug/bureau` from whatever checkout runs this.
  const noBinary = { binary: join(tmpdir(), "absent-bureau-binary") };
  const cases = [
    // A workspace on a WSL share: the distro's home, addressed through it.
    [{ anchor: SHARE, env: {}, probe }, SHARE_RUNS],
    // Nothing bridged anywhere: the host's own home, as before.
    [{ anchor: "/plain/workspace", env: {}, probe, ...noBinary }, runsDir({})],
    // Explicit overrides win outright, and never spawn a probe.
    [{ anchor: SHARE, env: { BUREAU_CANVAS_RUNS: join(tmpdir(), "explicit") }, probe }, join(tmpdir(), "explicit")],
    [{ anchor: SHARE, env: { BUREAU_HOME: join(tmpdir(), "home") }, probe }, join(tmpdir(), "home", "runs")],
  ];

  const actual = [];
  for (const [options] of cases) {
    actual.push(await resolveRunsDir(options));
  }
  assert.deepStrictEqual([actual, probed], [cases.map(([, expected]) => expected), ["Ubuntu-26.04"]]);
});

test("falls back to the host home when the distro cannot answer", async () => {
  const answers = [null, "", "   "];
  const resolved = [];
  for (const answer of answers) {
    resolved.push(await resolveRunsDir({ anchor: SHARE, env: {}, probe: () => answer }));
  }
  assert.deepStrictEqual(resolved, answers.map(() => runsDir({})));
});

/*
 * The committed run fixtures, held to the shape bureau writes.
 *
 * Every one of them used to carry a flat `data.pipeline` — a key
 * `RunStartedData` has no field for — so the whole pipeline-scoped Live surface
 * the browser suite exercises was green against a payload production cannot
 * emit, and the branch real runs take was reached by one node test alone.
 *
 * Each fixture is now pinned to the branch of `pipelineOf` it is there to
 * cover, so the three cannot collapse into one: `run-live` and `run-finished`
 * carry the pinned pipeline, `run-paused` carries only the assignment's
 * selection, and `run-group` deliberately keeps the flat key so the
 * forward-compatible last resort stays exercised rather than becoming dead code.
 */
const FIXTURE_PIPELINES = [
  ["run-live", "agent-eligible-pipeline", "snapshot.pipeline.name"],
  ["run-finished", "agent-eligible-pipeline", "snapshot.pipeline.name"],
  ["run-paused", "agent-eligible-pipeline", "snapshot.assignment.pipeline"],
  ["run-group", "review-queue-pipeline", "data.pipeline"],
];

/** Which key a fixture's `run_started` really carries the pipeline under. */
function carrier(started) {
  if (started.data?.snapshot?.pipeline?.name) {
    return "snapshot.pipeline.name";
  }
  return started.data?.snapshot?.assignment?.pipeline ? "snapshot.assignment.pipeline" : "data.pipeline";
}

test("each committed run fixture carries its pipeline where bureau writes it", async () => {
  const { readFile } = await import("node:fs/promises");
  const resolved = [];
  for (const [id] of FIXTURE_PIPELINES) {
    const raw = await readFile(new URL(`./fixtures/runs/${id}/events.jsonl`, import.meta.url), "utf8");
    const events = raw.trim().split("\n").map((line) => JSON.parse(line));
    resolved.push([id, summarize(id, events).pipeline, carrier(events[0])]);
  }

  assert.deepStrictEqual(resolved, FIXTURE_PIPELINES);
});
