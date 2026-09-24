import assert from "node:assert/strict";
import test from "node:test";

import {
  LABELS, evidence, findingBody, fingerprint, issueNumber, readEvidence, sourcePin, urlNumber, validateEvidence,
} from "./maintenance-contract.mjs";
import { checkSource } from "./maintenance-lifecycle.mjs";
import { validatePolicy } from "./maintenance-policy.mjs";
import { COMMIT, POLICY, fixture } from "./maintenance-test-support.mjs";

test("source pin requires the exact approved issuer, repo, issue, current body and commit", () => {
  const { request, source, value } = fixture();
  assert.deepEqual(sourcePin(request, "chaos", COMMIT, source), value.source);
  for (const changed of [
    { ...source, user: { login: "outside", id: 17 } },
    { ...source, user: { login: source.user.login, id: 18 } },
    { ...source, number: 70 },
    { ...source, labels: source.labels.filter((label) => label !== LABELS.approved) },
    { ...source, state: "closed" },
  ]) assert.throws(() => checkSource(changed, "chaos", POLICY));
  assert.throws(() => sourcePin(request, "chaos", "2".repeat(40), source), /checkout differs/u);
  assert.throws(() => sourcePin(request, "chaos", COMMIT, { ...source, body: `${source.body}changed` }), /changed/u);
});

test("work-item and publication identifiers cannot select another source", () => {
  for (const id of ["someone/bureau#7", "TheLarkInn/bureau#0", "7", "TheLarkInn/bureau#7/../8"]) {
    assert.throws(() => issueNumber(id));
  }
  for (const url of ["http://github.com/TheLarkInn/bureau/issues/7",
    "https://github.com/TheLarkInn/bureau/pull/7", "https://example.com/issues/7",
    "https://github.com/TheLarkInn/bureau/issues/7?redirect=8"]) assert.throws(() => urlNumber(url));
});

test("detector evidence is finite, complete and canonical", () => {
  const { value } = fixture("site-accessibility");
  assert.deepEqual(readEvidence(findingBody(value)), value);
  for (const changed of [
    { ...value, complete: false }, { ...value, checks: 0 }, { ...value, checks: "8" },
    { ...value, findings: [...value.findings, ...value.findings] },
    { ...value, findings: [{ ...value.findings[0], path: "../outside" }] },
    { ...value, findings: [{ ...value.findings[0], path: "crates/outside.rs" }] },
    { ...value, findings: [{ ...value.findings[0], detail: "" }] },
  ]) assert.throws(() => validateEvidence(changed));
});

test("recurring cycles retain dedup, while changed source or finding identities do not", () => {
  const { value } = fixture();
  const later = evidence({ ...value.source, cycle: "2026-09-17T18:00:00Z" }, 8, value.findings);
  assert.equal(fingerprint(later), fingerprint(value));
  const otherCommit = evidence({ ...value.source, commit: "2".repeat(40) }, 8, value.findings);
  assert.notEqual(fingerprint(otherCommit), fingerprint(value));
});

test("unconfigured identities and source IDs fail closed", () => {
  assert.equal(validatePolicy(POLICY), POLICY);
  for (const changed of [
    { ...POLICY, issuer_id: null }, { ...POLICY, issuer_login: "outside" },
    { ...POLICY, source_issues: { chaos: 7, "site-accessibility": 7, "site-responsive": 9 } },
    { ...POLICY, source_issues: {} }, { ...POLICY, cargo_cache_max_bytes: Infinity },
    { ...POLICY, cargo_cache_max_bytes: 3 * 1024 ** 3 },
  ]) assert.throws(() => validatePolicy(changed));
});
