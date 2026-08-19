// web/layout.js: deterministic coordinates for the editor's step graph.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { layoutPipeline } from "../web/layout.js";
import { pipelineView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);
const referenceUrl = new URL("./fixtures/reference-payload.json", import.meta.url);

async function view(url, name) {
  return pipelineView(JSON.parse(await readFile(url, "utf8")), name);
}

test("layout is deterministic byte for byte", async () => {
  const pipeline = await view(committedUrl, "agent-eligible-pipeline");

  assert.equal(JSON.stringify(layoutPipeline(pipeline)), JSON.stringify(layoutPipeline(pipeline)));
});

test("a forward success edge places the target one layer below", async () => {
  const layout = layoutPipeline(await view(committedUrl, "agent-eligible-pipeline"));
  const byId = new Map(layout.nodes.map((node) => [node.id, node]));

  assert.deepEqual(
    [byId.get("implement").y, byId.get("verify").y, byId.get("review").y],
    [0, 190, 380],
  );
});

test("terminals sit on a rail to the right of every step", async () => {
  const layout = layoutPipeline(await view(committedUrl, "agent-eligible-pipeline"));
  const steps = layout.nodes.filter((node) => node.step);
  const terminals = layout.nodes.filter((node) => !node.step);
  const right = Math.max(...steps.map((node) => node.x));

  assert.deepEqual(
    { terminals: terminals.length, allRight: terminals.every((node) => node.x > right) },
    { terminals: 3, allRight: true },
  );
});

test("edges pass through unchanged for the editor to route", async () => {
  const pipeline = await view(referenceUrl, "fix-failing-test");
  const layout = layoutPipeline(pipeline);

  assert.deepEqual(
    { edges: layout.edges.length, source: pipeline.edges.length },
    { edges: pipeline.edges.length, source: pipeline.edges.length },
  );
});
