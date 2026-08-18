import assert from "node:assert/strict";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";

const canvas = await import("../extension.mjs");
const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));
const bureauStub = fileURLToPath(new URL("./fixtures/findings-bureau.mjs", import.meta.url));
const validDir = fileURLToPath(new URL("./fixtures/findings-valid/.bureau", import.meta.url));

async function openInstance(instanceId, input = {}, options = {}) {
  const opened = await canvas.openBureauCanvas({ instanceId, input }, options);
  return {
    opened,
    close: () => canvas.closeBureauCanvas({ instanceId }),
  };
}

function normalized(path) {
  return path.replaceAll("\\", "/");
}

test("relative config dir resolves against the repo root", () => {
  const resolved = canvas.resolveInput({}, "C:\\Users\\selarkin\\.copilot\\session-state\\abc");

  assert.deepEqual(
    {
      dir: resolved.dir,
      repoRoot: resolved.repoRoot,
      underSessionState: normalized(resolved.dir).includes("/.copilot/session-state/"),
    },
    { dir: resolve(repoRoot, ".bureau"), repoRoot: resolve(repoRoot), underSessionState: false },
  );
});

test("absolute config dir is honored", () => {
  assert.equal(canvas.resolveInput({ dir: validDir }).dir, validDir);
});

test("serves the config renderer markup and fallback state", async () => {
  const instance = await openInstance("bureau-render-state-test");

  try {
    const page = await fetch(instance.opened.url).then((response) => response.text());
    const state = await fetch(new URL("/state", instance.opened.url)).then((response) => response.json());
    const implementer = state.config.view.roles.find((role) => role.name === "implementer");
    const pipeline = state.pipelines["agent-eligible-pipeline"].summary;

    assert.deepEqual(
      {
        markup: ["config-view", "drill-down", "--true-color-blue", "app.mjs"].map((text) => page.includes(text)),
        status: state.status,
        reason: state.validation.message,
        roles: pipeline.agentSteps.map((step) => step.role).sort(),
        permissions: implementer.permissions,
        repoAccess: state.config.view.repos[0].access,
        primaryRepo: state.config.view.assignments[0].primaryRepo,
        orphans: state.config.view.orphans.length,
      },
      {
        markup: [true, true, true, true],
        status: "Showing bundled sample; bureau binary not available.",
        reason: "Showing bundled sample; bureau binary not available.",
        roles: ["implementer", "reviewer"],
        permissions: ["repo:read", "repo:write", "model:invoke"],
        repoAccess: "push",
        primaryRepo: "bureau",
        orphans: 0,
      },
    );
  } finally {
    await instance.close();
  }
});

test("missing config dir also falls back with a plain status", async () => {
  const instance = await openInstance("bureau-render-missing-dir-test", { dir: "missing-config-dir" });

  try {
    const state = await fetch(new URL("/state", instance.opened.url)).then((response) => response.json());

    assert.deepEqual(
      { status: state.status, assignments: state.config.view.assignments.length, reason: state.validation.message },
      {
        status: "Showing bundled sample; config directory not found.",
        assignments: 1,
        reason: "Showing bundled sample; config directory not found.",
      },
    );
  } finally {
    await instance.close();
  }
});

test("real validate payload does not use the fixture", async () => {
  const instance = await openInstance(
    "bureau-render-real-payload-test",
    { dir: validDir },
    { findingsOptions: { binary: bureauStub } },
  );

  try {
    const state = await fetch(new URL("/state", instance.opened.url)).then((response) => response.json());

    assert.deepEqual(
      { status: state.status, validationState: state.validation.state, assignments: state.config.view.assignments.length },
      { status: "Validated", validationState: "validated", assignments: 0 },
    );
  } finally {
    await instance.close();
  }
});

test("pipeline intent updates state with drill-down placeholder", async () => {
  const instance = await openInstance("bureau-render-intent-test");

  try {
    const response = await fetch(new URL("/intent", instance.opened.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "open-pipeline", pipeline: "agent-eligible-pipeline" }),
    });
    const result = await response.json();
    const state = await fetch(new URL("/state", instance.opened.url)).then((reply) => reply.json());

    assert.deepEqual(
      {
        ok: result.ok,
        selected: result.state.selectedPipeline,
        persisted: state.selectedPipeline,
      },
      {
        ok: true,
        selected: {
          name: "agent-eligible-pipeline",
          missing: false,
          placeholder: "Pipeline drill-down will render here in a later issue.",
        },
        persisted: {
          name: "agent-eligible-pipeline",
          missing: false,
          placeholder: "Pipeline drill-down will render here in a later issue.",
        },
      },
    );
  } finally {
    await instance.close();
  }
});

test("request handling still refuses unrelated POSTs and path traversal", async () => {
  const instance = await openInstance("bureau-render-security-test");

  try {
    const post = await fetch(instance.opened.url, { method: "POST" });
    const traversal = await fetch(`${instance.opened.url}%2e%2e/%2e%2e/etc/passwd`);

    assert.deepEqual([post.status, traversal.status], [405, 404]);
  } finally {
    await instance.close();
  }
});