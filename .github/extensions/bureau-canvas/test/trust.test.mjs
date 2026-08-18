import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { FINDING_SOURCES } from "../lib/findings.mjs";
import { trustFindings } from "../lib/trust.mjs";

async function fixture(name) {
  const url = new URL(`./fixtures/trust-${name}-payload.json`, import.meta.url);
  return JSON.parse(await readFile(url, "utf8"));
}

async function findingsFor(name) {
  return trustFindings(await fixture(name));
}

test("fixed laundering pipeline is silent when the write role requires maintainer trust", async () => {
  assert.deepEqual(await findingsFor("fixed"), []);
});

test("laundering pipeline reports the path into a write-capable role", async () => {
  const findings = await findingsFor("laundering");
  const [finding] = findings;

  assert.deepEqual(
    {
      count: findings.length,
      source: finding.source,
      marker: finding.marker,
      target: finding.target,
      namesPath: finding.message.includes("triage -> propose"),
      permission: finding.message.includes("`repo:push`"),
    },
    {
      count: 1,
      source: FINDING_SOURCES.advisory,
      marker: "trust-advisory",
      target: { kind: "step", pipeline: "trust-laundering", step: "propose" },
      namesPath: true,
      permission: true,
    },
  );
});

test("step trust override can raise the write-capable gate and sever the path", async () => {
  assert.deepEqual(await findingsFor("step-override"), []);
});

test("multi-hop laundering reports once with the full data path", async () => {
  const findings = await findingsFor("multi-hop");

  assert.deepEqual(
    {
      count: findings.length,
      target: findings[0]?.target,
      path: findings[0]?.message.includes("triage -> plan -> propose"),
    },
    {
      count: 1,
      target: { kind: "step", pipeline: "trust-multi-hop", step: "propose" },
      path: true,
    },
  );
});

test("a gate that rejects the arriving grade stops propagation midway", async () => {
  assert.deepEqual(await findingsFor("severed-midway"), []);
});
