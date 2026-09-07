import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { pipelineLayout } from "../lib/layout.mjs";
import { pipelineView } from "../lib/view.mjs";
import {
  GRAPH_STATE_LABELS, graphEdgeCaption, graphEdgeLabels, graphGeometry, graphStepState, initialGraphViewport, needsAttention,
  nextAttention, searchGraphItems,
} from "../web/graph-presentation.mjs";
import { layoutPipeline } from "../web/layout.js";
import { applyEvents, emptyOverlay, resolveOverlay } from "../web/live/overlay.js";

const FIXTURES = new URL("./fixtures/", import.meta.url);

function freeze(value) {
  Object.values(value).filter((item) => item && typeof item === "object").forEach(freeze);
  return Object.freeze(value);
}

async function pipelineFixture() {
  const payload = JSON.parse(await readFile(new URL("committed-payload.json", FIXTURES), "utf8"));
  const view = pipelineView(payload, "agent-eligible-pipeline");
  return { view, layout: pipelineLayout(view), containers: [] };
}

async function eventsFixture(run) {
  const text = await readFile(new URL(`runs/${run}/events.jsonl`, FIXTURES), "utf8");
  return text.trim().split("\n").map((line) => JSON.parse(line));
}

test("graph state preserves every overlay outcome and never infers a run from a design", () => {
  for (const state of Object.keys(GRAPH_STATE_LABELS)) {
    const node = freeze({ className: `overlay-${state}` });
    assert.deepEqual(
      [graphStepState(node, "live"), graphStepState(node, "replay"), graphStepState(node)],
      [state, state, "design"],
    );
  }
});

test("no run, missing decorations, and unfamiliar outcomes remain unknown", () => {
  for (const node of [null, undefined, {}, { className: "" }, { className: "overlay-unrecognized" }]) {
    assert.deepEqual([graphStepState(node, "live"), graphStepState(node, "replay")], ["unknown", "unknown"]);
  }
  assert.equal(GRAPH_STATE_LABELS.unknown, "No run");
});

test("a paused boundary overrides a completed outcome only in live and replay", () => {
  const node = freeze({ className: "overlay-success", paused: true });
  assert.deepEqual(
    [graphStepState(node), graphStepState(node, "live"), graphStepState(node, "replay")],
    ["design", "paused", "paused"],
  );
});

test("real log reductions distinguish no run, running, pending, and a paused boundary", async () => {
  const pipeline = await pipelineFixture();
  const results = [];
  for (const run of [null, "run-live", "run-paused"]) {
    const overlay = run ? applyEvents(await eventsFixture(run)) : emptyOverlay();
    const resolved = resolveOverlay(pipeline, overlay);
    results.push(resolved.nodes.map((node) => [node.id, graphStepState(node, "live")]));
  }
  assert.deepEqual(results, [
    [["implement", "unknown"], ["verify", "unknown"], ["review", "unknown"]],
    [["implement", "running"], ["verify", "pending"], ["review", "pending"]],
    [["implement", "paused"], ["verify", "pending"], ["review", "pending"]],
  ]);
});

test("attention means failure, blocked, paused, or actual findings, not unfinished work", () => {
  for (const state of [...Object.keys(GRAPH_STATE_LABELS), undefined, "unrecognized"]) {
    assert.deepEqual(
      [needsAttention(state), needsAttention(state, []), needsAttention(state, [{ message: "Missing run command" }])],
      [["failure", "blocked", "paused"].includes(state), ["failure", "blocked", "paused"].includes(state), true],
    );
  }
});

const ITEMS = freeze([
  { id: "detail", name: "compile", kind: "deterministic", detail: "verify build", state: "pending" },
  { id: "contains", name: "reverify", kind: "agent", detail: "role: reviewer", state: "success" },
  { id: "prefix", name: "verify tests", kind: "deterministic", detail: "cargo test --offline", state: "failure", attention: true },
  { id: "prefix-two", name: "verify policy", kind: "decision", detail: "over: compile", state: "blocked", attention: true },
  { id: "literal", name: '<check "a&b"> [.*]+?', kind: "agent", state: "design" },
]);

test("search ranks title prefixes before title substrings and details, stably and case-insensitively", () => {
  assert.deepEqual(searchGraphItems(ITEMS, "  VeRiFy  ").map((item) => item.id),
    ["prefix", "prefix-two", "contains", "detail"]);
  assert.equal(searchGraphItems(ITEMS, "verify")[0], ITEMS[2]);
});

