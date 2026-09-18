import assert from "node:assert/strict";
import { test } from "node:test";
import { hasNodeMeasurement, measuredGraphNodes } from "../web/graph-measure-state.mjs";

function internalNode(id, measured = { width: 240, height: 112 }) {
  const userNode = { id, position: { x: 0, y: 0 }, style: { width: 240 } };
  return { ...userNode, measured, internals: { userNode, handleBounds: { source: [], target: [] } } };
}

test("controlled user props need no measurements when the internal node is measured", () => {
  const node = internalNode("verify");
  assert.equal(node.internals.userNode.measured, undefined);
  assert.equal(hasNodeMeasurement(node), true);
});

test("missing, invalid, or incomplete internal measurements are not ready", () => {
  const nodes = [undefined, internalNode("missing", {}), { ...internalNode("handles"), internals: {} }];
  for (const value of [undefined, 0, -1, NaN, Infinity, "240"]) {
    nodes.push(internalNode("width", { width: value, height: 112 }));
    nodes.push(internalNode("height", { width: 240, height: value }));
  }
  assert.deepEqual(nodes.map(hasNodeMeasurement), nodes.map(() => false));
});

test("same-ID loss is observed even when the controlled props retain their identity", () => {
  const node = internalNode("verify");
  const lookup = new Map([[node.id, node]]);
  const get = (id) => lookup.get(id);
  const states = [measuredGraphNodes(["verify"], get)];
  node.measured = {};
  states.push(measuredGraphNodes(["verify"], get));
  node.measured = { width: 240, height: 112 };
  states.push(measuredGraphNodes(["verify"], get));
  assert.deepEqual(states.map((state) => state?.map((item) => item.id) ?? null), [["verify"], null, ["verify"]]);
});

test("a complete Fit target cannot silently become the measured subset", () => {
  const ids = ["concurrent:run-checks", "claim", "run-checks", "read-diff", "read-tests", "terminal:done", "terminal:escalate"];
  const partial = new Set(["run-checks", "read-diff", "read-tests"]);
  const lookup = new Map(ids.map((id) => [id, internalNode(id, partial.has(id) ? { width: 240, height: 112 } : {})]));
  const get = (id) => lookup.get(id);
  assert.equal(measuredGraphNodes(ids, get), null);
  for (const node of lookup.values()) node.measured = { width: 240, height: 112 };
  assert.deepEqual(measuredGraphNodes(ids, get).map((node) => node.id), ids);
  assert.equal(measuredGraphNodes([...ids, "not-adopted-yet"], get), null);
});

test("hidden and collapsed nodes do not hold a visible target open or enter its bounds", () => {
  const visible = internalNode("group");
  const hidden = { ...internalNode("hidden", {}), hidden: true, internals: {} };
  const collapsed = internalNode("collapsed", {});
  const lookup = new Map([visible, hidden, collapsed].map((node) => [node.id, node]));
  const get = (id) => lookup.get(id);
  assert.deepEqual(measuredGraphNodes(["group", "hidden"], get), [visible]);
  assert.deepEqual(measuredGraphNodes(["hidden"], get), []);
  assert.deepEqual(measuredGraphNodes([], get), []);
});
