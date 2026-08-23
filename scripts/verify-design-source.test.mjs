import assert from "node:assert/strict";
import test from "node:test";

import { sourceProblem } from "./verify-design-source.mjs";

const request = { inputs: { source_commit: "abc123" } };
const workspace = { commit: "abc123", status: "" };

test("accepts a clean source issue without its trigger", () => {
  assert.equal(sourceProblem(request, { state: "open", labels: [] }, workspace), null);
});

test("rejects a source issue that remains eligible", () => {
  const issue = {
    state: "open",
    labels: [{ name: "bureau:design-scan" }],
  };

  assert.equal(
    sourceProblem(request, issue, workspace),
    "source issue still carries bureau:design-scan",
  );
});

test("rejects a closed source issue", () => {
  assert.equal(
    sourceProblem(request, { state: "closed", labels: [] }, workspace),
    "source issue was closed",
  );
});
