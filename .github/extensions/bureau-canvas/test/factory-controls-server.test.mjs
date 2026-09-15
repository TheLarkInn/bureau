import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";
const { openBureauCanvas, closeBureauCanvas } = await import("../extension.mjs");
const CONTROL = { session_id: "sdk-session", event_seq: 7, allowed: true, reason: null };

test("controls endpoint exposes CLI authority but never falls back to raw-log approval", async (t) => {
  const runsDir = await mkdtemp(join(tmpdir(), "bureau-factory-controls-"));
  t.after(() => rm(runsDir, { recursive: true, force: true }));
  const cases = [
    ["available", { code: 0, stdout: JSON.stringify({
      state: { run_id: "run-one" }, local_factory_resume: CONTROL,
    }), stderr: "" }, 200],
    ["unavailable", null, 503],
    ["incompatible", { code: 2, stdout: "", stderr: "local --json requires --events" }, 503],
  ];
  for (const [tag, result, status] of cases) {
    const instanceId = `factory-controls-${tag}`;
    const calls = [];
    const opened = await openBureauCanvas({ instanceId, input: {} }, {
      runsDir, exec: (args) => { calls.push(args); return result; },
    });
    try {
      const response = await fetch(new URL("runs/run-one/controls", opened.url));
      const body = await response.json();
      assert.equal(response.status, status);
      if (status === 200) {
        assert.deepEqual(body, { run_id: "run-one", local_factory_resume: CONTROL });
      } else {
        assert.ok(body.error);
        assert.equal(Object.hasOwn(body, "local_factory_resume"), false);
      }
      assert.deepEqual(calls, [["show", "run-one", "--json", "--runs", runsDir]]);
    } finally {
      await closeBureauCanvas({ instanceId });
    }
  }
});