test("search matches type, detail, state, and readable status labels", () => {
  for (const [query, ids] of [
    ["deterministic", ["detail", "prefix"]],
    ["cargo test --offline", ["prefix"]],
    ["failure", ["prefix"]],
    ["failed", ["prefix"]],
    ["blocked", ["prefix-two"]],
    ["role: reviewer", ["contains"]],
  ]) {
    assert.deepEqual(searchGraphItems(ITEMS, query).map((item) => item.id), ids);
  }
});

test("arbitrary search text is literal, missing matches stay empty, and an empty query restores all items", () => {
  for (const query of ["[.*]+?", '<check "a&b">']) {
    assert.deepEqual(searchGraphItems(ITEMS, query).map((item) => item.id), ["literal"]);
  }
  assert.deepEqual(searchGraphItems(ITEMS, "no-such-step"), []);
  assert.deepEqual(searchGraphItems([], "verify"), []);
  assert.deepEqual(searchGraphItems(ITEMS, " \n\t "), ITEMS);
});

test("next attention cycles only flagged items and safely handles missing or empty selection", () => {
  assert.deepEqual(
    [null, "missing", "contains", "prefix", "prefix-two"].map((id) => nextAttention(ITEMS, id)?.id),
    ["prefix", "prefix", "prefix", "prefix-two", "prefix"],
  );
  assert.equal(nextAttention([], "missing"), null);
  assert.equal(nextAttention(ITEMS.filter((item) => !item.attention), "prefix"), null);
});

test("viewer uses the editor's exact left-to-right coordinates, IDs, and terminal placement", async () => {
  const pipeline = freeze(await pipelineFixture());
  const actual = graphGeometry(pipeline);
  const expected = layoutPipeline(pipeline.view);
  const coordinates = (nodes) => nodes.map(({ id, x, y }) => ({ id, x, y }));
  assert.deepEqual(coordinates([...actual.steps, ...actual.terminals]), coordinates(expected.nodes));
  assert.deepEqual(actual.steps.map(({ id, x, y }) => [id, x, y]),
    [["implement", 0, 0], ["verify", 300, 0], ["review", 600, 0]]);
  assert.deepEqual(actual.edges, pipeline.layout.edges);
});

test("saved positions stay authoritative without changing step identities, edges, membership, or the input", () => {
  const steps = [
    { id: "checks", name: "checks", kind: "concurrent", order: 0, fields: { members: ["tests", "review"] }, x: 0, y: 0 },
    { id: "tests", name: "tests", kind: "deterministic", order: 1, parentId: "checks", fields: {}, x: 0, y: 100 },
    { id: "review", name: "review", kind: "agent", order: 2, parentId: "checks", fields: {}, x: 0, y: 200 },
  ];
  const terminals = [{ id: "terminal:done", name: "done", x: 100, y: 300 }];
  const edges = [{ id: "checks:success", source: "checks", target: "terminal:done", relation: "control", outcome: "success" }];
  const pipeline = freeze({
    view: { steps, terminals, edges }, layout: { name: "check-change", steps, terminals, edges },
    containers: [{ id: "frame:checks", parent: "checks", members: ["tests", "review"], x: 0, y: 0 }],
    arrangement: { checks: { x: -20, y: 35 }, tests: { x: 400, y: 40 }, "terminal:done": { x: 880, y: 60 } },
  });
  const before = JSON.stringify(pipeline);
  const result = graphGeometry(pipeline);
  assert.deepEqual(result.steps.map(({ id, x, y, parentId }) => [id, x, y, parentId]),
    [["checks", -20, 35, undefined], ["tests", 400, 40, "checks"], ["review", 300, 170, "checks"]]);
  assert.deepEqual(result.terminals, [{ id: "terminal:done", name: "done", x: 880, y: 60 }]);
  assert.deepEqual(result.containers, [
    { id: "frame:checks", parent: "checks", members: ["tests", "review"], x: -20, y: 35, width: 420, height: 135 },
  ]);
  assert.deepEqual(result.edges, edges);
  assert.equal(JSON.stringify(pipeline), before);
});

test("missing pipeline produces an empty graph instead of fabricated steps", () => {
  assert.deepEqual(graphGeometry(undefined), { steps: [], terminals: [], edges: [], containers: [] });
});

