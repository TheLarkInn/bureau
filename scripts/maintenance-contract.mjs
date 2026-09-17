import { createHash } from "node:crypto";

export const REPOSITORY = "TheLarkInn/bureau";
export const AUTHOR = "TheLarkInn";
export const CATEGORIES = ["chaos", "site-accessibility", "site-responsive"];
export const LABELS = Object.freeze({
  approved: "bureau:maintenance-approved",
  ready: "bureau:maintenance-ready",
  scan: "bureau:maintenance-scan",
  fix: "bureau:maintenance-fix",
  reported: "bureau:maintenance-reported",
  failed: "bureau:maintenance-failed",
  human: "bureau:maintenance-needs-human",
});
export const SHA = /^[a-f0-9]{40}$/u;
const INTENT = "bureau-maintenance-intent";
const EVIDENCE = "bureau-maintenance-evidence";

export function requireValue(condition, message) {
  if (!condition) throw new Error(message);
}

export function categoryLabel(category) {
  requireValue(CATEGORIES.includes(category), "unknown maintenance category");
  return `bureau:maintenance-${category}`;
}

export function labels(issue) {
  requireValue(Array.isArray(issue?.labels), "issue labels were not observed");
  return issue.labels.map((label) => typeof label === "string" ? label : label.name);
}

export function sourceMarker(category) {
  categoryLabel(category);
  return `<!-- bureau-maintenance-source:${category} -->`;
}

export function block(name, value) {
  return `<!-- ${name}\n${JSON.stringify(value)}\n-->`;
}

export function readBlock(body, name) {
  requireValue(typeof body === "string" && body.length <= 60_000, "invalid issue body");
  const start = `<!-- ${name}\n`;
  const pieces = body.split(start);
  requireValue(pieces.length === 2, `expected exactly one ${name} block`);
  const end = pieces[1].indexOf("\n-->");
  requireValue(end >= 0, `unterminated ${name} block`);
  return JSON.parse(pieces[1].slice(0, end));
}

export function intentBlock(intent) {
  return block(INTENT, intent);
}

export function readIntent(body, category) {
  const intent = readBlock(body, INTENT);
  requireValue(intent.schema === "bureau-maintenance-intent-v1", "invalid intent schema");
  requireValue(intent.category === category, "intent category mismatch");
  requireValue(SHA.test(intent.commit), "intent requires a full source commit");
  requireValue(/^\d{4}-\d{2}-\d{2}T(00|06|12|18):00:00Z$/u.test(intent.cycle)
    && Number.isFinite(Date.parse(intent.cycle)), "invalid six-hour intent cycle");
  return intent;
}

export function issueNumber(value) {
  const match = /^TheLarkInn\/bureau#([1-9]\d*)$/u.exec(value ?? "");
  requireValue(match && Number.isSafeInteger(Number(match[1])), "work item is outside the exact source");
  return Number(match[1]);
}

export function issueUrl(number) {
  requireValue(Number.isSafeInteger(number) && number > 0, "invalid issue number");
  return `https://github.com/${REPOSITORY}/issues/${number}`;
}

export function urlNumber(value) {
  const match = /^https:\/\/github\.com\/TheLarkInn\/bureau\/issues\/([1-9]\d*)$/u.exec(value ?? "");
  requireValue(match, "publication URL is outside the exact source");
  const number = Number(match[1]);
  requireValue(issueUrl(number) === value, "invalid publication URL");
  return number;
}

export function trustedIssue(issue, number) {
  requireValue(issue?.number === number && issue.html_url === issueUrl(number)
    && !issue.pull_request, "forge returned the wrong issue");
  requireValue(issue.user?.login === AUTHOR, "issue author is not the approved maintainer");
}

export function authorizedSource(issue, category, number, active = true) {
  trustedIssue(issue, number);
  const observed = labels(issue);
  requireValue(issue.state === "open", "source issue is closed");
  requireValue(issue.body?.split(sourceMarker(category)).length === 2, "source marker is not unique");
  requireValue(observed.includes(categoryLabel(category))
    && observed.includes(LABELS.approved) && observed.includes(LABELS.ready),
  "source is not maintainer-approved and ready");
  requireValue(CATEGORIES.filter((value) => observed.includes(categoryLabel(value))).length === 1,
    "source crosses maintenance categories");
  requireValue(!observed.includes(LABELS.fix), "source is also marked as a fix");
  requireValue(!observed.some((label) => [LABELS.failed, LABELS.human, "agent-eligible",
    "bureau:design-scan"].includes(label)), "source has a conflicting or terminal label");
  if (active) requireValue(observed.includes(LABELS.scan), "source scan is no longer active");
  return readIntent(issue.body, category);
}

