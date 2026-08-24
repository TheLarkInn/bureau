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
 * work.
 *
 * What this can catch is the selector losing its predicate — being rewritten
 * as a bare `FIELD_EDITORS.join(", ")`, which takes every editor back to
 * prompting whenever it is merely open. That is the defect the selector was
 * built to remove, and it is a one-line edit away.
 *
 * What it deliberately does *not* claim to catch is an editor whose root
 * publishes the wrong `data-dirty`: nothing renders here, so there is no root
 * to read. `e2e/playwright/specs/controls.spec.mjs` owns that half, per
 * editor, and fails when the attribute stops tracking the save.
 */
test("every editor the leave guard watches is matched only while it is dirty", () => {
  const clauses = DIRTY_FIELD_EDITORS.split(", ");
  const covered = FIELD_EDITORS.every((editor) => clauses.includes(`${editor}[data-dirty="true"]`));

  assert.deepEqual(
    [clauses.length, covered, clauses.every((clause) => clause.endsWith('[data-dirty="true"]'))],
    [FIELD_EDITORS.length, true, true],
  );
});
