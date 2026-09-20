import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { factoryForStep, factoryResumeBlocked } from "../web/live/copilot-factory.mjs";
import { applyEvents, runActions } from "../web/live/overlay.mjs";
import { enginePauseEvidence } from "./support/engine-pause.mjs";

const enabled = process.platform === "linux" || Boolean(process.env.BUREAU_ENGINE_PAUSE_EVIDENCE_DIR);

function paused(projection) {
  const overlay = applyEvents(projection.events);
  assert.deepEqual([overlay.status, runActions(overlay.status)],
    ["paused", { transport: "resume", cancel: true }]);
  const output = projection.events.findLast((event) => event.kind === "output" && event.data.stream === "run");
  assert.match(output.data.data, /^run paused at a step boundary; /u);
  assert.ok(output.data.data.endsWith(projection.marker.trim()), "retain the actual pause reason");
  assert.equal(projection.events.some((event) => event.kind === "run_finished"), false);
  return overlay;
}

describe("actual Engine to fresh CLI to canvas pause projection", { skip: !enabled }, () => {
  let evidence;
  before(async () => { evidence = await enginePauseEvidence(); });
  after(async () => { await evidence?.cleanup(); });

  test("ordinary CLI pause inside an Engine step offers Resume", async () => {
    const projection = await evidence.read("ordinary");
    const overlay = paused(projection);
    assert.deepEqual([projection.marker, projection.control, Object.keys(overlay.factories.records)],
      ["paused\n", null, []]);
  });

  test("clean bootstrap pause stays unadmitted and uses Bureau continuation eligibility", async () => {
    const projection = await evidence.read("bootstrap");
    const overlay = paused(projection);
    const factory = factoryForStep(overlay.factories, "factory-step");
    assert.deepEqual([factory.runId, factory.nativeStatus, factory.canResume, projection.control.allowed,
      factoryResumeBlocked(factory, projection.control)], [null, null, false, true, false]);
  });

  test("an SDK-paused run retains its separate native status and resume authority", async () => {
    const projection = await evidence.read("sdk-paused");
    const overlay = paused(projection);
    const factory = factoryForStep(overlay.factories, "factory-step");
    assert.deepEqual([factory.nativeStatus, factory.canResume, factory.executionClean,
      factoryResumeBlocked(factory, projection.control)], ["paused", true, true, false]);
  });

  test("actual in-flight prefixes still offer Pause rather than Resume", async () => {
    for (const scenario of ["ordinary", "bootstrap", "sdk-paused", "finished"]) {
      const { events } = await evidence.read(scenario);
      const first = events.findIndex((event) => event.kind === "step_started");
      assert.notEqual(first, -1);
      const overlay = applyEvents(events.slice(0, first + 1));
      assert.deepEqual([overlay.status, runActions(overlay.status)],
        ["running", { transport: "pause", cancel: true }], scenario);
    }
  });

  test("a genuinely finished Engine run has no transport control", async () => {
    const projection = await evidence.read("finished");
    const overlay = applyEvents(projection.events);
    assert.deepEqual([overlay.status, runActions(overlay.status), projection.marker,
      projection.events.filter((event) => event.kind === "run_finished").length],
    ["finished", { transport: null, cancel: false }, null, 1]);
  });
});
