// Bridge actual fake-runtime Engine::run evidence through the newly built CLI and canvas host.
// No provider, model, factory-start or forge mutation is invoked here.
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

import { createDocument, parse, render } from "../lib/codec.mjs";
import { runBureau, summarize } from "../lib/runs.mjs";
import { factoryEvents } from "../web/live/copilot-factory.mjs";

process.env.BUREAU_CANVAS_NO_SDK = "1";
delete process.env.BUREAU_CANVAS_TEST;
const { openBureauCanvas, closeBureauCanvas } = await import("../extension.mjs");

const [binary, admittedReceipt, bootstrapReceipt, ...extra] = process.argv.slice(2);
assert.ok(binary && isAbsolute(binary), "Pass the explicit freshly built Bureau executable.");
assert.equal(extra.length, 0, "Pass the two original Engine evidence receipts, not copied run directories.");

const config = await mkdtemp(join(tmpdir(), "bureau-factory-canvas-"));
const expected = new Set(["completed", "paused", "unadmitted"]);

async function receipt(path, schema, producer, keys) {
  assert.ok(path && isAbsolute(path), "Pass an absolute original Engine evidence receipt path.");
  const document = JSON.parse(await readFile(path, "utf8"));
  assert.equal(document.schema, schema);
  assert.equal(document.producer, `copilot_factory_engine::evidence::${producer}`);
  assert.equal(document.fake_runtime, true, "Only offline fake-runtime evidence is accepted.");
  assert.ok(isAbsolute(document.source), "The receipt must identify its actual compiled source.");
  assert.ok(isAbsolute(document.runs_dir), "The receipt must preserve its actual Engine run root.");
  const entries = keys.map((key) => {
    const entry = document[key];
    assert.ok(entry?.run_id, `Missing actual ${key} run identity.`);
    assert.equal(entry.directory, join(document.runs_dir, entry.run_id));
    assert.equal(entry.events, join(entry.directory, "events.jsonl"));
    return entry;
  });
  return { runs: document.runs_dir, source: document.source, entries };
}

function checkSnapshot(events) {
  const snapshot = events.find((event) => event.kind === "run_started")?.data?.snapshot;
  assert.ok(snapshot?.pipeline, "Engine evidence must contain its actual immutable pipeline snapshot.");
  const text = createDocument(snapshot.pipeline);
  const doc = parse(text, { path: `pipelines/${snapshot.pipeline.name}.yaml` });
  const selected = snapshot.pipeline.steps.filter((step) => step.copilot_factory);
  assert.ok(selected.length > 0, "Evidence must be an opt-in local factory pipeline.");
  for (const step of selected) {
    assert.ok(typeof step.copilot_factory.model_credential === "string"
      && step.copilot_factory.model_credential.trim(), "Regenerate evidence after required model-auth references.");
    assert.deepEqual(doc.view.steps.find((candidate) => candidate.name === step.name)?.fields.copilotFactory,
      step.copilot_factory);
  }
  assert.equal(render(doc.view, doc.doc, doc.style), text, "Native factory snapshots must round-trip losslessly.");
}

async function checkRun(url, runs, evidence) {
  const runId = evidence.run_id;
  const result = await runBureau(["show", runId, "--events", "--json", "--runs", resolve(runs)], { binary });
  assert.equal(result?.code, 0, result?.stderr ?? "Explicit Bureau executable unavailable; no fallback permitted.");
  const events = JSON.parse(result.stdout);
  const projection = factoryEvents(events);
  assert.equal(projection.error, null);
  const records = Object.values(projection.records);
  assert.equal(records.length, 1, "Each retained fixture runs one actual factory step.");
  for (const record of records) {
    assert.ok(record.sessionId, "Real persisted SDK session identity required.");
    assert.equal(record.sessionId, evidence.native_session_id);
    assert.equal(record.runId, evidence.native_run_id);
    assert.equal(record.nativeStatus, evidence.native_status);
    assert.equal(record.problem, null);
    if (record.runId) {
      assert.ok(record.accounting, "Accepted native runs require actual accounting.");
      expected.delete(record.nativeStatus);
    } else {
      assert.equal(record.nativeStatus, null, "An unadmitted session has no native run status.");
      expected.delete("unadmitted");
    }
  }
  checkSnapshot(events);
  const response = await fetch(new URL(`runs/${encodeURIComponent(runId)}/events`, url));
  assert.equal(response.status, 200);
  const payload = await response.json();
  assert.equal(payload.source, undefined, "The host must use the explicit CLI, not raw-log or sample fallback.");
  assert.deepEqual(payload.events, events);
  const stateResult = await runBureau(["show", runId, "--json", "--runs", resolve(runs)], { binary });
  assert.equal(stateResult?.code, 0, stateResult?.stderr ?? "State projection unavailable.");
  const state = JSON.parse(stateResult.stdout);
  const controlsResponse = await fetch(new URL(`runs/${encodeURIComponent(runId)}/controls`, url));
  assert.equal(controlsResponse.status, 200);
  const controls = await controlsResponse.json();
  assert.deepEqual(controls, { run_id: runId, local_factory_resume: state.local_factory_resume });
  for (const record of records.filter((record) => record.nativeStatus === "paused" || !record.runId)) {
    assert.equal(controls.local_factory_resume?.allowed, true);
    assert.equal(controls.local_factory_resume.session_id, record.sessionId);
    assert.equal(controls.local_factory_resume.event_seq, record.eventSeq);
  }
  const listing = await (await fetch(new URL("runs", url))).json();
  const actual = listing.runs.find((run) => run.run_id === runId);
  assert.deepEqual(actual, summarize(runId, events));
  process.stdout.write(`PASS actual Engine -> CLI -> canvas: ${runId}\n`);
}

try {
  const groups = await Promise.all([
    receipt(admittedReceipt, "bureau-factory-engine-evidence-v1",
      "completed_and_paused_runs", ["completed", "paused"]),
    receipt(bootstrapReceipt, "bureau-factory-engine-bootstrap-evidence-v1",
      "clean_bootstrap_run", ["bootstrap"]),
  ]);
  assert.equal(groups[0].source, groups[1].source, "Generate all receipts from the same compiled source.");
  for (const [index, group] of groups.entries()) {
    const instanceId = `native-engine-evidence-${index}`;
    try {
      const opened = await openBureauCanvas({ instanceId, input: { dir: config } },
        { binary, runsDir: resolve(group.runs) });
      for (const entry of group.entries) await checkRun(opened.url, group.runs, entry);
    } finally {
      await closeBureauCanvas({ instanceId });
    }
  }
  assert.deepEqual([...expected], [], "Completed, paused and clean unadmitted evidence must be exercised.");
} finally {
  await rm(config, { recursive: true, force: true });
}
