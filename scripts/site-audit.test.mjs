import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { addFinding, auditSchema, incompleteReport, reportStatus, viewports } from "../site/audit.mjs";
import { checkOptions, encodeReport } from "../site/check.mjs";

test("audit statuses never certify skipped or incomplete coverage", () => {
  const base = { schema: auditSchema, kind: "accessibility", complete: true, checks: 1, findings: [] };
  for (const [report, status] of [
    [base, 0], [{ ...base, findings: [{ id: "issue" }] }, 1],
    [{ ...base, checks: 0 }, 2], [{ ...base, complete: false }, 2],
    [{ ...base, checks: -1 }, 2], [incompleteReport("responsive", "Missing browser."), 2],
  ]) assert.equal(reportStatus(report), status);
});

test("audit argument parsing is strict and does not offer an installation fallback", () => {
  assert.deepEqual({ ...checkOptions(["--kind", "accessibility", "--json"]) }, { kind: "accessibility", json: true });
  for (const args of [[], ["--kind", "static"], ["--kind", "responsive", "--install"], ["--kind", "responsive", "extra"]]) {
    assert.throws(() => checkOptions(args));
  }
  assert.deepEqual(viewports, [320, 375, 768, 1024, 1280, 1536]);
});

test("invalid check requests return one versioned JSON document and a tooling failure", () => {
  const result = spawnSync(process.execPath, ["site/check.mjs", "--kind", "invalid", "--json"], {
    encoding: "utf8", timeout: 10_000, maxBuffer: 64 * 1024,
  });
  assert.equal(result.status, 2);
  const report = JSON.parse(result.stdout);
  assert.equal(report.schema, auditSchema);
  assert.equal(report.complete, false);
  assert.equal(report.checks, 0);
  assert.match(report.error.message, /--kind/u);
});

test("oversized reports fail explicitly instead of truncating findings into success", () => {
  const encoded = encodeReport({
    schema: auditSchema, kind: "responsive", complete: true, checks: 1,
    findings: [{ id: "large", detail: "x".repeat(140 * 1024) }],
  });
  const report = JSON.parse(encoded);
  assert.ok(Buffer.byteLength(encoded) < 128 * 1024);
  assert.equal(reportStatus(report), 2);
  assert.match(report.error.message, /exceeded/u);
});

test("nonempty findings satisfy the bounded maintenance adapter shape", () => {
  const report = { schema: auditSchema, kind: "responsive", complete: true, checks: 144, findings: [] };
  addFinding(report, "responsive-repo-320-pass-overflow", "Page overflows the viewport",
    "420px content in a 320px viewport.", "site/src/assets/layout.css");
  assert.equal(reportStatus(report), 1);
  assert.deepEqual(report.findings, [{
    id: "responsive-repo-320-pass-overflow", title: "Page overflows the viewport",
    detail: "420px content in a 320px viewport.", path: "site/src/assets/layout.css",
  }]);
  addFinding(report, "responsive-root-320-pass-overflow", "Page overflows the viewport",
    "420px content in a 320px viewport.", "site/src/assets/layout.css");
  assert.equal(report.findings.length, 1);
});

test("finding limits fail rather than certify a partially reported audit", () => {
  const report = { findings: [] };
  for (let index = 0; index < 20; index += 1) addFinding(report, `test-${index}`, `Title ${index}`, "Detail");
  assert.throws(() => addFinding(report, "extra", "Another defect", "Detail"), /incomplete/u);
  assert.throws(() => addFinding({ findings: [] }, "bad id", "Bad identity", "Detail"), /Invalid/u);
  assert.throws(() => addFinding({ findings: [] }, "id", "Bad path", "Detail", "site/src/../package.json"), /Invalid/u);
});

test("browser audits are bounded and no package installation happens in the runner", async () => {
  const source = await readFile(new URL("../site/audit.mjs", import.meta.url), "utf8");
  const config = await readFile(new URL("../site/playwright.config.mjs", import.meta.url), "utf8");
  assert.match(source, /chromium\.launch/u);
  assert.match(source, /window\.axe\.run/u);
  assert.match(source, /110_000/u);
  assert.match(config, /workers: 1/u);
  assert.match(config, /retries: 0/u);
  assert.match(config, /trace: "off", video: "off"/u);
  assert.match(config, /contextOptions: \{ reducedMotion: "reduce" \}/u);
  assert.doesNotMatch(source, /(?:spawn|exec|install|npx|npm ci)\(/u);
});
