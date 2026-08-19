// The save path's round-trip guarantee (DESIGN.md review question Q4): an
// edited pipeline is rendered, written, and validated; findings that name
// the edited pipeline put the original bytes back. The validator is mocked —
// these tests never shell out.

import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { arrangementFor, readLayout, savePipeline, writeLayout } from "../lib/pipeline.mjs";
import { editable, setEdge } from "../lib/edit.mjs";
import { pipelineView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);
const pipelineFileUrl = new URL("./fixtures/pipeline-roundtrip/agent-eligible-pipeline.yaml", import.meta.url);
const layoutName = "layout.json";

async function fixtureView() {
  const payload = JSON.parse(await readFile(committedUrl, "utf8"));
  return pipelineView(payload, "agent-eligible-pipeline");
}

async function fixtureText() {
  return readFile(pipelineFileUrl, "utf8");
}

/** An in-memory filesystem: one pipeline file plus the layout sidecar. */
async function memoryFs(extra = {}) {
  const files = new Map([[pipelinePath(), await fixtureText()]]);
  return {
    files,
    deps: {
      readText: (path) => (files.has(path) ? Promise.resolve(files.get(path)) : Promise.reject(new Error(`missing ${path}`))),
      writeText: (path, text) => {
        files.set(path, text);
        return Promise.resolve();
      },
      ...extra,
    },
  };
}

function fixtureDirPath() {
  return fileURLToPath(new URL("./fixtures/pipeline-roundtrip", import.meta.url));
}

function pipelinePath() {
  return join(fixtureDirPath(), "pipelines", "agent-eligible-pipeline.yaml");
}

function cleanValidation() {
  return { ok: true, state: "validated", errors: [], findings: [] };
}

function pipelineFinding(pipeline, message) {
  return {
    source: "bureau-validate",
    marker: "validation",
    path: `pipelines/${pipeline}.yaml`,
    message,
    target: { kind: "pipeline", pipeline },
  };
}

function inputFor(view, layout = null) {
  return { dir: fixtureDirPath(), pipeline: "agent-eligible-pipeline", view, layout };
}

test("save writes the rendered pipeline when validation is clean", async () => {
  const { files, deps } = await memoryFs({ validate: () => Promise.resolve(cleanValidation()) });
  const view = editable(await fixtureView());
  const edited = setEdge(view, "verify", "failure", "abort");

  const result = await savePipeline(inputFor(edited), deps);

  assert.deepEqual(
    { saved: result.saved, findings: result.findings.length, rewritten: files.get(pipelinePath()).includes("on_failure: abort") },
    { saved: true, findings: 0, rewritten: true },
  );
});

test("save reverts the file when findings name the edited pipeline", async () => {
  const original = await fixtureText();
  const validate = () => Promise.resolve({ ...cleanValidation(), findings: [pipelineFinding("agent-eligible-pipeline", "broken")] });
  const { files, deps } = await memoryFs({ validate });
  const view = editable(await fixtureView());

  const result = await savePipeline(inputFor(setEdge(view, "verify", "failure", "abort")), deps);

  assert.deepEqual({ saved: result.saved, findings: result.findings.length, restored: files.get(pipelinePath()) === original }, {
    saved: false,
    findings: 1,
    restored: true,
  });
});

test("findings from another pipeline do not block the save", async () => {
  const validate = () => Promise.resolve({ ...cleanValidation(), findings: [pipelineFinding("other-pipeline", "broken elsewhere")] });
  const { files, deps } = await memoryFs({ validate });
  const view = editable(await fixtureView());
  const before = files.get(pipelinePath());

  const result = await savePipeline(inputFor(setEdge(view, "verify", "failure", "abort")), deps);

  assert.deepEqual({ saved: result.saved, changed: files.get(pipelinePath()) !== before }, { saved: true, changed: true });
});

