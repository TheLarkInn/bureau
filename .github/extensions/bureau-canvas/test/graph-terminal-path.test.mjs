import assert from "node:assert/strict";
import test from "node:test";
import { graphTerminalPath } from "../web/graph-presentation.mjs";

test("exit curves pass through their caption with horizontal, continuous tangents", () => {
  assert.equal(
    graphTerminalPath({ sourceX: 0, sourceY: 20, targetX: 1000, targetY: 120 }, 800, 70),
    "M0,20 C400,20 400,70 800,70 C900,70 900,120 1000,120",
  );
});

test("saved backward positions retain right-facing departure and left-facing arrival", () => {
  assert.equal(
    graphTerminalPath({ sourceX: 1000, sourceY: 20, targetX: 0, targetY: 120 }, -100, 70),
    "M1000,20 C1550,20 -650,70 -100,70 C-50,70 -50,120 0,120",
  );
});
