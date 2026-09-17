import {
  LABELS, authorizedSource, categoryLabel, digest, findingBody, findingMarker, fingerprint,
  issueNumber, issueUrl, labels, readEvidence, readIntent, reportBody, requireValue,
  trustedIssue, urlNumber, validateEvidence,
} from "./maintenance-contract.mjs";
import { issuer } from "./maintenance-policy.mjs";

export function checkSource(issue, category, policy, source = null, active = true) {
  const number = policy.source_issues[category];
  issuer(issue, policy);
  const intent = authorizedSource(issue, category, number, active);
  if (source) {
    requireValue(source.id === `TheLarkInn/bureau#${number}` && source.category === category,
      "evidence source is not the reviewed source issue");
    if (active) requireValue(intent.commit === source.commit && intent.cycle === source.cycle,
      "live source intent changed; stale evidence cannot authorize effects");
  }
  return intent;
}

export function matchingIssues(issues, value) {
  requireValue(Array.isArray(issues), "repository issues were not observed");
  const marker = findingMarker(value);
  return issues.filter((issue) => !issue.pull_request && issue.body?.includes(marker));
}

function noConflictingLabels(issue, category) {
  const observed = labels(issue);
  requireValue(!observed.some((label) => [LABELS.scan, LABELS.approved, "agent-eligible",
    "bureau:design-scan"].includes(label)), "finding has a source or unrelated automation label");
  requireValue(!observed.some((label) => label.startsWith("bureau:maintenance-")
    && ![categoryLabel(category), LABELS.fix, LABELS.ready, LABELS.failed, LABELS.human].includes(label)),
  "finding crosses maintenance categories");
}

export function checkedFinding(issue, value, policy) {
  const expected = validateEvidence(value);
  trustedIssue(issue, issue.number);
  issuer(issue, policy);
  requireValue(issue.number !== policy.source_issues[expected.source.category], "finding is the source issue");
  const recorded = readEvidence(issue.body);
  requireValue(fingerprint(recorded) === fingerprint(expected), "finding fingerprint does not match detection");
  requireValue(issue.body === findingBody(recorded), "finding body is not canonical deterministic evidence");
  noConflictingLabels(issue, expected.source.category);
  return recorded;
}

export function publication(issue, value) {
  return { url: issueUrl(issue.number), fingerprint: fingerprint(value),
    disposition: issue.state === "closed" ? "closed" : "open" };
}

export function verifyReport(comments, value, published, policy) {
  const expected = reportBody(value, published);
  const matches = comments.filter((comment) => comment.body === expected);
  requireValue(matches.length === 1, "expected exactly one independently observed source report");
  issuer(matches[0], policy);
}

export function verifyDraft({ source, issues, comments }, value, policy) {
  const expected = validateEvidence(value);
  checkSource(source, expected.source.category, policy, expected.source);
  const matches = matchingIssues(issues, expected);
  requireValue(matches.length === 1, `expected one finding-marker issue, observed ${matches.length}`);
  const issue = matches[0];
  const recorded = checkedFinding(issue, expected, policy);
  requireValue(["open", "closed"].includes(issue.state), "unknown issue state");
  const observed = labels(issue);
  const previouslyReady = observed.includes(LABELS.ready) || observed.includes(LABELS.fix);
  if (previouslyReady || digest(recorded) !== digest(expected)) {
    verifyReport(comments, recorded, { ...publication(issue, recorded), disposition: "open" }, policy);
  }
  return publication(issue, expected);
}

export function verifyHandoff({ source, issues, comments }, value, expected, policy) {
  const checked = validateEvidence(value);
  checkSource(source, checked.source.category, policy, checked.source, false);
  const intent = readIntent(source.body, checked.source.category);
  requireValue(intent.commit === checked.source.commit && intent.cycle === checked.source.cycle,
    "source intent changed during handoff");
  requireValue(!labels(source).includes(LABELS.scan) && labels(source).includes(LABELS.reported),
    "source scan was not durably reported");
  if (expected) {
    const matches = matchingIssues(issues, checked);
    requireValue(matches.length === 1 && matches[0].number === urlNumber(expected.url),
      "handoff issue identity is missing or duplicated");
    checkedFinding(matches[0], checked, policy);
    requireValue(JSON.stringify(publication(matches[0], checked)) === JSON.stringify(expected),
      "finding state changed during handoff");
    if (expected.disposition === "open") {
      const observed = labels(matches[0]);
      requireValue([categoryLabel(checked.source.category), LABELS.fix, LABELS.ready]
        .every((label) => observed.includes(label)), "fix handoff is not ready");
    }
  } else {
    requireValue(checked.findings.length === 0, "cannot clear unreported findings");
  }
  verifyReport(comments, checked, expected, policy);
}

export function verifyFix(issue, source, comments, category, policy) {
  issuer(issue, policy);
  trustedIssue(issue, issue.number);
  requireValue(issue.state === "open", "fix issue is closed");
  const observed = labels(issue);
  requireValue([categoryLabel(category), LABELS.fix, LABELS.ready]
    .every((label) => observed.includes(label)), "fix issue is not selectively ready");
  requireValue(!observed.includes(LABELS.failed) && !observed.includes(LABELS.human),
    "fix issue needs human review");
  const recorded = checkedFinding(issue, readEvidence(issue.body), policy);
  requireValue(recorded.source.category === category, "finding category mismatch");
  checkSource(source, category, policy, recorded.source, false);
  verifyReport(comments, recorded, publication(issue, recorded), policy);
  return recorded;
}

export async function observe(api, category, policy) {
  const number = policy.source_issues[category];
  const source = await api.issue(number);
  const issues = await api.issues();
  const comments = await api.comments(number);
  return { source, issues, comments };
}

export async function liveFix(api, request, category, policy) {
  const number = issueNumber(request.item.external_id);
  const issue = await api.issue(number);
  requireValue(issue.body === request.item.body && issue.html_url === request.item.url,
    "fix issue changed after claim");
  const sourceNumber = policy.source_issues[category];
  const source = await api.issue(sourceNumber);
  return verifyFix(issue, source, await api.comments(sourceNumber), category, policy);
}
