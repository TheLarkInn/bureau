import assert from "node:assert/strict";
import test from "node:test";

import { nextSeed, runSoak, soakOptions } from "./maintenance-soak.mjs";
import { validateSuite } from "./maintenance-suite.mjs";
import { COMMIT, libtestRun } from "./maintenance-test-support.mjs";

function clock() {
  let time = 0;
  return { now: () => time, sleep: async (milliseconds) => { time += milliseconds; } };
}

test("finite seed sequence and manifest schema are deterministic", () => {
  assert.deepEqual([nextSeed(0), nextSeed(1)], [1013904223, 1015568748]);
  const suite = { schema: "bureau-chaos-suite-v1", source_commit: COMMIT,
    binary: "/prepared/maintenance_chaos", sha256: "2".repeat(64), test: "seeded_offline_invariants" };
  assert.equal(validateSuite(suite), suite);
  for (const changed of [{ ...suite, test: "" }, { ...suite, source_commit: "main" },
    { ...suite, sha256: null }]) assert.throws(() => validateSuite(changed));
});

test("soak duration and iteration bounds cannot grow without limit", () => {
  for (const options of [{ seed: -1 }, { seed: 2 ** 32 }, { minutes: 31 },
    { minutes: 0 }, { minutes: NaN }, { maxIterations: 2001 }, { maxIterations: 0 }]) {
    assert.throws(() => soakOptions(options));
  }
});

test("a successful campaign reaches its requested duration without a real wait or a build", async () => {
  const seeds = [];
  const result = await runSoak(soakOptions({ seed: 17, minutes: 1 }), async (seed) => {
    seeds.push(seed);
    return libtestRun();
  }, clock());
  assert.equal(result.complete && result.iterations > 0 && result.elapsed_ms === 60_000, true);
  assert.equal(seeds[1], nextSeed(seeds[0]));
});

test("assertions retain the failing seed and infrastructure never becomes a clean report", async () => {
  for (const [run, kind] of [[libtestRun(true), "invariant"],
    [{ ...libtestRun(), problem: "disk floor" }, "infrastructure"]]) {
    const result = await runSoak(soakOptions({ seed: 37, minutes: 1 }), async () => run, clock());
    assert.deepEqual([result.complete, result.failure.kind, result.failure.seed], [false, kind, 37]);
  }
});

test("a repetition cap before the requested soak duration fails rather than claiming completion", async () => {
  const result = await runSoak(soakOptions({ minutes: 25, maxIterations: 1 }), async () => libtestRun(), clock());
  assert.deepEqual([result.complete, result.iterations, result.failure.kind], [false, 1, "incomplete"]);
});
