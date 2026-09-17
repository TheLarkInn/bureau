import assert from "node:assert/strict";
import test from "node:test";

import { LABELS, evidence, findingBody, intentBlock, labels, reportBody } from "./maintenance-contract.mjs";
import { draft, handoff, clear } from "./maintenance-publish.mjs";
import { checkSource, observe, verifyDraft, verifyFix, verifyHandoff } from "./maintenance-lifecycle.mjs";
import { POLICY, fixture, fakeForge } from "./maintenance-test-support.mjs";

test("detection -> inert draft -> independent verification -> selective handoff -> verified fix", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  const receipt = await draft(forge.api, value, POLICY);
  assert.deepEqual(labels(forge.issues.get(42)), []);
  const verified = verifyDraft(await observe(forge.api, "chaos", POLICY), value, POLICY);
  assert.deepEqual(verified, receipt);
  await handoff(forge.api, value, verified, POLICY);
  const observed = await observe(forge.api, "chaos", POLICY);
  verifyHandoff(observed, value, receipt, POLICY);
  assert.deepEqual(verifyFix(forge.issues.get(42), observed.source, observed.comments, "chaos", POLICY), value);
  assert.equal(labels(observed.source).includes("human-context"), true);
});

test("no findings still require an independently observed source report and cleared scan", async () => {
  const { source, value } = fixture("site-responsive", []);
  const forge = fakeForge(source);
  await clear(forge.api, value, POLICY);
  verifyHandoff(await observe(forge.api, "site-responsive", POLICY), value, null, POLICY);
  assert.equal(forge.writes.filter((write) => write.path === "/issues").length, 0);
});

test("same finding draft deduplicates without another create", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  assert.deepEqual(await draft(forge.api, value, POLICY), await draft(forge.api, value, POLICY));
  assert.equal(forge.writes.filter((write) => write.path === "/issues").length, 1);
});

test("a lost create response is observed and adopted, never retried", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  forge.loseResponse("POST", "/issues");
  assert.equal((await draft(forge.api, value, POLICY)).url, "https://github.com/TheLarkInn/bureau/issues/42");
  assert.equal(forge.writes.filter((write) => write.path === "/issues").length, 1);
});

test("an unresolvable create remains indeterminate with only one POST", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  let posts = 0;
  forge.api.request = async (method, path) => {
    if (path === "/user") return { login: POLICY.issuer_login, id: POLICY.issuer_id };
    posts += 1;
    throw new Error("lost before observation");
  };
  await assert.rejects(draft(forge.api, value, POLICY), /indeterminate/u);
  assert.equal(posts, 1);
});

test("duplicate markers, tampered evidence and premature ready labels cannot establish a draft", async () => {
  for (const mutate of [
    (forge) => forge.issues.set(43, { ...forge.issues.get(42), number: 43 }),
    (forge) => { forge.issues.get(42).body += "\nModel claims everything passed."; },
    (forge) => { forge.issues.get(42).labels.push(LABELS.ready); },
    (forge) => { forge.issues.get(42).user.id = 18; },
  ]) {
    const { source, value } = fixture();
    const forge = fakeForge(source);
    await draft(forge.api, value, POLICY);
    mutate(forge);
    const observed = await observe(forge.api, "chaos", POLICY);
    assert.throws(() => verifyDraft(observed, value, POLICY));
  }
});

test("source approval revocation stops handoff before any write and rejects a later fix", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  const receipt = await draft(forge.api, value, POLICY);
  const before = forge.writes.length;
  forge.issues.get(7).labels = labels(source).filter((label) => label !== LABELS.approved);
  await assert.rejects(handoff(forge.api, value, receipt, POLICY), /approved/u);
  assert.equal(forge.writes.length, before);
  forge.issues.get(42).labels.push(LABELS.fix, LABELS.ready);
  assert.throws(() => verifyFix(forge.issues.get(42), forge.issues.get(7), [], "chaos", POLICY));
});

test("closed or rejected findings are reported without reopening or authorizing a fix", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  await draft(forge.api, value, POLICY);
  forge.issues.get(42).state = "closed";
  const receipt = verifyDraft(await observe(forge.api, "chaos", POLICY), value, POLICY);
  await handoff(forge.api, value, receipt, POLICY);
  assert.deepEqual([forge.issues.get(42).state, labels(forge.issues.get(42))], ["closed", []]);
});

test("a later scan of a previously rejected finding stays deduplicated and never reopens it", async () => {
  const { source, value, intent } = fixture();
  const forge = fakeForge(source);
  await draft(forge.api, value, POLICY);
  forge.issues.get(42).state = "closed";
  const receipt = verifyDraft(await observe(forge.api, "chaos", POLICY), value, POLICY);
  await handoff(forge.api, value, receipt, POLICY);
  const cycle = "2026-09-17T18:00:00Z";
  const freshSource = forge.issues.get(7);
  freshSource.body = source.body.replace(intentBlock(intent), intentBlock({ ...intent, cycle }));
  freshSource.labels = labels(source);
  const later = evidence({ ...value.source, cycle }, 8, value.findings);
  const observedAt = "2026-09-17T18:01:00Z";
  const repeated = verifyDraft(await observe(forge.api, "chaos", POLICY), later, POLICY, observedAt);
  await handoff(forge.api, later, repeated, POLICY, observedAt);
  assert.deepEqual([forge.issues.get(42).state, labels(forge.issues.get(42))], ["closed", []]);
  assert.equal(forge.writes.filter((write) => write.path === "/issues").length, 1);
});

test("a model cannot authorize altered evidence with its own newly written prior-report claim", async () => {
  const { source, value } = fixture();
  const forge = fakeForge(source);
  const receipt = await draft(forge.api, value, POLICY);
  const altered = { ...value, checks: value.checks + 1 };
  forge.issues.get(42).body = findingBody(altered);
  forge.comments.get(7).push({ body: reportBody(altered, receipt),
    user: { login: POLICY.issuer_login, id: POLICY.issuer_id },
    created_at: "2026-09-17T12:01:00Z", updated_at: "2026-09-17T12:01:00Z" });
  assert.throws(() => verifyDraft({
    source, issues: [...forge.issues.values()], comments: forge.comments.get(7),
  }, value, POLICY, "2026-09-17T12:00:01Z"), /deterministic evidence/u);
});

test("marker text and a copied author login cannot substitute for issuer identity or canonical source", () => {
  const { source, value } = fixture();
  assert.throws(() => checkSource({ ...source, number: 70 }, "chaos", POLICY));
  const finding = { number: 42, html_url: "https://github.com/TheLarkInn/bureau/issues/42",
    state: "open", user: { login: POLICY.issuer_login, id: POLICY.issuer_id },
    labels: ["bureau:maintenance-chaos", LABELS.fix, LABELS.ready], body: findingBody(value) };
  const comments = [{ body: reportBody(value, { url: finding.html_url, fingerprint: "fabricated", disposition: "open" }),
    user: { login: POLICY.issuer_login, id: 18 } }];
  assert.throws(() => verifyFix(finding, source, comments, "chaos", POLICY));
});
