import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { configView, pipelineView } from "../lib/view.mjs";
import {
  arrangementBucket,
  arrangementItemKey,
  configLayout,
  pipelineLayout,
} from "../lib/layout.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);
const referenceUrl = new URL("./fixtures/reference-payload.json", import.meta.url);
const terminalRailUrl = new URL("./fixtures/terminal-rail-payload.json", import.meta.url);

async function fixture(url) {
  return JSON.parse(await readFile(url, "utf8"));
}

async function referencePipeline() {
  return pipelineView(await fixture(referenceUrl), "fix-failing-test");
}

function itemById(items, id) {
  return items.find((item) => item.id === id);
}

function edgeBy(layout, source, outcome, target) {
  return layout.edges.find((edge) => edge.source === source && edge.outcome === outcome && edge.target === target);
}

function itemBox(item) {
  return { left: item.x, top: item.y, right: item.x + 240, bottom: item.y + 112 };
}

function intersects(left, right) {
  return left.left < right.right && left.right > right.left && left.top < right.bottom && left.bottom > right.top;
}

test("pipeline layout is deterministic byte for byte", async () => {
  const view = await referencePipeline();

  assert.equal(JSON.stringify(pipelineLayout(view)), JSON.stringify(pipelineLayout(view)));
});

test("retry edges classify as back from geometry", async () => {
  const layout = pipelineLayout(await referencePipeline());
  const retryRoutes = ["passed", "verdict", "verify"].map(
    (source) => edgeBy(layout, source, "failure", "propose").route,
  );

  assert.deepEqual(retryRoutes, ["back", "back", "back"]);
});

test("non-success edge to lower target classifies as exit", async () => {
  const layout = pipelineLayout(await referencePipeline());

  assert.equal(edgeBy(layout, "claim", "failure", "terminal:escalate").route, "exit");
});

test("terminal rail uses deepest incoming row instead of bottom pinning", async () => {
  const view = pipelineView(await fixture(terminalRailUrl), "terminal-rail");
  const layout = pipelineLayout(view);
  const claim = itemById(layout.steps, "claim");
  const verify = itemById(layout.steps, "verify");
  const escalate = itemById(layout.terminals, "terminal:escalate");

  assert.deepEqual(
    { escalateRow: escalate.row, expectedRow: claim.row + 1, bottomTerminalRow: verify.row + 1 },
    { escalateRow: 1, expectedRow: 1, bottomTerminalRow: 3 },
  );
});

test("cycle edges do not increase step depth without bound", async () => {
  const layout = pipelineLayout(await referencePipeline());
  const rows = layout.steps.map((step) => step.row);

  assert.deepEqual(
    { propose: itemById(layout.steps, "propose").row, maxRow: Math.max(...rows) },
    { propose: 2, maxRow: 7 },
  );
});

test("shuffled step input keeps the same placement", async () => {
  const view = await referencePipeline();
  const shuffled = { ...view, steps: [...view.steps].reverse(), edges: [...view.edges].reverse() };

  assert.deepEqual(pipelineLayout(shuffled), pipelineLayout(view));
});

test("arranged positions update geometric edge routes", async () => {
  const layout = pipelineLayout(await referencePipeline(), { positions: { propose: { x: 0, y: 999 } } });

  assert.equal(edgeBy(layout, "passed", "failure", "propose").route, "exit");
});

test("data and observes routes follow their relation", async () => {
  const layout = pipelineLayout(await referencePipeline());
  const routes = [
    layout.edges.find((edge) => edge.relation === "data").route,
    layout.edges.find((edge) => edge.relation === "observes").route,
  ];

  assert.deepEqual(routes, ["data", "observes"]);
});

test("config layout uses fixed columns and detached orphans", async () => {
  const payload = await fixture(committedUrl);
  payload.config.pipelines.unused = { name: "unused", steps: [] };
  const layout = configLayout(configView(payload));
  const mainMaxY = Math.max(...layout.items.filter((item) => !item.orphan).map((item) => item.y));
  const unused = itemById(layout.items, "pipeline:unused");

  assert.deepEqual(
    {
      work: itemById(layout.items, "work-source:agent-eligible").column,
      assignment: itemById(layout.items, "assignment:agent-eligible").column,
      repo: itemById(layout.items, "repo:bureau").column,
      pipeline: itemById(layout.items, "pipeline:agent-eligible-pipeline").column,
      orphanDetached: unused.orphan && unused.y > mainMaxY,
    },
    { work: 0, assignment: 1, repo: 2, pipeline: 3, orphanDetached: true },
  );
});

test("config cards reserve room for their own content", async () => {
  const payload = await fixture(committedUrl);
  // A role holding every permission is far taller than one holding two; a fixed
  // row gap is what made this overlap.
  payload.config.roles.everything = {
    name: "everything",
    agent: "/x:y",
    adapter: "copilot",
    min_trust: "maintainer",
    permissions: ["repo:read", "repo:write", "repo:push", "issues:read", "issues:write", "pr:read", "pr:write", "pr:review", "pr:merge", "runs:read", "model:invoke"],
  };
  const layout = configLayout(configView(payload));
  const overlaps = [];
  for (const left of layout.items) {
    for (const right of layout.items) {
      const collides = left.id < right.id
        && left.column === right.column
        && left.y < right.y + right.height
        && right.y < left.y + left.height;
      if (collides) {
        overlaps.push(`${left.id} / ${right.id}`);
      }
    }
  }

  assert.deepEqual(
    { overlaps, tallerThanSmall: layout.items.find((item) => item.id === "role:everything").height > layout.items.find((item) => item.id === "role:reviewer").height },
    { overlaps: [], tallerThanSmall: true },
  );
});

test("config layout gives every shared-column item a distinct cell", async () => {  const layout = configLayout(configView(await fixture(committedUrl)));
  const cells = layout.items.map((item) => `${item.column}:${item.row}`);
  const intersections = [];
  for (const [index, item] of layout.items.entries()) {
    for (const other of layout.items.slice(index + 1)) {
      if (intersects(itemBox(item), itemBox(other))) {
        intersections.push(`${item.id}/${other.id}`);
      }
    }
  }

  assert.deepEqual({ duplicateCells: cells.length - new Set(cells).size, intersections }, { duplicateCells: 0, intersections: [] });
});

test("arrangement helpers are deterministic and ignore stale entries", async () => {
  const key = arrangementItemKey(".bureau", "propose");
  const layout = pipelineLayout(await referencePipeline(), { positions: { propose: { x: 99, y: 88 }, stale: { x: 1, y: 2 } } });

  assert.deepEqual(
    { key, bucket: arrangementBucket(".bureau"), propose: itemById(layout.steps, "propose") },
    { key: ".bureau\u001fpropose", bucket: arrangementBucket(".bureau"), propose: { ...itemById(layout.steps, "propose"), x: 99, y: 88 } },
  );
});