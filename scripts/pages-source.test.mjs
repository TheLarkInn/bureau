import assert from "node:assert/strict";
import test from "node:test";
import { pagesSource } from "./pages-source.mjs";

const sha = "a".repeat(40);
const context = {
  sha, eventName: "push", ref: "refs/heads/main",
  repo: { owner: "TheLarkInn", repo: "bureau" }, payload: {},
};
const github = {
  rest: { git: { async getRef(args) {
    assert.deepEqual(args, { owner: "TheLarkInn", repo: "bureau", ref: "heads/main" });
    return { data: { object: { type: "commit", sha } } };
  } } },
};

test("Pages accepts only the exact current trusted main commit for push and dispatch", async () => {
  for (const eventName of ["push", "workflow_dispatch"]) {
    assert.equal(await pagesSource({ github, context: { ...context, eventName } }), sha);
  }
});

test("PR checks use the immutable merge SHA without a privileged lookup", async () => {
  assert.equal(await pagesSource({
    github: {},
    context: { ...context, eventName: "pull_request", ref: "refs/pull/42/merge", payload: { pull_request: { base: { ref: "main" } } } },
  }), sha);
});

test("Pages rejects a fork, branch, tag, untrusted event, mutable ref, or stale main", async () => {
  for (const change of [
    { repo: { owner: "other", repo: "bureau" } },
    { ref: "refs/heads/feature" }, { ref: "refs/tags/v1.0.0" },
    { eventName: "pull_request_target" }, { eventName: "workflow_run" },
    { sha: "main" }, { sha: "b".repeat(40) },
    { eventName: "pull_request", payload: { pull_request: { base: { ref: "feature" } } } },
  ]) await assert.rejects(pagesSource({ github, context: { ...context, ...change } }));
});

test("Pages never converts a failed or non-commit main lookup into a source", async () => {
  for (const getRef of [
    async () => { throw new Error("Lookup unavailable."); },
    async () => ({ data: { object: { type: "tag", sha } } }),
  ]) await assert.rejects(pagesSource({ github: { rest: { git: { getRef } } }, context }));
});
