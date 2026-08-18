import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import * as YAML from "../lib/vendor/yaml.mjs";

import { advisories, pluginPicker } from "../lib/advisories.mjs";

const root = fileURLToPath(new URL("./fixtures/advisory-config", import.meta.url));
const repo = fileURLToPath(new URL("../../../../", import.meta.url));

test("plugin picker reports that enumeration is not available", () => {
  assert.deepEqual(pluginPicker(), {
    enumerable: false,
    items: [],
    reason: "bureau-plugin has no read-only enumeration surface for resolved plugins and agents.",
  });
});

test("agent resolution feedback reports only unresolved roles", async () => {
  const found = await advisories(payload({ roles: { good: role("/bureau:good"), bad: role("/missing:bad") } }), {
    resolveAgent: async (agent) => ({ ok: agent === "/bureau:good", message: `agent ${agent} missing` }),
  });

  assert.deepEqual(found.map((item) => [item.marker, item.target.role, item.message]), [["agent-resolution", "bad", "agent /missing:bad missing"]]);
});

test("downstream deterministic check keeps an agent step silent", async () => {
  const found = await advisories(writePayload({ checked: pipeline([agentStep("implement", "verify"), checkStep("verify")]) }));

  assert.equal(found.some((item) => item.marker === "check-advisory"), false);
});

test("committed config produces no check advisories", async () => {
  const found = await advisories(await committedPayload());

  assert.deepEqual(found.filter((item) => item.marker === "check-advisory"), []);
});

test("write-capable agent routed straight to a terminal is advised", async () => {
  const found = await advisories(writePayload({ unchecked: pipeline([agentStep("implement", "done")]) }));

  assert.deepEqual(found.map((item) => [item.marker, item.target.pipeline, item.target.step]), [["check-advisory", "unchecked", "implement"]]);
});

test("read-only agent routed straight to a terminal stays silent", async () => {
  const found = await advisories(payload({ roles: { implementer: readRole() }, pipelines: { review: pipeline([agentStep("review", "done")]) } }));

  assert.deepEqual(found.filter((item) => item.marker === "check-advisory"), []);
});

test("script file advisory distinguishes missing and existing repo files", async () => {
  const steps = [
    { name: "real", type: "deterministic", run: "bash scripts/check.sh", next: "done" },
    { name: "missing", type: "deterministic", run: "bash scripts/missing.sh", next: "done" },
  ];
  const found = await advisories(payload({ pipelines: { scripts: pipeline(steps) } }), { repoRoot: root });

  assert.deepEqual(found.map((item) => [item.marker, item.target.step]), [["script-advisory", "missing"]]);
});

function payload(config) {
  return {
    ok: true,
    dir: `${root}/.bureau`,
    errors: [],
    findings: [],
    config: { repos: {}, roles: {}, assignments: {}, pipelines: {}, ...config },
  };
}

function writePayload(pipelines) {
  return payload({ roles: { implementer: writeRole() }, pipelines });
}

function role(agent) {
  return { name: "role", agent, adapter: "copilot", permissions: [] };
}

function writeRole() {
  return { name: "implementer", agent: "/bureau:implementer", adapter: "copilot", permissions: ["repo:write"] };
}

function readRole() {
  return { name: "implementer", agent: "/bureau:reviewer", adapter: "copilot", permissions: ["repo:read", "model:invoke"] };
}

function pipeline(steps) {
  return { name: "pipeline", steps };
}

function agentStep(name, next) {
  return { name, type: "agent", role: "implementer", next };
}

function checkStep(name) {
  return { name, type: "deterministic", run: "cargo test --offline", next: "done" };
}

async function committedPayload() {
  const dir = join(repo, ".bureau");
  return { ok: true, dir, errors: [], findings: [], config: await committedConfig(dir) };
}

async function committedConfig(dir) {
  return {
    ...(await YAML.parse(await readFile(join(dir, "repos.yaml"), "utf8"))),
    roles: await namedFiles(join(dir, "roles")),
    assignments: await namedFiles(join(dir, "assignments")),
    pipelines: await namedFiles(join(dir, "pipelines")),
  };
}

async function namedFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const values = await Promise.all(entries.filter((entry) => entry.isFile()).map((entry) => namedFile(dir, entry.name)));
  return Object.fromEntries(values);
}

async function namedFile(dir, name) {
  const value = YAML.parse(await readFile(join(dir, name), "utf8"));
  return [basename(name).replace(/\.(ya?ml)$/u, ""), value];
}
