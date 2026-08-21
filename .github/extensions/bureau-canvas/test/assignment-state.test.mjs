import assert from "node:assert/strict";
import test from "node:test";

import { nextExpandedAssignment } from "../web/assignment-state.js";

test("switching assignments respects an editor that refuses to close", () => {
  const cases = [
    [null, "one", true, "one"],
    ["one", "one", true, null],
    ["one", "two", true, "two"],
    ["one", "one", false, "one"],
    ["one", "two", false, "one"],
  ];

  for (const [current, requested, close, expected] of cases) {
    assert.equal(nextExpandedAssignment(current, requested, () => close), expected);
  }
});
