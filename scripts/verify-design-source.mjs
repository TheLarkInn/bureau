import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { readStepRequest } from "./read-step-request.mjs";

const HEADERS = {
  Accept: "application/vnd.github+json",
  "User-Agent": "bureau-design-audit",
};

function labelNames(issue) {
  return (issue?.labels ?? []).map((label) =>
    typeof label === "string" ? label : label.name);
}

export function sourceProblem(request, issue, workspace) {
  const sourceCommit = request?.inputs?.source_commit;
  if (workspace.commit !== sourceCommit || workspace.status) {
    return "no-findings publication changed the worktree";
  }
  if (issue?.state !== "open") return "source issue was closed";
  return labelNames(issue).includes("bureau:design-scan")
    ? "source issue still carries bureau:design-scan"
    : null;
}

async function main() {
  const request = await readStepRequest();
  const number = request.item.external_id.split("#").at(-1);
  const response = await fetch(
    `https://api.github.com/repos/TheLarkInn/bureau/issues/${number}`,
    { headers: HEADERS },
  );
  if (!response.ok) {
    throw new Error(`reading source issue returned HTTP ${response.status}`);
  }
  const options = { encoding: "utf8" };
  const workspace = {
    commit: execFileSync("git", ["rev-parse", "HEAD"], options).trim(),
    status: execFileSync("git", ["status", "--porcelain"], options).trim(),
  };
  const problem = sourceProblem(request, await response.json(), workspace);
  if (problem) {
    console.error(problem);
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
