import { tagCommit } from "./release-assets.mjs";

const conventionalTitle = /^(feat|fix|perf|refactor|revert|docs|style|test|build|ci|chore)(\([^\r\n()]+\))?!?: \S[^\r\n]*$/;
const releaseFiles = new Set([
  "Cargo.toml",
  "Cargo.lock",
  "CHANGELOG.md",
  "crates/bureau/Cargo.toml",
  "crates/bureau/CHANGELOG.md",
  "crates/bureau-lifecycle/Cargo.toml",
  "crates/bureau-plugin/Cargo.toml",
  ".github/extensions/bureau-canvas/Cargo.toml",
]);

export function validateTitle(title) {
  if (!conventionalTitle.test(title)) {
    throw new Error("Use a Conventional Commit PR title, e.g. feat: add a command, fix: repair a bug, or feat!: change the CLI contract.");
  }
}

function isReleasePr(pr, repository) {
  return pr.state === "open" && !pr.draft && pr.user.login === "github-actions[bot]"
    && pr.base.ref === "main" && pr.head.repo?.full_name === repository
    && pr.head.ref.startsWith("release-plz-")
    && pr.labels.some(({ name }) => name === "release");
}

export function validateReleasePr(pr, repository) {
  if (!isReleasePr(pr, repository)) {
    throw new Error("Refusing to automate a PR that is not an open, same-repository release-plz release PR.");
  }
}

export function validateReleaseDiff(comparison, baseSha) {
  if (comparison.behind_by !== 0 || comparison.ahead_by !== 1
      || comparison.merge_base_commit.sha !== baseSha
      || !comparison.files?.length
      || comparison.files.some(({ filename, status }) => !releaseFiles.has(filename)
        || !["added", "modified"].includes(status))) {
    throw new Error("Release PR must be one version/changelog-only commit directly on current main; rerun CI on main to refresh it.");
  }
}

async function readReleasePr(github, repo, number) {
  const { data: pr } = await github.rest.pulls.get({ ...repo, pull_number: number });
  validateReleasePr(pr, `${repo.owner}/${repo.repo}`);
  const { data: main } = await github.rest.repos.getBranch({ ...repo, branch: "main" });
  const { data: comparison } = await github.rest.repos.compareCommitsWithBasehead({
    ...repo, basehead: `${main.commit.sha}...${pr.head.sha}`,
  });
  validateReleaseDiff(comparison, main.commit.sha);
  return { pr, base: main.commit.sha };
}

export async function dispatch(github, repo, number = "") {
  await github.rest.actions.createWorkflowDispatch({
    ...repo, workflow_id: "ci.yml", ref: "main",
    inputs: { release_pr: String(number) },
  });
}

async function pendingRelease(github, repo, sha) {
  const releases = await github.paginate(github.rest.repos.listReleases, { ...repo, per_page: 100 });
  const tags = await github.paginate(github.rest.repos.listTags, { ...repo, per_page: 100 });
  const pending = releases.filter((release) => release.draft
    && release.author.login === "github-actions[bot]").map((release) => release.tag_name);
  pending.push(...tags.filter((tag) => !releases.some((release) => release.tag_name === tag.name))
    .map((tag) => tag.name));
  const versions = pending.filter((tag) => /^v\d+\.\d+\.\d+$/.test(tag));
  if (versions.length > 1) throw new Error("Multiple pending releases require inspection.");
  if (!versions.length) return null;
  const commit = await tagCommit(github, repo, versions[0]);
  const { data } = await github.rest.repos.compareCommitsWithBasehead({
    ...repo, basehead: `${commit}...${sha}`,
  });
  if (data.merge_base_commit.sha !== commit) {
    throw new Error("Pending release is not an ancestor of this main revision.");
  }
  return commit;
}

export async function source({ github, context, core }) {
  const number = context.payload.inputs?.release_pr;
  if (number) {
    if (context.eventName !== "workflow_dispatch" || !/^[1-9]\d*$/.test(number)) {
      throw new Error("release_pr must be a positive PR number supplied by workflow_dispatch.");
    }
    const { pr, base } = await readReleasePr(github, context.repo, Number(number));
    core.setOutput("sha", pr.head.sha);
    core.setOutput("base", base);
    core.setOutput("release_pr", number);
    return;
  }
  if (context.eventName === "pull_request") {
    validateTitle(context.payload.pull_request.title);
  } else {
    if (context.ref !== "refs/heads/main") throw new Error("Release CI must be dispatched on main.");
    const pending = await pendingRelease(github, context.repo, context.sha);
    if (pending) {
      core.setOutput("sha", pending);
      core.setOutput("recover_release", "true");
      return;
    }
  }
  core.setOutput("sha", context.sha);
  core.setOutput("base", context.sha);
  core.setOutput("release_pr", "");
}

export async function merge({ github, context, sha, base, number }) {
  const { pr, base: currentBase } = await readReleasePr(github, context.repo, Number(number));
  if (pr.head.sha !== sha || currentBase !== base) {
    throw new Error("Release PR or main changed after validation; refusing to merge an untested revision.");
  }
  // GitHub's strict, up-to-date branch protection guards base drift during
  // this request; the REST sha condition itself protects only the PR head.
  const { data: result } = await github.rest.pulls.merge({
    ...context.repo, pull_number: pr.number, sha,
    merge_method: "squash", commit_title: pr.title,
  });
  if (!result.merged) throw new Error(`Release PR was not merged: ${result.message}`);
  // GITHUB_TOKEN merges do not trigger push workflows. Explicit dispatch does.
  await dispatch(github, context.repo);
}

export async function dispatchPrs({ github, context, prs }) {
  const parsed = JSON.parse(prs || "[]");
  if (!Array.isArray(parsed)) throw new Error("Unexpected release-plz PR output.");
  if (!parsed.length) {
    const open = await github.paginate(github.rest.pulls.list, {
      ...context.repo, state: "open", base: "main", per_page: 100,
    });
    parsed.push(...open.filter((pr) => isReleasePr(pr, `${context.repo.owner}/${context.repo.repo}`)));
  }
  if (parsed.length > 1) throw new Error("Expected one workspace release PR.");
  for (const pr of parsed) {
    const number = Number(pr.number);
    if (!Number.isSafeInteger(number) || number < 1) throw new Error("Invalid release-plz PR number.");
    await dispatch(github, context.repo, number);
  }
}