test("a decision edited only through its `on` map round-trips", async () => {
  const { files, deps } = await memoryFs({ validate: () => Promise.resolve(cleanValidation()) });
  let view = editable(await fixtureView());
  view = {
    ...view,
    steps: [
      ...view.steps,
      { id: "gate", name: "gate", type: "step", kind: "decision", order: 2, fields: { over: "verify", on: { success: "done", failure: "implement", blocked: "escalate", "no-work": "done" } } },
    ],
  };

  const result = await savePipeline(inputFor(view), deps);
  const written = files.get(pipelinePath());

  assert.deepEqual(
    [result.saved, written.includes("type: decision"), written.includes("no-work: done"), /on:\s*\n\s+success: done/u.test(written)],
    [true, true, true, true],
  );
});

test("layout sidecar round-trips positions per pipeline", async () => {
  const { deps } = await memoryFs();
  await writeLayout(fixtureDirPath(), "agent-eligible-pipeline", { implement: { x: 40, y: 0 }, verify: { x: 360, y: 190 } }, deps);
  await writeLayout(fixtureDirPath(), "other-pipeline", { start: { x: 0, y: 0 } }, deps);

  const layouts = await readLayout(fixtureDirPath(), deps);

  assert.deepEqual(
    { pipelines: Object.keys(layouts).sort(), implement: layouts["agent-eligible-pipeline"]?.steps?.implement },
    { pipelines: ["agent-eligible-pipeline", "other-pipeline"], implement: { x: 40, y: 0 } },
  );
});

test("layout sidecar drops invalid positions and tolerates a missing file", async () => {
  const { deps } = await memoryFs();
  await writeLayout(fixtureDirPath(), "p", { good: { x: 1, y: 2 }, bad: { x: Number.NaN, y: 0 } }, deps);
  const layouts = await readLayout(fixtureDirPath(), deps);
  const missing = await readLayout(fixtureDirPath(), { readText: () => Promise.reject(new Error("nope")) });

  assert.deepEqual(
    { kept: arrangementFor(layouts, "p"), missing },
    { kept: { good: { x: 1, y: 2 } }, missing: {} },
  );
});

test("save persists positions together with the pipeline", async () => {
  const { deps } = await memoryFs({ validate: () => Promise.resolve(cleanValidation()) });
  const view = editable(await fixtureView());
  const layout = { implement: { x: 10, y: 20 }, verify: { x: 400, y: 200 } };

  await savePipeline(inputFor(view, layout), deps);
  const layouts = await readLayout(fixtureDirPath(), deps);

  assert.deepEqual(arrangementFor(layouts, "agent-eligible-pipeline"), layout);
});

test("save refuses a view whose name does not match the target pipeline", async () => {
  const { deps } = await memoryFs({ validate: () => Promise.resolve(cleanValidation()) });
  const view = { ...(await fixtureView()), name: "renamed-elsewhere" };

  await assert.rejects(() => savePipeline(inputFor(view), deps), /does not match/u);
});

test("save throws when the pipeline file is missing", async () => {
  const { deps } = await memoryFs({ validate: () => Promise.resolve(cleanValidation()) });
  deps.readText = () => Promise.reject(new Error("gone"));
  const view = editable(await fixtureView());

  await assert.rejects(() => savePipeline(inputFor(view), deps), /has no file/u);
});

test("layout positions survive a save via the real filesystem", async () => {
  const { mkdtemp, mkdir, rm } = await import("node:fs/promises");
  const scratch = join(process.cwd(), "target");
  await mkdir(scratch, { recursive: true });
  const root = await mkdtemp(join(scratch, "canvas-pipeline-test-"));
  try {
    const dir = join(root, ".bureau");
    await mkdir(join(dir, "pipelines"), { recursive: true });
    await writeFile(join(dir, "pipelines", "agent-eligible-pipeline.yaml"), await fixtureText(), "utf8");
    const view = editable(await fixtureView());

    await savePipeline(
      { dir, pipeline: "agent-eligible-pipeline", view, layout: { implement: { x: 3, y: 4 } } },
      { validate: () => Promise.resolve(cleanValidation()) },
    );

    const onDisk = JSON.parse(await readFile(join(dir, layoutName), "utf8"));
    assert.deepEqual(onDisk.pipelines["agent-eligible-pipeline"].steps, { implement: { x: 3, y: 4 } });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
