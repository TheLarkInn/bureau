import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";

const canvas = await import("../extension.mjs");
const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));
const bureauStub = fileURLToPath(new URL("./fixtures/findings-bureau.mjs", import.meta.url));
const validDir = fileURLToPath(new URL("./fixtures/findings-valid/.bureau", import.meta.url));
const referenceUrl = new URL("./fixtures/reference-payload.json", import.meta.url);

async function payloadFixture(url) {
  return JSON.parse(await readFile(url, "utf8"));
}

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


test("vendors renderer modules locally and serves a fallback", async () => {
  const html = await readFile(new URL("../web/index.html", import.meta.url), "utf8");
  const vendorDir = new URL("../web/vendor/", import.meta.url);
  const vendored = ["react.mjs", "react-jsx-runtime.mjs", "react-dom.mjs", "react-dom-client.mjs", "xyflow-react.mjs", "xyflow-react.css"];
  const present = await Promise.all(vendored.map((name) => stat(new URL(name, vendorDir)).then((info) => info.size > 0, () => false)));

  assert.deepEqual(
    {
      mapped: vendored.map((name) => html.includes(`./vendor/${name}`)),
      present,
      // A CDN reference means the panel would need network to draw.
      remote: html.includes("esm.sh") || html.includes("https://"),
      fallback: ["Bureau renderer could not start", "window.addEventListener(\"error\"", "unhandledrejection", "await import(\"./app.mjs\")", "Config dir"].map((text) => html.includes(text)),
    },
    {
      mapped: vendored.map(() => true),
      present: vendored.map(() => true),
      remote: false,
      fallback: [true, true, true, true, true],
    },
  );
});

/**
 * The Edge harness must not gate itself on a network it no longer uses.
 *
 * A preflight to esm.sh stood in `skipReason` from when the pages loaded React
 * from a CDN. The renderer was vendored an hour later — the test above asserts
 * the page carries no remote reference at all — and the preflight stayed,
 * turning "this machine is offline" into "skip the whole browser suite and
 * exit 0". A harness may name a pairing it cannot run; it may not pass by
 * running nothing, so the absence is pinned rather than left to be re-added.
 *
 * Both halves of that are pinned, because either alone is escapable. Removing
 * the URL still leaves `async function skipReason()` free to `await` a bare
 * hostname through `dns.lookup`; and pinning only the call site measures the
 * symptom rather than the defect, since an `async` declaration under an
 * un-awaited call returns a promise, `if (skip)` takes it as truthy, and the
 * harness prints `skipped: [object Promise]` and exits 0 — the exact silence
 * this test exists to prevent, wearing a green tick.
 */
