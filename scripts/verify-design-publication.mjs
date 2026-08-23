import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { readStepRequest } from "./read-step-request.mjs";

const ISSUE_URL = /^https:\/\/github\.com\/TheLarkInn\/bureau\/issues\/([1-9]\d*)$/u;
const API = "https://api.github.com/repos/TheLarkInn/bureau/issues";
const HEADERS = {
  Accept: "application/vnd.github+json",
  "User-Agent": "bureau-design-audit",
};
const TRUSTED_AUTHOR = "TheLarkInn";

function labelNames(issue) {
  return (issue?.labels ?? []).map((label) =>
    typeof label === "string" ? label : label.name);
}

function sourceNumber(request) {
  return Number(request?.item?.external_id?.split("#").at(-1));
}

function markerMatches(issues, marker) {
  return issues.filter((issue) =>
    !issue.pull_request && issue.body?.includes(marker));
}

function sourceIssue(request, issues) {
  return issues.find((candidate) =>
    !candidate.pull_request && candidate.number === sourceNumber(request));
}

function draftProblem(source, issue) {
  if (source.number === issue.number) return "drafted issue is the source issue";
  if (source.state !== "open") return "source issue was closed";
  if (issue.user?.login !== TRUSTED_AUTHOR) {
    return "drafted issue author is not trusted";
  }
  if (!labelNames(source).includes("bureau:design-scan")) {
    return "source issue lost bureau:design-scan before handoff";
  }
  const labels = labelNames(issue);
  if (labels.includes("agent-eligible")) return "drafted issue is already agent-eligible";
  return labels.includes("bureau:design-scan")
    ? "drafted issue carries the source trigger"
    : null;
}

function handoffProblem(source, issue) {
  if (source.state !== "open") return "source issue was closed";
  if (issue.user?.login !== TRUSTED_AUTHOR) {
    return "published issue author is not trusted";
  }
  if (!labelNames(issue).includes("agent-eligible")) {
    return "published issue lacks agent-eligible";
  }
  return labelNames(source).includes("bureau:design-scan")
    ? "source issue still carries bureau:design-scan"
    : null;
}

export function publicationProblem(request, receipt, issues, workspace, requireHandoff = false) {
  const outputs = request?.inputs ?? {};
  const url = ISSUE_URL.exec(outputs.created_issue_url ?? "");
  const fingerprint = `${request?.item?.external_id}:${outputs.source_commit}`;
  const marker = `<!-- bureau-design-audit:${fingerprint} -->`;
  const matches = markerMatches(issues, marker);
  if (!url) return "created_issue_url is not a Bureau GitHub issue URL";
  if (outputs.fingerprint !== fingerprint) return "fingerprint does not match the audited source";
  if (receipt?.created_issue_url !== outputs.created_issue_url) return "publication receipt URL does not match";
  if (receipt?.fingerprint !== fingerprint) return "publication receipt fingerprint does not match";
  if (workspace?.commit !== outputs.source_commit || workspace?.status) return "publisher changed the worktree";
  if (matches.length !== 1) return `expected one audit-marker issue, observed ${matches.length}`;
  if (matches[0].number !== Number(url[1]) || matches[0].state !== "open") {
    return "published issue is missing or closed";
  }
  const source = sourceIssue(request, issues);
  if (!source) return "source issue was not observed";
  return requireHandoff
    ? handoffProblem(source, matches[0])
    : draftProblem(source, matches[0]);
}

async function allIssues() {
  const issues = [];
  for (let page = 1; ; page += 1) {
    const batch = await issuePage(page);
    issues.push(...batch);
    if (batch.length < 100) return issues;
  }
}

async function issuePage(page) {
  let status = 0;
  for (let attempt = 1; attempt <= 4; attempt += 1) {
    const response = await fetch(`${API}?state=all&per_page=100&page=${page}`, {
      headers: HEADERS,
    });
    if (response.ok) return response.json();
    status = response.status;
    if (status !== 429 && status < 500) break;
    const retryAfter = Number(response.headers.get("retry-after")) * 1000;
    await new Promise((resolveDelay) =>
      setTimeout(resolveDelay, retryAfter || attempt * 1000));
  }
  throw new Error(`listing repository issues returned HTTP ${status}`);
}

function workspace() {
  const options = { encoding: "utf8" };
  return {
    commit: execFileSync("git", ["rev-parse", "HEAD"], options).trim(),
    status: execFileSync("git", ["status", "--porcelain"], options).trim(),
  };
}

async function main() {
  const request = await readStepRequest();
  const receiptPath = request.artifacts?.["publication.json"];
  const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
  const requireHandoff = process.argv.includes("--require-handoff");
  const problem = publicationProblem(
    request,
    receipt,
    await allIssues(),
    workspace(),
    requireHandoff,
  );
  if (problem) {
    console.error(problem);
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