export function sourcePin(request, category, commit, issue) {
  requireValue(request?.schema === "v2", "maintenance requires a v2 StepRequest");
  const number = issueNumber(request.item?.external_id);
  requireValue(request.item.url === issueUrl(number), "request issue URL mismatch");
  const intent = authorizedSource(issue, category, number);
  requireValue(issue.body === request.item.body, "source intent changed after claim");
  requireValue(commit === intent.commit, "checkout differs from the requested source commit; re-arm intent");
  return { category, id: request.item.external_id, commit, cycle: intent.cycle };
}

export function digest(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

export function seedFor(source) {
  return Number.parseInt(digest(source).slice(0, 8), 16);
}

export function validateFindings(findings, category) {
  requireValue(Array.isArray(findings) && findings.length <= 20, "invalid or excessive findings");
  const ids = new Set();
  for (const finding of findings) {
    requireValue(/^[a-zA-Z0-9._:-]{1,160}$/u.test(finding?.id ?? ""), "invalid finding id");
    requireValue(!ids.has(finding.id), "duplicate finding id");
    ids.add(finding.id);
    for (const [key, maximum] of [["title", 160], ["detail", 4000]]) {
      requireValue(typeof finding[key] === "string" && finding[key].length > 0
        && finding[key].length <= maximum && !finding[key].includes("\0"), `invalid finding ${key}`);
    }
    requireValue(typeof finding.path === "string" && finding.path.length <= 240
      && !/[:\\\0\r\n]/u.test(finding.path) && !finding.path.startsWith("/")
      && !finding.path.split("/").some((part) => ["", ".", ".."].includes(part)), "invalid finding path");
    if (category.startsWith("site-")) {
      requireValue(finding.path.startsWith("site/"), "site finding is outside site/");
    }
    requireValue(finding.line === undefined
      || (Number.isSafeInteger(finding.line) && finding.line > 0), "invalid finding line");
  }
  return findings;
}

export function evidence(source, checks, findings, seed = seedFor(source)) {
  categoryLabel(source.category);
  issueNumber(source.id);
  requireValue(SHA.test(source.commit) && typeof source.cycle === "string", "invalid evidence source");
  requireValue(Number.isSafeInteger(checks) && checks > 0, "no checks ran");
  requireValue(Number.isInteger(seed) && seed >= 0 && seed <= 0xffff_ffff, "invalid seed");
  validateFindings(findings, source.category);
  return { schema: "bureau-maintenance-evidence-v1", source, complete: true, checks, seed, findings };
}

export function validateEvidence(value) {
  requireValue(value?.schema === "bureau-maintenance-evidence-v1" && value.complete === true,
    "missing or incomplete deterministic evidence");
  const canonical = evidence(value.source, value.checks, value.findings, value.seed);
  requireValue(JSON.stringify(canonical).length <= 48_000, "evidence exceeds the issue budget");
  return canonical;
}

export function fingerprint(value) {
  const checked = validateEvidence(value);
  return digest({
    category: checked.source.category, id: checked.source.id, commit: checked.source.commit,
    findings: checked.findings.map(({ id, path }) => ({ id, path }))
      .sort((left, right) => left.id.localeCompare(right.id, "en")),
  });
}

export function findingMarker(value) {
  return `<!-- bureau-maintenance-finding:${fingerprint(value)} -->`;
}

export function findingBody(value) {
  const checked = validateEvidence(value);
  requireValue(checked.findings.length > 0, "cannot draft an empty finding");
  const details = checked.findings.map((finding) =>
    `- **${finding.title}** (\`${finding.path}${finding.line ? `:${finding.line}` : ""}\`): ${finding.detail}`);
  return [
    findingMarker(checked), `Source: ${checked.source.id} at \`${checked.source.commit}\`.`,
    `Reproduction seed: \`${checked.seed}\`. Checks executed: ${checked.checks}.`,
    ...details, "", "Acceptance: reproduce the recorded failure, make a bounded patch, and pass",
    "the same deterministic check plus the repository gates. No automatic merge.",
    "", block(EVIDENCE, checked),
  ].join("\n");
}

export function readEvidence(body) {
  return validateEvidence(readBlock(body, EVIDENCE));
}

export function reportBody(value, publication = null) {
  const checked = validateEvidence(value);
  return block("bureau-maintenance-report", {
    evidence: checked, evidence_sha256: digest(checked), publication,
  });
}

export function stepResult(outcome, outputs = {}, message = "", artifacts = []) {
  return { schema: "v2", outcome, outputs, artifacts, trust: "derived", message };
}
