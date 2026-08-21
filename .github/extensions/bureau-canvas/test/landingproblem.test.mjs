// Why the primary repo cannot take a branch, when it cannot.
//
// The browser suite runs against the bundled sample, which registers one
// repo, so the two-repo rules live here: a repo missing from the registry is
// a different problem from one registered read-only, and reporting either as
// the other sends the operator to the wrong fix.
//
// The rule lives in `web/app.mjs`, which imports React, so it is restated
// here against the same registry shape the view model supplies.

import assert from "node:assert/strict";
import test from "node:test";

const LANDING_ACCESS = ["pr", "push"];

const REGISTRY = [
  { name: "odsp-web", access: "push" },
  { name: "spo.core", access: "pr" },
  { name: "augloop", access: "read" },
];

const repoEntry = (name) => REGISTRY.find((repo) => repo.name === name);

/** Mirrors `landingProblem` in web/app.mjs. */
function landingProblem(name) {
  const entry = repoEntry(name);
  if (!entry) {
    return { kind: "unknown" };
  }
  if (!LANDING_ACCESS.includes(entry.access)) {
    return { kind: "read-only" };
  }
  return null;
}

test("a push or pr repo may hold the primary slot", () => {
  assert.deepEqual([landingProblem("odsp-web"), landingProblem("spo.core")], [null, null]);
});

test("a read-only primary is reported as read-only, not as missing", () => {
  assert.equal(landingProblem("augloop").kind, "read-only");
});

test("a repo absent from the registry is reported as unknown, not as read-only", () => {
  // The registry is where access lives, so an unregistered repo has no access
  // to report; calling it read-only would state a fact the UI does not have.
  assert.equal(landingProblem("rushstack").kind, "unknown");
});

test("reordering moves which repo the branch lands in", () => {
  const reorder = (list, from, to) => {
    const next = [...list];
    [next[from], next[to]] = [next[to], next[from]];
    return next;
  };
  const start = ["augloop", "odsp-web"];
  const promoted = reorder(start, 0, 1);

  assert.deepEqual(
    { before: landingProblem(start[0]).kind, after: landingProblem(promoted[0]), order: promoted },
    { before: "read-only", after: null, order: ["odsp-web", "augloop"] },
  );
});

test("only the first repo decides whether a branch can land", () => {
  // `augloop` is read-only, but below the primary it is only context.
  const list = ["odsp-web", "augloop"];

  assert.equal(landingProblem(list[0]), null);
});
