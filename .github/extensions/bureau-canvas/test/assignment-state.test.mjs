import assert from "node:assert/strict";
import test from "node:test";

import { DIRTY_FIELD_EDITORS, FIELD_EDITORS, nextExpandedAssignment } from "../web/assignment-state.js";

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

/**
 * The guard warns about unsaved work, so every clause has to be about unsaved
 * work. Dropping the predicate from one — or adding an editor to the list
 * without it — takes that editor back to prompting whenever it is merely open,
 * which is the defect this selector was built to remove and which no
 * screenshot can show.
 */
test("every editor the leave guard watches is matched only while it is dirty", () => {
  const clauses = DIRTY_FIELD_EDITORS.split(", ");
  const covered = FIELD_EDITORS.every((editor) => clauses.includes(`${editor}[data-dirty="true"]`));

  assert.deepEqual(
    [clauses.length, covered, clauses.every((clause) => clause.endsWith('[data-dirty="true"]'))],
    [FIELD_EDITORS.length, true, true],
  );
});