test("the Edge harness gates on the browser, never on the network", async () => {
  const harness = await readFile(new URL("../e2e/run.mjs", import.meta.url), "utf8");
  const remote = [...harness.matchAll(/["'`](https?:\/\/[^"'`]+)["'`]/gu)].map((match) => match[1]);

  assert.deepEqual(
    {
      // Loopback is the DevTools endpoint; a fixture URL is a string the
      // harness types into a form, not something it fetches. Both allowances
      // end at a host boundary, so they admit the host and not merely names
      // that begin with it.
      offHost: remote.filter((url) => !/^https?:\/\/(?:127\.0\.0\.1|localhost|x)(?:[:/]|$)/u.test(url)),
      declaredSynchronous: /\nfunction skipReason\(\) \{/u.test(harness),
      calledSynchronously: /\n\s*const skip = skipReason\(\);/u.test(harness),
    },
    { offHost: [], declaredSynchronous: true, calledSynchronously: true },
  );
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
        markup: ["@xyflow/react", "react-dom/client", "--true-color-blue", "app.mjs"].map((text) => page.includes(text)),
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

test("pipeline intent selects a pipeline that carries its own step diagram", async () => {
  const instance = await openInstance("bureau-render-intent-test");

  try {
    const response = await fetch(new URL("/intent", instance.opened.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "open-pipeline", pipeline: "agent-eligible-pipeline" }),
    });
    const result = await response.json();
    const state = await fetch(new URL("/state", instance.opened.url)).then((reply) => reply.json());
    const drawn = state.pipelines["agent-eligible-pipeline"];

    assert.deepEqual(
      {
        ok: result.ok,
        selected: result.state.selectedPipeline,
        persisted: state.selectedPipeline,
        steps: drawn.view.steps.length,
        routed: drawn.layout.edges.every((edge) => Boolean(edge.route)),
      },
      {
        ok: true,
        selected: { name: "agent-eligible-pipeline", missing: false },
        persisted: { name: "agent-eligible-pipeline", missing: false },
        steps: 3,
        routed: true,
      },
    );
  } finally {
    await instance.close();
  }
});


test("committed pipeline view keeps absent branches absent", async () => {
  const instance = await openInstance("bureau-render-committed-pipeline-test", { pipeline: "agent-eligible-pipeline" });

  try {
    const page = await fetch(instance.opened.url).then((response) => response.text());
    const state = await fetch(new URL("/state", instance.opened.url)).then((response) => response.json());
    const pipeline = state.pipelines["agent-eligible-pipeline"];
    const verifyEdges = pipeline.layout.edges.filter((edge) => edge.source === "verify" && edge.relation === "control");

    assert.deepEqual(
      {
        selected: state.selectedPipeline.name,
        steps: pipeline.layout.steps.length,
        reachedTerminals: pipeline.view.terminals.filter((terminal) => pipeline.layout.edges.some((edge) => edge.target === terminal.id)).length,
        verifyOutcomes: verifyEdges.map((edge) => edge.outcome).sort(),
        markup: ["flow-edge--data", "flow-edge--observes", "terminal-pill"].map((text) => page.includes(text)),
      },
      {
        selected: "agent-eligible-pipeline",
        steps: 3,
        reachedTerminals: 2,
        verifyOutcomes: ["failure", "success"],
        markup: [true, true, true],
      },
    );
  } finally {
    await instance.close();
  }
});

test("reference pipeline state carries retry routes and concurrent membership", async () => {
  const payload = await payloadFixture(referenceUrl);
  const instance = await openInstance(
    "bureau-render-reference-pipeline-test",
    { pipeline: "fix-failing-test" },
    { payload },
  );

  try {
    const state = await fetch(new URL("/state", instance.opened.url)).then((response) => response.json());
    const pipeline = state.pipelines["fix-failing-test"];
    const retries = pipeline.layout.edges
      .filter((edge) => edge.target === "propose" && ["passed", "verdict", "verify"].includes(edge.source))
      .map((edge) => `${edge.source}:${edge.route}`)
      .sort();
    const passedOutcomes = pipeline.layout.edges
      .filter((edge) => edge.source === "passed" && edge.relation === "control")
      .map((edge) => edge.outcome)
      .sort();

    assert.deepEqual(
      {
        steps: pipeline.layout.steps.length,
        concurrentMembers: pipeline.layout.steps.filter((step) => step.parentId === "run-checks").map((step) => step.id).sort(),
        memberControlEdges: pipeline.layout.edges.filter((edge) => ["apply", "review"].includes(edge.source) && edge.relation === "control").length,
        retries,
        passedOutcomes,
        relations: [...new Set(pipeline.layout.edges.map((edge) => edge.relation))].sort(),
      },
      {
        steps: 9,
        concurrentMembers: ["apply", "review"],
        memberControlEdges: 0,
        retries: ["passed:back", "verdict:back", "verify:back"],
        passedOutcomes: ["blocked", "failure", "no-work", "success"],
        relations: ["control", "data", "observes"],
      },
    );
  } finally {
    await instance.close();
  }
});

test("back to config intent clears selected pipeline", async () => {
  const instance = await openInstance("bureau-render-back-test", { pipeline: "agent-eligible-pipeline" });

  try {
    const response = await fetch(new URL("/intent", instance.opened.url), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "back-to-config" }),
    });
    const result = await response.json();

    assert.deepEqual({ ok: result.ok, pipeline: result.state.pipeline, selected: result.state.selectedPipeline }, { ok: true, pipeline: null, selected: null });
  } finally {
    await instance.close();
  }
});

test("focus and reload keep the selected pipeline subject", async () => {
  const instanceId = "bureau-render-actions-selected-test";
  const instance = await openInstance(instanceId, { pipeline: "agent-eligible-pipeline" });
  const actions = canvas.canvasActions();

  try {
    const focus = await actions.find((action) => action.name === "focus").handler({ instanceId, input: { kind: "step", name: "verify" } });
    const reload = await actions.find((action) => action.name === "reload").handler({ instanceId, input: {} });
    const state = await fetch(new URL("/state", instance.opened.url)).then((response) => response.json());

    assert.deepEqual(
      { focusSubject: focus.subject.pipeline, reloadScope: reload.scope, reloadPipeline: reload.subject.pipeline, selected: state.selectedPipeline.name },
      { focusSubject: "agent-eligible-pipeline", reloadScope: "pipeline", reloadPipeline: "agent-eligible-pipeline", selected: "agent-eligible-pipeline" },
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