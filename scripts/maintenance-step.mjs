import { realpath } from "node:fs/promises";

import {
  LABELS, categoryLabel, issueNumber, labels, requireValue, sourcePin, stepResult, validateEvidence,
} from "./maintenance-contract.mjs";
import { github } from "./maintenance-http.mjs";
import { loadPolicy } from "./maintenance-policy.mjs";
import { checkSource, liveFix, observe, verifyDraft, verifyHandoff } from "./maintenance-lifecycle.mjs";
import { CheckFailure, git, requireClean, requirePatch, runCheck, saveEvidence, workspace } from "./maintenance-checks.mjs";
import { stepDeadline } from "./maintenance-deadline.mjs";

async function intake(category, request, policy, api) {
  const number = issueNumber(request.item.external_id);
  const issue = await api.issue(number);
  const observedAt = issue.updated_at;
  requireValue(Number.isFinite(Date.parse(observedAt)), "forge source timestamp was not observed");
  requireValue(labels(issue).includes(categoryLabel(category)), "work item has the wrong category");
  const state = workspace();
  requireValue(!state.status, "maintenance requires a clean initial worktree");
  if (labels(issue).includes(LABELS.fix)) {
    const recorded = await liveFix(api, request, category, policy);
    git(["merge-base", "--is-ancestor", recorded.source.commit, state.commit]);
    return stepResult("no-work", { maintenance_source: { ...recorded.source, commit: state.commit },
      maintenance_observed_at: observedAt,
      maintenance_finding: recorded }, "route the verified finding to bounded reproduction");
  }
  checkSource(issue, category, policy);
  return stepResult("success", {
    maintenance_source: sourcePin(request, category, state.commit, issue),
    maintenance_observed_at: observedAt,
  }, "recorded the exact approved source and cycle");
}

async function scan(category, request, policy, api) {
  const source = request.inputs.maintenance_source;
  checkSource(await api.issue(policy.source_issues[category]), category, policy, source);
  requireClean(source);
  const checked = await runCheck(source, policy, { deadline: stepDeadline(category, request.step) });
  requireClean(source);
  checkSource(await api.issue(policy.source_issues[category]), category, policy, source);
  const artifacts = await saveEvidence(checked.evidence, checked.log, request.step);
  return stepResult(checked.evidence.findings.length ? "success" : "no-work",
    { maintenance_evidence: checked.evidence }, "bounded deterministic scan completed", artifacts);
}

async function verifyPublication(category, request, policy, api) {
  const value = validateEvidence(request.inputs.maintenance_evidence);
  requireClean(request.inputs.maintenance_source);
  const observed = await observe(api, category, policy);
  if (request.step === "verify-draft") {
    const receipt = verifyDraft(observed, value, policy, request.inputs.maintenance_observed_at);
    requireValue(JSON.stringify(receipt) === JSON.stringify(request.inputs.maintenance_publication),
      "agent publication claim differs from independently observed forge state");
    return stepResult("success", { maintenance_draft: receipt }, "draft independently verified; no handoff yet");
  }
  const expected = request.step === "verify-clear" ? null : request.inputs.maintenance_draft;
  verifyHandoff(observed, value, expected, policy);
  return stepResult("success", {}, "live source, issuer, evidence, dedup and handoff verified");
}

async function fixCheck(category, request, policy, api) {
  await liveFix(api, request, category, policy);
  const source = request.inputs.maintenance_source;
  if (request.step !== "reproduce") requirePatch(source);
  const checked = await runCheck(source, policy, { seed: request.inputs.maintenance_finding.seed,
    deadline: stepDeadline(category, request.step) });
  await liveFix(api, request, category, policy);
  const artifacts = await saveEvidence(checked.evidence, checked.log, request.step);
  if (request.step === "reproduce") {
    requireClean(source);
    const original = request.inputs.maintenance_finding.findings;
    const reproduced = checked.evidence.findings.some((finding) => original.some((item) => item.id === finding.id));
    return stepResult(reproduced ? "success" : "blocked",
      { maintenance_reproduction: checked.evidence },
      reproduced ? "live approval checked and recorded failure reproduced" : "finding did not reproduce; human review required",
      artifacts);
  }
  return stepResult(checked.evidence.findings.length ? "failure" : "success",
    { maintenance_validation: checked.evidence }, "patch checked against the same deterministic detector", artifacts);
}

async function fullGates(category, request, policy, api) {
  await liveFix(api, request, category, policy);
  requirePatch(request.inputs.maintenance_source);
  const checked = await runCheck(request.inputs.maintenance_source, policy,
    { gates: true, seed: request.inputs.maintenance_finding.seed, deadline: stepDeadline(category, request.step) });
  await liveFix(api, request, category, policy);
  requirePatch(request.inputs.maintenance_source);
  const artifacts = await saveEvidence({ gates: "passed" }, checked.log, "full-gates");
  return stepResult("success", {}, "repository gates passed; existing engine owns PR publication", artifacts);
}

export async function executeStep(category, request, policy, api) {
  requireValue(request?.schema === "v2", "maintenance requires the v2 step contract");
  requireValue(await realpath(request.worktree) === await realpath(process.cwd()), "worktree identity mismatch");
  if (request.step === "intake") return intake(category, request, policy, api);
  requireValue(request.inputs?.maintenance_source?.category === category, "missing deterministic source pin");
  if (request.step === "detect") return scan(category, request, policy, api);
  if (["verify-draft", "verify-handoff", "verify-clear"].includes(request.step)) {
    return verifyPublication(category, request, policy, api);
  }
  if (["reproduce", "validate-patch"].includes(request.step)) return fixCheck(category, request, policy, api);
  if (request.step === "full-gates") return fullGates(category, request, policy, api);
  throw new Error(`unknown deterministic maintenance step: ${request.step}`);
}

export async function failureResult(error, request, persist = saveEvidence) {
  if (!(error instanceof CheckFailure)) return stepResult("blocked", {}, error.message);
  try {
    const value = { complete: false, source: request.inputs?.maintenance_source, message: error.message };
    const artifacts = await persist(value, error.log, request.step);
    return stepResult("blocked", {}, error.message, artifacts);
  } catch (persistence) {
    return stepResult("blocked", {},
      `${error.message}; failed to preserve check evidence: ${persistence.message}`);
  }
}

export async function runStep(category, request) {
  try {
    const result = await executeStep(category, request, await loadPolicy(), github());
    console.log(JSON.stringify(result));
  } catch (error) {
    console.log(JSON.stringify(await failureResult(error, request)));
  }
}
