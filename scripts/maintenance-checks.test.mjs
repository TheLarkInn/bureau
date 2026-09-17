import assert from "node:assert/strict";
import test from "node:test";

import { CheckFailure, chaosResult, patchProblem, siteResult } from "./maintenance-checks.mjs";
import { failureResult } from "./maintenance-step.mjs";
import { fixture, libtestRun } from "./maintenance-test-support.mjs";

function siteRun(findings = []) {
  return { code: findings.length ? 1 : 0, signal: null, problem: null,
    stdout: JSON.stringify({ schema: "bureau-site-check-v1", kind: "accessibility",
      checks: 8, complete: true, findings }) };
}

test("site reports require real complete positive-count checks and consistent exit statuses", () => {
  assert.deepEqual(siteResult(siteRun(), "site-accessibility"), { checks: 8, findings: [] });
  const payload = JSON.parse(siteRun().stdout);
  for (const changed of [{ ...payload, checks: 0 }, { ...payload, complete: false },
    { ...payload, kind: "responsive" }, { ...payload, findings: null }]) {
    assert.throws(() => siteResult({ ...siteRun(), stdout: JSON.stringify(changed) }, "site-accessibility"));
  }
  for (const changed of [{ ...siteRun(), code: 2 }, { ...siteRun(), code: null, signal: "SIGKILL" },
    { ...siteRun(), problem: "output cap" }]) {
    assert.throws(() => siteResult(changed, "site-accessibility"));
  }
});

test("an exact libtest assertion failure is reproducible evidence, missing tooling is not", () => {
  assert.deepEqual(chaosResult(libtestRun(), 17), { checks: 1, findings: [] });
  assert.match(chaosResult(libtestRun(true), 17).findings[0].detail, /BUREAU_CHAOS_SEED=17/u);
  for (const changed of [
    { ...libtestRun(), stdout: "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n" },
    { ...libtestRun(), stdout: libtestRun().stdout.replace("0 ignored", "1 ignored") },
    { ...libtestRun(), code: 1, stdout: "", stderr: "unshare: operation not permitted" },
    { ...libtestRun(), problem: "memory floor" }, { ...libtestRun(true), code: 0 },
  ]) assert.throws(() => chaosResult(changed, 17));
});

test("patches cannot edit verification, dependencies, automation or another category", () => {
  assert.equal(patchProblem(["site/src/assets/layout.css"], "site-responsive"), null);
  assert.equal(patchProblem(["crates/bureau/src/reconcile.rs", "dylint.toml"], "chaos"), null);
  for (const path of ["site/check.mjs", "site/audit.mjs", "site/content.mjs", "site/package.json",
    "scripts/maintenance-step.mjs", ".bureau/maintenance/repos.yaml", "crates/bureau/src/reconcile.rs"]) {
    assert.equal(typeof patchProblem([path], "site-accessibility"), "string");
  }
  for (const paths of [[], ["crates/bureau/tests/maintenance_chaos.rs"],
    ["crates/bureau/tests/maintenance_chaos/support.rs"], ["Cargo.toml"],
    ["crates/bureau/tests/runlog_framing.rs"], ["crates/bureau/tests/edge/testdir.rs"],
    ["crates/bureau/src/cli/run/tests.rs"], ["crates/bureau/src/cli/run/claim/tests.rs"],
    ["crates/bureau/src/cli/run/observe/tests.rs"],
    ["crates/bureau/tests/rate_admission.rs"], ["crates/bureau/tests/rate_admission/support.rs"],
    ["crates/bureau/src/state/accounting/tests.rs"], ["crates/bureau/src/state/accounting/tests/nested.rs"],
    ["crates/bureau/tests/migration_cli.rs"],
    ["crates/bureau/Cargo.toml"], ["crates/nested/deeper/Cargo.lock"],
    ["crates/bureau/build.rs"], ["crates/bureau/.cargo/config.toml"],
    ["crates/nested/package.json"], ["crates/nested/package-lock.json"],
    Array.from({ length: 21 }, (_, index) => `crates/source${index}.rs`)]) {
    assert.equal(typeof patchProblem(paths, "chaos"), "string");
  }
});

test("accounting implementation remains editable while its ledger and migration proofs stay protected", () => {
  assert.equal(patchProblem(["crates/bureau/src/state/accounting.rs"], "chaos"), null);
});

test("incomplete/manual browser evidence remains blocked but retains bounded raw diagnostics", async () => {
  const { request } = fixture("site-accessibility");
  request.step = "detect";
  const raw = '{"complete":false,"findings":[{"id":"manual-review"}]}\n';
  const error = new CheckFailure("site check needs manual review", raw);
  let captured;
  const artifacts = [{ name: "detect.log", path: "target/bureau-maintenance/detect.log" }];
  const result = await failureResult(error, request, async (value, log) => {
    captured = { value, log };
    return artifacts;
  });
  assert.deepEqual([result.outcome, result.artifacts, captured.log, captured.value.complete],
    ["blocked", artifacts, raw, false]);
});

test("failed evidence persistence is explicit and never turns a blocked check into success", async () => {
  const error = new CheckFailure("incomplete check", "bounded output");
  const result = await failureResult(error, { step: "detect", inputs: {} }, async () => {
    throw new Error("disk unavailable");
  });
  assert.equal(result.outcome, "blocked");
  assert.match(result.message, /incomplete check; failed to preserve check evidence: disk unavailable/u);
});
