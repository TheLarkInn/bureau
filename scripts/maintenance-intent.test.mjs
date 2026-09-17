import assert from "node:assert/strict";
import test from "node:test";

import { LABELS, intentBlock, labels, readIntent } from "./maintenance-contract.mjs";
import { cycleAt, trustedWorkflow, updateIntent } from "./update-maintenance-intent.mjs";
import { clear } from "./maintenance-publish.mjs";
import { COMMIT, POLICY, fakeForge, fixture } from "./maintenance-test-support.mjs";

test("external intent uses stable six-hour UTC cycles, not event IDs", () => {
  assert.deepEqual(["2026-09-17T12:01:00Z", "2026-09-17T17:59:59Z", "2026-09-17T18:00:00Z"]
    .map((date) => cycleAt(new Date(date))),
  ["2026-09-17T12:00:00Z", "2026-09-17T12:00:00Z", "2026-09-17T18:00:00Z"]);
});

test("updater selects only trusted default-branch schedules/dispatches in this exact repository", () => {
  const environment = { GITHUB_REPOSITORY: "TheLarkInn/bureau", GITHUB_EVENT_NAME: "schedule",
    GITHUB_REF: "refs/heads/main", BUREAU_DEFAULT_BRANCH: "main",
    BUREAU_MAINTENANCE_ENABLED: "true", GITHUB_SHA: COMMIT };
  assert.equal(trustedWorkflow(environment), undefined);
  for (const changed of [{ GITHUB_EVENT_NAME: "pull_request" }, { GITHUB_EVENT_NAME: "pull_request_target" },
    { GITHUB_REF: "refs/heads/unreviewed" }, { GITHUB_REPOSITORY: "outside/bureau" },
    { BUREAU_MAINTENANCE_ENABLED: "" }, { GITHUB_SHA: "main" }]) {
    assert.throws(() => trustedWorkflow({ ...environment, ...changed }));
  }
});

test("recurrence updates one exact source, preserves prose/unrelated labels, and deduplicates pending work", async () => {
  const { source } = fixture();
  const forge = fakeForge(source);
  const date = new Date("2026-09-17T18:20:00Z");
  await updateIntent(forge.api, "chaos", COMMIT, date, POLICY);
  const writes = forge.writes.length;
  await updateIntent(forge.api, "chaos", COMMIT, date, POLICY);
  assert.equal(forge.writes.length, writes);
  assert.equal(labels(forge.issues.get(7)).includes("human-context"), true);
  assert.match(forge.issues.get(7).body, /Human instructions stay here\./u);
  assert.equal(readIntent(forge.issues.get(7).body, "chaos").cycle, "2026-09-17T18:00:00Z");
});

test("completed same-cycle scans remain complete; next cycle re-arms the persistent source", async () => {
  const { source, value } = fixture("chaos", []);
  const forge = fakeForge(source);
  await clear(forge.api, value, POLICY);
  const writes = forge.writes.length;
  await updateIntent(forge.api, "chaos", COMMIT, new Date("2026-09-17T13:00:00Z"), POLICY);
  assert.equal(forge.writes.length, writes);
  await updateIntent(forge.api, "chaos", COMMIT, new Date("2026-09-17T18:00:00Z"), POLICY);
  assert.equal(labels(forge.issues.get(7)).includes(LABELS.scan), true);
  assert.equal(labels(forge.issues.get(7)).includes(LABELS.reported), false);
});

test("no source approval is granted, and an outsider copied marker is rejected", async () => {
  for (const mutate of [
    (source) => { source.labels = labels(source).filter((label) => label !== LABELS.approved); },
    (source) => { source.user.id = 18; },
    (source) => { source.labels.push(LABELS.human); },
  ]) {
    const { source } = fixture();
    mutate(source);
    const forge = fakeForge(source);
    await assert.rejects(updateIntent(forge.api, "chaos", COMMIT, new Date(), POLICY));
    assert.equal(forge.writes.length, 0);
  }
});

test("approval is re-read before writing; partially updated intent can recover without another body mutation", async () => {
  const { source, intent } = fixture();
  const forge = fakeForge(source);
  let reads = 0;
  forge.onRead(() => {
    reads += 1;
    if (reads === 2) forge.issues.get(7).labels = labels(source).filter((label) => label !== LABELS.approved);
  });
  await assert.rejects(updateIntent(forge.api, "chaos", COMMIT, new Date("2026-09-17T18:00:00Z"), POLICY));
  assert.equal(forge.writes.length, 0);
  const partial = fakeForge({ ...source,
    body: source.body.replace(intentBlock(intent), intentBlock({ ...intent, cycle: "2026-09-17T18:00:00Z" })),
    labels: labels(source).filter((label) => label !== LABELS.scan) });
  await updateIntent(partial.api, "chaos", COMMIT, new Date("2026-09-17T18:00:00Z"), POLICY);
  assert.equal(partial.writes.some((write) => write.method === "PATCH"), false);
  assert.equal(labels(partial.issues.get(7)).includes(LABELS.scan), true);
});
