// Scaffolds are checked against the real `bureau validate --json` rather than
// against a hand-written idea of the schema. If the CLI accepts a config the
// canvas built from empty, the scaffolds are right; asserting field shapes
// would only prove the test agrees with itself.

import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { createText, repoNames, withRepo, withoutRepo, renameReference, withDeclaredName, choicesFor, requiredFields } from "../lib/entities.mjs";
import { insertStep, orphanedBy, removeStep, scaffoldStep, scaffoldStepEdges, stepConsequences } from "../lib/steps.mjs";
import { parse, render } from "../lib/codec.mjs";
import { findings } from "../lib/findings.mjs";
import { skipWithoutBureau } from "./support/bureau-binary.mjs";

const needsBureau = await skipWithoutBureau();

const repoRoot = fileURLToPath(new URL("../../../../", import.meta.url));
// The `bureau` binary runs inside WSL, so a scratch config has to live where
// that binary can see it. `target/` is gitignored and is not a scanned config
// directory, so nothing here can be mistaken for real config.
const scratchRoot = new URL("../../../../target/canvas-crud-tests/", import.meta.url);

async function withDir(fn) {
  const dir = await mkdtemp(join(fileURLToPath(scratchRoot), "cfg-"));
  try {
    await Promise.all(["roles", "assignments", "pipelines"].map((sub) => mkdir(join(dir, sub), { recursive: true })));
    await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Builds a whole config from empty, exactly as the canvas would. */
async function buildConfig(dir) {
  await writeFile(join(dir, "repos.yaml"), withRepo("repos: {}\n", "bureau", {
    url: "https://github.com/TheLarkInn/bureau.git",
    forge: "github",
    access: "push",
    credential: "github-main",
  }));
  await writeFile(join(dir, "roles", "implementer.yaml"), createText("role", "implementer", { permissions: ["repo:read", "repo:write", "model:invoke"] }));
  await writeFile(join(dir, "pipelines", "build.yaml"), createText("pipeline", "build"));
  await writeFile(join(dir, "assignments", "work.yaml"), createText("assignment", "work", {
    work: { forge: "github", source: "TheLarkInn/bureau", filter: "is:open" },
    repos: ["bureau"],
    pipeline: "build",
    role: "implementer",
    verify: "cargo test --offline",
  }));
}

test("a config scaffolded from empty is accepted by bureau validate", { skip: needsBureau }, async () => {
  await withDir(async (dir) => {
    await buildConfig(dir);

    const result = await findings(dir, { cwd: repoRoot });

    assert.deepEqual({ state: result.state, ok: result.ok, errors: result.errors }, { state: "validated", ok: true, errors: [] });
  });
});

test("adding a repo leaves the other entries byte-identical", async () => {
  const before = withRepo("repos: {}\n", "bureau", { url: "u", forge: "github", access: "push", credential: "c" });
  const after = withRepo(before, "docs", { url: "d", forge: "github", access: "read", credential: "c2" });
  const beforeLines = before.trimEnd().split("\n");

  assert.deepEqual(
    {
      // Block style, as the committed config uses — not a single flow line.
      block: before.includes("\n  bureau:\n"),
      kept: beforeLines.every((line) => after.includes(line)),
      names: repoNames(after),
      removed: withoutRepo(after, "docs") === before,
    },
    { block: true, kept: true, names: ["bureau", "docs"], removed: true },
  );
});

test("renaming a role rewrites the declaring file and every referrer", async () => {
  const rolePath = "roles/implementer.yaml";
  const role = createText("role", "implementer");
  const assignment = createText("assignment", "work", { role: "implementer", repos: ["bureau"], pipeline: "build" });
  const pipeline = createText("pipeline", "build", {
    steps: [{ name: "propose", type: "agent", role: "implementer" }],
  });

  assert.deepEqual(
    {
      declared: withDeclaredName(role, rolePath, "builder").includes("name: builder"),
      assignment: renameReference(assignment, "assignments/work.yaml", "role", "implementer", "builder")?.includes("role: builder"),
      pipeline: renameReference(pipeline, "pipelines/build.yaml", "role", "implementer", "builder")?.includes("role: builder"),
      // A file that never referenced it is left alone entirely.
      untouched: renameReference(createText("role", "reviewer"), "roles/reviewer.yaml", "role", "implementer", "builder"),
    },
    { declared: true, assignment: true, pipeline: true, untouched: null },
  );
});

test("a scaffolded step of every kind is accepted by bureau validate", { skip: needsBureau }, async () => {
  await withDir(async (dir) => {
    await buildConfig(dir);
    const path = join(dir, "pipelines", "build.yaml");
    const parsed = parse(await readFile(path, "utf8"), { path: "pipelines/build.yaml" });
    const agent = scaffoldStep("agent", "propose", { role: "implementer" });
    const withAgent = insertStep(parsed.view, agent, 1, scaffoldStepEdges(agent, "done"));
    // Insertion does not auto-wire the predecessor: guessing where control
    // should flow would silently change the pipeline. The caller connects it,
    // and `bureau validate` reports `unreachable` if nobody does.
    withAgent.edges.push({
      id: "control:start:success->propose",
      source: "start",
      target: "propose",
      relation: "control",
      outcome: "success",
    });
    await writeFile(path, render(withAgent, parsed.doc, parsed.style));

    const result = await findings(dir, { cwd: repoRoot });

    assert.deepEqual({ state: result.state, errors: result.errors }, { state: "validated", errors: [] });
  });
});

test("an unwired inserted step is reported by the CLI, not silently accepted", { skip: needsBureau }, async () => {
  await withDir(async (dir) => {
    await buildConfig(dir);
    const path = join(dir, "pipelines", "build.yaml");
    const parsed = parse(await readFile(path, "utf8"), { path: "pipelines/build.yaml" });
    const agent = scaffoldStep("agent", "propose", { role: "implementer" });
    await writeFile(path, render(insertStep(parsed.view, agent, 1, scaffoldStepEdges(agent, "done")), parsed.doc, parsed.style));

    const result = await findings(dir, { cwd: repoRoot });

    assert.deepEqual(
      { state: result.state, message: result.errors.map((error) => error.message) },
      { state: "validated", message: ["pipeline `build` step `propose`: unreachable from `start`"] },
    );
  });
});

test("a scaffolded decision covers all four outcomes and writes no defaults", async () => {
  const decision = scaffoldStep("decision", "passed", { over: "verify" });
  const edges = scaffoldStepEdges(decision, "abort");

  assert.deepEqual(
    {
      outcomes: edges.map((candidate) => candidate.outcome).sort(),
      attempts: "maxAttempts" in decision.fields && decision.fields.maxAttempts === 1,
      wroteAbortField: JSON.stringify(decision.fields).includes("abort"),
    },
    { outcomes: ["blocked", "failure", "no-work", "success"], attempts: true, wroteAbortField: false },
  );
});

test("deleting the entry step is reported distinctly from an ordinary delete", async () => {
  const view = {
    kind: "pipeline",
    name: "p",
    steps: [scaffoldStep("deterministic", "first"), scaffoldStep("deterministic", "second")],
    edges: [{ id: "e", source: "first", target: "second", relation: "control", outcome: "success" }],
  };

  const entry = stepConsequences(view, "first");
  const ordinary = stepConsequences(view, "second");

  assert.deepEqual(
    {
      entrySeverities: entry.map((item) => item.severity),
      successor: entry.find((item) => item.severity === "entry-step")?.successor,
      ordinarySeverities: ordinary.map((item) => item.severity),
    },
    { entrySeverities: ["entry-step"], successor: "second", ordinarySeverities: ["referrer"] },
  );
});

test("removing a step reports what it orphans transitively", () => {
  const view = {
    kind: "pipeline",
    name: "p",
    steps: ["a", "b", "c", "d"].map((name) => scaffoldStep("deterministic", name)),
    edges: [
      { id: "1", source: "a", target: "b", relation: "control", outcome: "success" },
      { id: "2", source: "b", target: "c", relation: "control", outcome: "success" },
      { id: "3", source: "c", target: "d", relation: "control", outcome: "success" },
    ],
  };

  // Removing `b` strands `c`, and `d` was only reachable through `c`.
  assert.deepEqual(orphanedBy(view, "b"), ["c", "d"]);
});

test("removing a step drops its references from the surviving steps", () => {
  const view = {
    kind: "pipeline",
    name: "p",
    steps: [scaffoldStep("deterministic", "a"), { ...scaffoldStep("agent", "b", { role: "r" }), fields: { inputsFrom: ["a"], maxAttempts: 1, role: "r" } }],
    edges: [{ id: "1", source: "a", target: "b", relation: "control", outcome: "success" }],
  };

  const after = removeStep(view, "a");

  assert.deepEqual({ steps: after.steps.map((step) => step.name), inputs: after.steps[0].fields.inputsFrom, edges: after.edges }, { steps: ["b"], inputs: [], edges: [] });
});

test("closed sets and required fields mirror the Rust enums", () => {
  assert.deepEqual(
    {
      adapter: choicesFor("role", "adapter"),
      access: choicesFor("repo", "access"),
      trust: choicesFor("role", "min_trust"),
      roleRequired: requiredFields("role"),
    },
    {
      adapter: ["copilot", "claude", "fake"],
      access: ["read", "pr", "push"],
      trust: ["untrusted", "derived", "maintainer", "trusted"],
      roleRequired: ["name", "agent", "adapter", "permissions", "min_trust"],
    },
  );
});
