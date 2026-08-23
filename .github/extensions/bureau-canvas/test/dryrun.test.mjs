import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { rm, readdir, readFile } from "node:fs/promises";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { parse as parseYaml } from "../lib/vendor/yaml.mjs";
import { dryRun } from "../lib/dryrun.mjs";

const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));
const committedDir = join(repoRoot, ".bureau");
const artifactsBase = join(repoRoot, "target", "bureau-canvas-dryrun-tests");
const stub = fileURLToPath(new URL("./fixtures/dryrun-bureau.mjs", import.meta.url));
let nextArtifacts = 0;

test("committed agent-eligible pipeline dry-runs through every step", async () => {
  const before = await treeHash(committedDir);
  const events = [];
  const result = await runDry({ onEvent: (event) => events.push(event) });
  const after = await treeHash(committedDir);

  assert.deepEqual(
    {
      ok: result.ok,
      steps: result.steps,
      terminal: result.terminal,
      stepEvents: events.filter((event) => event.type === "step_started").map((event) => event.id),
      hashStable: before === after,
    },
    {
      ok: true,
      steps: ["implement", "verify", "review", "repair", "reverify", "rereview"],
      terminal: "terminal:done",
      stepEvents: ["implement", "verify", "review", "repair", "reverify", "rereview"],
      hashStable: true,
    },
  );
});
test("scratch config uses fake roles and absolute fixtures", async () => {
  const result = await runDry();
  const config = await readScratchPipeline(result);
  const roles = await readScratchRoles(result);
  const agentSteps = config.steps.filter((step) => step.type === "agent");

  assert.deepEqual(
    {
      validation: result.validation.code,
      adapters: agentSteps.map((step) => roles[step.role].adapter),
      absoluteFixtures: agentSteps.map((step) => isAbsoluteFixture(step.fixture)),
      roleNames: agentSteps.map((step) => step.role),
    },
    {
      validation: 0,
      adapters: ["fake", "fake", "fake", "fake"],
      absoluteFixtures: [true, true, true, true],
      roleNames: ["dryrun-fake", "dryrun-fake", "dryrun-fake", "dryrun-fake"],
    },
  );
});
test("failing fixture reports the terminal from the run log", async () => {
  const events = [];
  const result = await runDry({
    fixtures: { implement: { outcome: "failure", message: "forced failure" } },
    onEvent: (event) => events.push(event),
  });

  assert.deepEqual(
    {
      ok: result.ok,
      steps: result.steps,
      terminal: result.terminal,
      terminalEvents: events.filter((event) => event.type === "terminal"),
    },
    {
      ok: false,
      steps: ["implement"],
      terminal: "terminal:escalate",
      terminalEvents: [{ type: "terminal", id: "terminal:escalate", terminal: "escalate", outcome: "blocked" }],
    },
  );
});
test("next entered step comes from the run log, not the config edge", async () => {
  const result = await runDry({ item: "dryrun-log-over-config" });

  assert.deepEqual(result.steps, ["implement", "verify"]);
  assert.equal(result.terminal, "terminal:done");
});

async function runDry(options = {}) {
  const artifactsRoot = options.artifactsRoot ?? nextArtifactsRoot();
  await cleanArtifacts(artifactsRoot);
  return dryRun({
    dir: committedDir,
    pipeline: "agent-eligible-pipeline",
    item: "dryrun-item",
    binary: stub,
    artifactsRoot,
    pollMs: 1,
    ...options,
  });
}

async function readScratchPipeline(result) {
  const file = join(result.scratch.configDir, "pipelines", "agent-eligible-pipeline.yaml");
  return parseYaml(await readFile(file, "utf8"));
}

async function readScratchRoles(result) {
  const roles = {};
  for (const file of await yamlFiles(join(result.scratch.configDir, "roles"))) {
    const role = parseYaml(await readFile(file, "utf8"));
    roles[role.name] = role;
  }
  return roles;
}

async function yamlFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && [".yaml", ".yml"].includes(extname(entry.name)))
    .map((entry) => join(dir, entry.name));
}

function isAbsoluteFixture(path) {
  return path.startsWith("/") || /^[A-Za-z]:[\\/]/u.test(path) || path.startsWith("\\\\");
}

function nextArtifactsRoot() {
  nextArtifacts += 1;
  return join(artifactsBase, `case-${nextArtifacts}`);
}

async function cleanArtifacts(artifactsRoot) {
  await rm(artifactsRoot, { recursive: true, force: true });
}

async function treeHash(dir) {
  const hash = createHash("sha256");
  for (const file of await filesUnder(dir)) {
    hash.update(relative(dir, file));
    hash.update("\0");
    hash.update(await readFile(file));
    hash.update("\0");
  }
  return hash.digest("hex");
}

async function filesUnder(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = join(dir, entry.name);
      return entry.isDirectory() ? filesUnder(path) : [path];
    }),
  );
  return nested.flat().sort();
}
