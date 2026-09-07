import assert from "node:assert/strict";
import test from "node:test";
import {
  dispatchPrs, merge, source, validateReleaseDiff, validateReleasePr, validateTitle,
} from "./release-pr.mjs";

const repository = "owner/bureau";
const repo = { owner: "owner", repo: "bureau" };
const pr = {
  number: 17, state: "open", draft: false, title: "chore: release",
  user: { login: "github-actions[bot]" }, labels: [{ name: "release" }],
  base: { ref: "main" },
  head: { ref: "release-plz-2026-09-07", sha: "tested", repo: { full_name: repository } },
};
const comparison = {
  ahead_by: 1, behind_by: 0, merge_base_commit: { sha: "main" },
  files: [{ filename: "Cargo.toml", status: "modified" }],
};

function api(pull = pr, base = "main", diff = comparison) {
  const calls = [];
  return {
    calls,
    paginate: async () => [],
    rest: {
      pulls: {
        get: async () => ({ data: pull }),
        merge: async (input) => {
          calls.push(["merge", input]);
          return { data: { merged: true } };
        },
      },
      repos: {
        listReleases: "releases",
        listTags: "tags",
        getBranch: async () => ({ data: { commit: { sha: base } } }),
        compareCommitsWithBasehead: async () => ({ data: diff }),
      },
      actions: { createWorkflowDispatch: async (input) => calls.push(["dispatch", input]) },
    },
  };
}

test("requires conventional titles so squash commits carry version intent", () => {
  for (const title of ["feat: command", "fix(cli): repair", "feat!: breaking", "chore: release"]) {
    assert.doesNotThrow(() => validateTitle(title));
  }
  for (const title of ["Update files", "feat:", "feat: one\nfix: two", "unknown: change"]) {
    assert.throws(() => validateTitle(title), /Conventional Commit/);
  }
});

test("release PR identity checks reject lookalikes and forks", () => {
  assert.doesNotThrow(() => validateReleasePr(pr, repository));
  for (const change of [
    { state: "closed" }, { draft: true }, { user: { login: "human" } },
    { labels: [] }, { base: { ref: "other" } },
    { head: { ...pr.head, repo: { full_name: "outside/bureau" } } },
    { head: { ...pr.head, ref: "feature" } }, { head: { ...pr.head, repo: null } },
  ]) {
    assert.throws(() => validateReleasePr({ ...pr, ...change }, repository), /Refusing/);
  }
});

test("only a single version-only commit on current main may auto-merge", () => {
  assert.doesNotThrow(() => validateReleaseDiff(comparison, "main"));
  for (const change of [
    { ahead_by: 2 }, { behind_by: 1 }, { merge_base_commit: { sha: "old" } },
    { files: [] }, { files: [{ filename: ".github/workflows/ci.yml", status: "modified" }] },
    { files: [{ filename: "Cargo.toml", status: "removed" }] },
  ]) {
    assert.throws(() => validateReleaseDiff({ ...comparison, ...change }, "main"), /version\/changelog-only/);
  }
});

test("dispatch checks the release head instead of the dispatch event's main SHA", async () => {
  const outputs = {};
  await source({
    github: api(),
    context: { repo, sha: "main", eventName: "workflow_dispatch", payload: { inputs: { release_pr: "17" } } },
    core: { setOutput: (key, value) => { outputs[key] = value; } },
  });
  assert.deepEqual(outputs, { sha: "tested", base: "main", release_pr: "17" });
});

test("ordinary PR checks use the merge SHA and validate the title", async () => {
  const outputs = {};
  await source({
    github: api(),
    context: { repo, sha: "merge", eventName: "pull_request", payload: { pull_request: { title: "feat: command" } } },
    core: { setOutput: (key, value) => { outputs[key] = value; } },
  });
  assert.equal(outputs.sha, "merge");
});

test("automatic merge is SHA-conditional and explicitly triggers main CI", async () => {
  const github = api();
  await merge({ github, context: { repo }, sha: "tested", base: "main", number: "17" });
  assert.deepEqual(github.calls.map(([name]) => name), ["merge", "dispatch"]);
  assert.equal(github.calls[0][1].sha, "tested");
  assert.deepEqual(github.calls[1][1].inputs, { release_pr: "" });
});

test("head and base races prevent merges and dispatches", async () => {
  for (const github of [
    api({ ...pr, head: { ...pr.head, sha: "new-head" } }),
    api(pr, "new-main", { ...comparison, merge_base_commit: { sha: "new-main" } }),
  ]) {
    await assert.rejects(merge({ github, context: { repo }, sha: "tested", base: "main", number: 17 }), /changed after validation/);
    assert.deepEqual(github.calls, []);
  }
});

test("a blocked merge never dispatches main", async () => {
  const github = api();
  github.rest.pulls.merge = async () => ({ data: { merged: false, message: "required review" } });
  await assert.rejects(merge({ github, context: { repo }, sha: "tested", base: "main", number: 17 }), /required review/);
  assert.deepEqual(github.calls, []);
});

test("release-plz PR output is dispatched even though token-created PRs have no CI event", async () => {
  const github = api();
  await dispatchPrs({ github, context: { repo }, prs: '[{"number":17}]' });
  assert.deepEqual(github.calls[0][1].inputs, { release_pr: "17" });
  await assert.rejects(dispatchPrs({ github, context: { repo }, prs: "{}" }), /Unexpected/);
  await assert.rejects(dispatchPrs({ github, context: { repo }, prs: '[{"number":"bad"}]' }), /Invalid/);
});

test("an unchanged bot release PR is redispatched after a missed or failed run", async () => {
  const github = api();
  github.paginate = async () => [pr, { ...pr, number: 18, user: { login: "human" } }];
  await dispatchPrs({ github, context: { repo }, prs: "[]" });
  assert.equal(github.calls.length, 1);
  assert.deepEqual(github.calls[0][1].inputs, { release_pr: "17" });
});

test("main recovers a pending draft at its original tested commit", async () => {
  const outputs = {};
  const github = api();
  github.paginate = async (method) => method === "releases"
    ? [{ draft: true, tag_name: "v0.1.0", author: { login: "github-actions[bot]" } }] : [];
  github.rest.git = { getRef: async () => ({ data: { object: { type: "commit", sha: "release" } } }) };
  github.rest.repos.compareCommitsWithBasehead = async () => ({ data: { merge_base_commit: { sha: "release" } } });
  await source({
    github,
    context: { repo, sha: "main", ref: "refs/heads/main", eventName: "schedule", payload: {} },
    core: { setOutput: (key, value) => { outputs[key] = value; } },
  });
  assert.deepEqual(outputs, { sha: "release", recover_release: "true" });
});

test("draft recovery rejects tags not on main's history", async () => {
  const github = api();
  github.paginate = async (method) => method === "releases"
    ? [{ draft: true, tag_name: "v0.1.0", author: { login: "github-actions[bot]" } }] : [];
  github.rest.git = { getRef: async () => ({ data: { object: { type: "commit", sha: "unrelated" } } }) };
  await assert.rejects(source({
    github,
    context: { repo, sha: "main", ref: "refs/heads/main", eventName: "push", payload: {} },
    core: { setOutput: () => {} },
  }), /not an ancestor/);
});

test("ordinary main runs with no pending release check the event SHA", async () => {
  const outputs = {};
  await source({
    github: api(),
    context: { repo, sha: "main", ref: "refs/heads/main", eventName: "push", payload: {} },
    core: { setOutput: (key, value) => { outputs[key] = value; } },
  });
  assert.deepEqual(outputs, { sha: "main", base: "main", release_pr: "" });
});
