import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  CATEGORIES, LABELS, REPOSITORY, SHA, categoryLabel, digest, intentBlock, labels, readBlock,
  readIntent, requireValue, sourceMarker, trustedIssue, validateEvidence,
} from "./maintenance-contract.mjs";
import { github } from "./maintenance-http.mjs";
import { issuer, loadPolicy } from "./maintenance-policy.mjs";

export function cycleAt(date) {
  requireValue(Number.isFinite(date.getTime()), "invalid intent clock");
  const cycle = new Date(date);
  cycle.setUTCHours(Math.floor(cycle.getUTCHours() / 6) * 6, 0, 0, 0);
  return cycle.toISOString().replace(".000Z", "Z");
}

export function desiredIntent(category, commit, date) {
  categoryLabel(category);
  requireValue(SHA.test(commit), "intent update requires a full committed source SHA");
  return { schema: "bureau-maintenance-intent-v1", category, commit, cycle: cycleAt(date) };
}

export function updateBody(body, intent) {
  const current = readIntent(body, intent.category);
  const old = intentBlock(current);
  requireValue(body.split(old).length === 2, "intent block must use the canonical JSON encoding");
  const result = body.replace(old, intentBlock(intent));
  requireValue(result.length <= 60_000, "updated source body exceeds the limit");
  return result;
}

function approvedIntentSource(issue, category, policy) {
  trustedIssue(issue, policy.source_issues[category]);
  issuer(issue, policy);
  const observed = labels(issue);
  requireValue(issue.body?.split(sourceMarker(category)).length === 2, "source marker is not unique");
  requireValue([categoryLabel(category), LABELS.approved, LABELS.ready].every((label) => observed.includes(label)),
    "source approval/ready labels must be granted separately by a human");
  requireValue(!observed.some((label) => [LABELS.fix, LABELS.failed, LABELS.human,
    "agent-eligible", "bureau:design-scan"].includes(label)), "source requires human review before re-arming");
  requireValue(["open", "closed"].includes(issue.state), "unknown source issue state");
  return readIntent(issue.body, category);
}

function completed(comments, intent, policy) {
  return comments.some((comment) => {
    if (comment.user?.login !== policy.issuer_login || comment.user.id !== policy.issuer_id
      || !comment.body?.startsWith("<!-- bureau-maintenance-report\n")) return false;
    const report = readBlock(comment.body, "bureau-maintenance-report");
    const checked = validateEvidence(report.evidence);
    requireValue(digest(checked) === report.evidence_sha256, "source report evidence digest is invalid");
    return checked.source.category === intent.category && checked.source.commit === intent.commit
      && checked.source.cycle === intent.cycle
      && checked.source.id === `${REPOSITORY}#${policy.source_issues[intent.category]}`;
  });
}

export async function updateIntent(api, category, commit, date, policy) {
  const number = policy.source_issues[category];
  const source = await api.issue(number);
  const old = approvedIntentSource(source, category, policy);
  const intent = desiredIntent(category, commit, date);
  const unchanged = JSON.stringify(old) === JSON.stringify(intent);
  if (unchanged && completed(await api.comments(number), intent, policy)) {
    return { category, number, status: "already-reported", intent };
  }
  // A second read prevents an old approval snapshot from authorizing a new mutation.
  const live = await api.issue(number);
  approvedIntentSource(live, category, policy);
  requireValue(live.body === source.body && JSON.stringify(labels(live)) === JSON.stringify(labels(source)),
    "source changed during intent preparation; retry a later wake");
  if (!unchanged || live.state !== "open") {
    await api.request("PATCH", `/issues/${number}`, { body: updateBody(live.body, intent), state: "open" });
  }
  let current = await api.issue(number);
  approvedIntentSource(current, category, policy);
  requireValue(JSON.stringify(readIntent(current.body, category)) === JSON.stringify(intent), "intent write was not observed");
  if (!labels(current).includes(LABELS.scan)) {
    await api.request("POST", `/issues/${number}/labels`, { labels: [LABELS.scan] });
  }
  current = await api.issue(number);
  approvedIntentSource(current, category, policy);
  if (labels(current).includes(LABELS.reported)) {
    await api.request("DELETE", `/issues/${number}/labels/${encodeURIComponent(LABELS.reported)}`);
  }
  const final = await api.issue(number);
  approvedIntentSource(final, category, policy);
  requireValue(final.body === updateBody(source.body, intent) && final.state === "open"
    && labels(final).includes(LABELS.scan) && !labels(final).includes(LABELS.reported),
  "intent was not independently observed in its ready state");
  return { category, number, status: unchanged ? "pending" : "updated", intent };
}

export function trustedWorkflow(environment) {
  requireValue(environment.GITHUB_REPOSITORY === REPOSITORY, "intent updater is restricted to the exact repository");
  requireValue(["schedule", "workflow_dispatch"].includes(environment.GITHUB_EVENT_NAME),
    "intent mutation is restricted to trusted schedule/dispatch events");
  requireValue(environment.GITHUB_REF === `refs/heads/${environment.BUREAU_DEFAULT_BRANCH}`
    && environment.BUREAU_DEFAULT_BRANCH, "intent updater must run on the default branch");
  requireValue(environment.BUREAU_MAINTENANCE_ENABLED === "true", "maintenance intent updates are disabled");
  requireValue(SHA.test(environment.GITHUB_SHA ?? ""), "missing committed workflow source");
}

async function main() {
  trustedWorkflow(process.env);
  requireValue(process.env.GH_TOKEN, "intent updater requires the scoped workflow credential");
  const api = github({ token: process.env.GH_TOKEN });
  const policy = await loadPolicy();
  const results = [];
  for (const category of CATEGORIES) {
    try {
      results.push(await updateIntent(api, category, process.env.GITHUB_SHA, new Date(), policy));
    } catch (error) {
      results.push({ category, status: "blocked", message: error.message });
      process.exitCode = 1;
    }
  }
  console.log(JSON.stringify({ schema: "bureau-maintenance-intents-v1", results }));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
