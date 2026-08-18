import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { findings, FINDING_SOURCES } from "../lib/findings.mjs";

const fixturesUrl = new URL("./fixtures/", import.meta.url);
const stub = fileURLToPath(new URL("./fixtures/findings-bureau.mjs", import.meta.url));

function fixtureDir(name) {
  return fileURLToPath(new URL(`./fixtures/${name}/.bureau`, import.meta.url));
}

function runFixture(name, options = {}) {
  return findings(fixtureDir(name), { binary: stub, ...options });
}

test("a bureau without --json is reported as skew, not as a crash", async () => {
  const old = fileURLToPath(new URL("./findings-old-bureau.mjs", fixturesUrl));

  const result = await findings(fixtureDir("findings-valid"), { binary: old });

  assert.deepEqual(
    {
      state: result.state,
      ok: result.ok,
      // The message has to say what to do; "did not return JSON" does not.
      actionable: result.message.includes("does not support") && result.message.includes("BUREAU_CANVAS_BUREAU"),
    },
    { state: "unsupported-binary", ok: false, actionable: true },
  );
});

test("a current binary is used even when an older one is found first", async () => {
  const old = fileURLToPath(new URL("./findings-old-bureau.mjs", fixturesUrl));
  const dir = fixtureDir("findings-valid");

  // `locateCandidates` prefers PATH, so simulate PATH holding the stale one
  // and the workspace holding a current build.
  const result = await findings(dir, { candidates: [old, stub] });

  assert.deepEqual({ state: result.state, ok: result.ok }, { state: "validated", ok: true });
});

test("valid config returns no findings", async () => {
  const result = await runFixture("findings-valid");

  assert.deepEqual(
    { ok: result.ok, state: result.state, errors: result.errors.length, findings: result.findings.length },
    { ok: true, state: "validated", errors: 0, findings: 0 },
  );
});

test("invalid config keeps messages and attaches step targets", async () => {
  const result = await runFixture("findings-invalid");
  const first = result.findings[0];

  assert.deepEqual(
    {
      ok: result.ok,
      source: first.source,
      message: first.message,
      target: first.target,
      totals: [result.errors.length, result.findings.length],
    },
    {
      ok: false,
      source: FINDING_SOURCES.validate,
      message: "pipeline `bad-edge` step `verify`: unknown next target `missing`",
      target: { kind: "step", pipeline: "bad-edge", step: "verify" },
      totals: [3, 3],
    },
  );
});

test("unattributed errors stay visible as file findings", async () => {
  const result = await runFixture("findings-invalid");

  assert.deepEqual(result.findings.map((finding) => finding.target), [
    { kind: "step", pipeline: "bad-edge", step: "verify" },
    { kind: "pipeline", pipeline: "bad-edge", path: "pipelines/bad-edge.yaml" },
    { kind: "file", path: "notes.txt" },
  ]);
});

test("crash without JSON is distinct from invalid config", async () => {
  const result = await runFixture("findings-crash");

  assert.deepEqual(
    { ok: result.ok, state: result.state, errors: result.errors, findings: result.findings, exitCode: result.exitCode },
    { ok: false, state: "crash", errors: [], findings: [], exitCode: 101 },
  );
});

test("missing binary is a distinct state", async () => {
  const result = await findings(fixtureDir("findings-valid"), { binary: fileURLToPath(new URL("./missing-bureau.mjs", fixturesUrl)) });

  assert.deepEqual(
    { ok: result.ok, state: result.state, errors: result.errors, findings: result.findings },
    { ok: false, state: "binary-missing", errors: [], findings: [] },
  );
});

test("missing dir is a distinct state", async () => {
  const result = await runFixture("findings-missing");

  assert.deepEqual(
    { ok: result.ok, state: result.state, errors: result.errors, findings: result.findings },
    { ok: false, state: "dir-missing", errors: [], findings: [] },
  );
});
