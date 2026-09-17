import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { readStepRequest } from "./read-step-request.mjs";
import {
  LABELS, categoryLabel, findingBody, fingerprint, issueUrl, reportBody,
  requireValue, stepResult, validateEvidence,
} from "./maintenance-contract.mjs";
import { github } from "./maintenance-http.mjs";
import { loadPolicy } from "./maintenance-policy.mjs";
import {
  checkSource, checkedFinding, matchingIssues, observe, publication, verifyDraft, verifyHandoff, verifyReport,
} from "./maintenance-lifecycle.mjs";
import { requireClean } from "./maintenance-checks.mjs";

async function actor(api, policy) {
  const user = await api.request("GET", "/user");
  requireValue(user.login === policy.issuer_login && user.id === policy.issuer_id,
    "publishing credential differs from the reviewed issuer identity");
}

async function sourceStillCurrent(api, value, policy) {
  const source = await api.issue(policy.source_issues[value.source.category]);
  checkSource(source, value.source.category, policy, value.source);
}

export async function draft(api, value, policy) {
  const checked = validateEvidence(value);
  requireValue(checked.findings.length > 0, "draft requires deterministic findings");
  await actor(api, policy);
  await sourceStillCurrent(api, checked, policy);
  const found = matchingIssues(await api.issues(), checked);
  requireValue(found.length <= 1, "duplicate finding markers; human review required");
  if (found.length) {
    checkedFinding(found[0], checked, policy);
    return publication(found[0], checked);
  }
  let created;
  try {
    created = await api.request("POST", "/issues", {
      title: `fix: ${checked.source.category} maintenance findings at ${checked.source.commit.slice(0, 12)}`,
      body: findingBody(checked),
    });
  } catch (error) {
    const observed = matchingIssues(await api.issues(), checked);
    requireValue(observed.length === 1,
      `issue creation is indeterminate; do not repeat POST: ${error.message}`);
    [created] = observed;
  }
  checkedFinding(created, checked, policy);
  requireValue(created.state === "open", "created finding is not open");
  await sourceStillCurrent(api, checked, policy);
  return publication(created, checked);
}

async function recordReport(api, value, published, policy) {
  const number = policy.source_issues[value.source.category];
  const body = reportBody(value, published);
  const existing = (await api.comments(number)).filter((comment) => comment.body === body);
  requireValue(existing.length <= 1, "duplicate source reports");
  if (existing.length) {
    verifyReport(existing, value, published, policy);
    return;
  }
  try {
    await api.request("POST", `/issues/${number}/comments`, { body });
  } catch (error) {
    const matches = (await api.comments(number)).filter((comment) => comment.body === body);
    requireValue(matches.length === 1, `source report is indeterminate; do not repeat POST: ${error.message}`);
  }
  verifyReport(await api.comments(number), value, published, policy);
}

async function markReported(api, value, policy) {
  await sourceStillCurrent(api, value, policy);
  const number = policy.source_issues[value.source.category];
  await api.request("POST", `/issues/${number}/labels`, { labels: [LABELS.reported] });
  await api.request("DELETE", `/issues/${number}/labels/${encodeURIComponent(LABELS.scan)}`);
}

export async function handoff(api, value, expected, policy, observedAt) {
  const checked = validateEvidence(value);
  await actor(api, policy);
  const observed = await observe(api, checked.source.category, policy);
  const verified = verifyDraft(observed, checked, policy, observedAt);
  requireValue(JSON.stringify(verified) === JSON.stringify(expected), "draft receipt changed after verification");
  await sourceStillCurrent(api, checked, policy);
  await recordReport(api, checked, expected, policy);
  if (verified.disposition === "open") {
    const issue = observed.issues.find((candidate) => issueUrl(candidate.number) === verified.url);
    await sourceStillCurrent(api, checked, policy);
    checkedFinding(await api.issue(issue.number), checked, policy);
    await api.request("POST", `/issues/${issue.number}/labels`, {
      labels: [categoryLabel(checked.source.category), LABELS.fix, LABELS.ready],
    });
  }
  await markReported(api, checked, policy);
  verifyHandoff(await observe(api, checked.source.category, policy), checked, expected, policy);
  return expected;
}

export async function clear(api, value, policy) {
  const checked = validateEvidence(value);
  requireValue(checked.findings.length === 0, "cannot clear a source with findings");
  await actor(api, policy);
  await sourceStillCurrent(api, checked, policy);
  await recordReport(api, checked, null, policy);
  await markReported(api, checked, policy);
  const number = policy.source_issues[checked.source.category];
  verifyHandoff({ source: await api.issue(number), comments: await api.comments(number), issues: [] },
    checked, null, policy);
}

async function main() {
  try {
    const request = await readStepRequest({ maximumBytes: 1024 * 1024 });
    requireValue(request?.schema === "v2", "publisher requires a v2 request");
    const policy = await loadPolicy();
    const value = validateEvidence(request.inputs?.maintenance_evidence);
    requireClean(value.source);
    const api = github({ token: process.env.GH_TOKEN });
    requireValue(process.env.GH_TOKEN, "publisher has no explicitly granted forge credential");
    const mode = process.argv[2];
    let receipt = null;
    if (mode === "draft") receipt = await draft(api, value, policy);
    else if (mode === "handoff") receipt = await handoff(api, value, request.inputs.maintenance_draft,
      policy, request.inputs.maintenance_observed_at);
    else if (mode === "clear") await clear(api, value, policy);
    else throw new Error("publisher operation must be draft, handoff, or clear");
    console.log(JSON.stringify(stepResult("success", {
      maintenance_publication: receipt, maintenance_fingerprint: fingerprint(value),
    }, "forge effects require the following independent deterministic verification")));
  } catch (error) {
    console.log(JSON.stringify(stepResult("blocked", {}, error.message)));
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