test("initial compact framing keeps wide graphs legible and their first column reachable", () => {
  const bounds = freeze({ x: -120, y: 40, width: 2000, height: 500 });
  for (const width of [390, 760]) {
    const viewport = initialGraphViewport(bounds, width, 600);
    assert.deepEqual(
      { zoom: viewport.zoom, left: viewport.x + bounds.x * viewport.zoom },
      { zoom: 0.8, left: 32 },
    );
    assert.ok(viewport.y + bounds.y * viewport.zoom >= 72);
    assert.ok(viewport.x + (bounds.x + bounds.width) * viewport.zoom > width);
  }
});

test("initial framing never magnifies a small graph or produces invalid empty bounds", () => {
  for (const bounds of [{ x: 20, y: -40, width: 200, height: 100 }, { x: 0, y: 0, width: 0, height: 0 }]) {
    const viewport = initialGraphViewport(freeze(bounds), 1280, 900);
    assert.equal(viewport.zoom, 1);
    assert.ok(Object.values(viewport).every(Number.isFinite));
  }
});

test("framing accounts for positive and negative bounds origins without changing screen placement", () => {
  for (const size of [{ width: 200, height: 100 }, { width: 2000, height: 1000 }]) {
    const baseline = initialGraphViewport({ x: 0, y: 0, ...size }, 760, 600);
    for (const [x, y] of [[-320, 140], [480, -160]]) {
      const viewport = initialGraphViewport(freeze({ x, y, ...size }), 760, 600);
      assert.deepEqual(
        { x: viewport.x + x * viewport.zoom, y: viewport.y + y * viewport.zoom, zoom: viewport.zoom },
        baseline,
      );
    }
  }
});

test("parallel captions are centered and spaced without changing edge identity, metadata, or input", () => {
  const edges = freeze([
    { id: "first", source: "implement", target: "verify", data: { label: "success", route: "spine" } },
    { id: "second", source: "implement", target: "verify", data: { label: "failure", route: "back" } },
    { id: "third", source: "implement", target: "verify", data: { label: "blocked" } },
    { id: "separate", source: "review", target: "verify", data: { label: "success" } },
  ]);
  const before = JSON.stringify(edges);
  const labeled = graphEdgeLabels(edges);
  assert.deepEqual(labeled.map(({ id, data }) => [id, data.labelOffset, data.terminalLabel]),
    [["first", -28, false], ["second", 0, false], ["third", 28, false], ["separate", 0, false]]);
  assert.deepEqual(labeled.map(({ id, source, target, data: { labelOffset, terminalLabel, terminalColumn, ...data } }) =>
    ({ id, source, target, data })), edges);
  assert.equal(JSON.stringify(edges), before);
  assert.notEqual(labeled[0].data, edges[0].data);
});

test("terminal captions share a target rail across sources with deterministic terminal columns", () => {
  const edges = freeze([
    { id: "done-a", source: "implement", target: "terminal:done", data: { label: "no-work" } },
    { id: "abort", source: "verify", target: "terminal:abort", data: { label: "failure" } },
    { id: "done-b", source: "review", target: "terminal:done", data: { label: "success" } },
  ]);
  const labeled = graphEdgeLabels(edges);
  assert.deepEqual(labeled.map(({ id, data }) => [id, data.labelOffset, data.terminalLabel, data.terminalColumn]),
    [["done-a", -14, true, 1], ["abort", 0, true, 0], ["done-b", 14, true, 1]]);
  assert.deepEqual(labeled.map(({ data }) => graphEdgeCaption({
    data, labelX: 300, labelY: 100, targetX: 1000, targetY: 400,
  })), [[872, 386], [928, 400], [872, 414]]);
  assert.deepEqual(graphEdgeLabels([]), []);
});

test("ordinary captions retain path coordinates while terminal captions use their destination rail", () => {
  for (const [data, expected] of [
    [undefined, [250, 120]],
    [{ labelOffset: -14, terminalLabel: false }, [250, 106]],
    [{ labelOffset: 28, terminalLabel: true, terminalColumn: 0 }, [728, 628]],
    [{ labelOffset: 0, terminalLabel: true, terminalColumn: 1 }, [672, 600]],
  ]) {
    const args = freeze({ data, labelX: 250, labelY: 120, targetX: 800, targetY: 600 });
    assert.deepEqual(graphEdgeCaption(args), expected);
  }
});
