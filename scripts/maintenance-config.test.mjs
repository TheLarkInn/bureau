import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

import { CATEGORIES, LABELS } from "./maintenance-contract.mjs";
import { durableFilesystem } from "../deployment/check.mjs";
import { readOnlyMount } from "./maintenance-tools.mjs";

const read = async (path) => (await readFile(new URL(`../${path}`, import.meta.url), "utf8")).replace(/\r\n/gu, "\n");

test("isolated profile has exactly three selective bounded assignments, never ordinary root assignment copies", async () => {
  const directory = new URL("../.bureau/maintenance/assignments/", import.meta.url);
  assert.deepEqual((await readdir(directory)).sort(), CATEGORIES.map((category) => `maintenance-${category}.yaml`).sort());
  for (const category of CATEGORIES) {
    const assignment = await read(`.bureau/maintenance/assignments/maintenance-${category}.yaml`);
    for (const required of [`name: maintenance-${category}`, `pipeline: maintenance-${category}`,
      'source: TheLarkInn/bureau', `label:"bureau:maintenance-${category}"`,
      `approval_label: ${LABELS.ready}`, "max_concurrent: 1", "max_runs_per_hour: 2",
      "max_runs_per_day: 6", "max_open_prs: 2", "max_cost_per_day_usd: 10", "max_run_hours: 1"]) {
      assert.equal(assignment.includes(required), true, required);
    }
    assert.match(assignment, /-label:agent-eligible/u);
    assert.match(assignment, /-label:"bureau:design-scan"/u);
  }
});

test("every pipeline wires reporting, independent handoff verification, reproduction, patch validation and gates", async () => {
  for (const category of CATEGORIES) {
    const pipeline = await read(`.bureau/maintenance/pipelines/maintenance-${category}.yaml`);
    for (const step of ["intake", "detect", "report-findings", "verify-draft", "handoff",
      "verify-handoff", "report-clean", "verify-clear", "reproduce", "implement", "validate-patch", "repair", "full-gates"]) {
      assert.equal(pipeline.includes(`- name: ${step}\n`), true, `${category}:${step}`);
    }
    for (const protectedInputs of ["inputs_from: [report-findings, intake, detect]",
      "inputs_from: [handoff, intake, detect, verify-draft]", "inputs_from: [report-clean, intake, detect]"]) {
      assert.equal(pipeline.includes(protectedInputs), true, "deterministic inputs must override agent outputs");
    }
    assert.match(pipeline, /execFileSync\("git".*--no-ext-diff.*--exit-code/u);
    assert.match(pipeline, /max_attempts: 2/u);
    assert.equal(pipeline.includes(`runStep("${category}",r)`), true);
  }
});

test("fixer roles never get forge/push/merge grants; reporting permissions remain separately scoped", async () => {
  for (const category of CATEGORIES) {
    const role = await read(`.bureau/maintenance/roles/maintenance-${category}-fixer.yaml`);
    assert.match(role, /permissions: \[repo:read, repo:write, model:invoke\]/u);
    assert.doesNotMatch(role, /issues:|pr:|repo:push/u);
  }
  const reporter = await read(".bureau/maintenance/roles/maintenance-reporter.yaml");
  assert.match(reporter, /issues:read, issues:write/u);
  assert.doesNotMatch(reporter, /pr:merge|repo:push/u);
});

test("every named resource matches the actual Bureau loader's filename identity rule", async () => {
  for (const kind of ["assignments", "pipelines", "roles"]) {
    const directory = new URL(`../.bureau/maintenance/${kind}/`, import.meta.url);
    for (const file of await readdir(directory)) {
      const text = await read(`.bureau/maintenance/${kind}/${file}`);
      const name = /^name: ([a-z-]+)$/mu.exec(text)?.[1];
      assert.equal(file, `${name}.yaml`, `${kind}/${file}`);
    }
  }
});

test("external wake cannot create issues, approve work, or dispatch Bureau runs", async () => {
  const workflow = await read(".github/workflows/maintenance.yml");
  assert.match(workflow, /BUREAU_MAINTENANCE_ENABLED == 'true'/u);
  assert.match(workflow, /github\.ref == format/u);
  assert.match(workflow, /issues: write/u);
  assert.match(workflow, /persist-credentials: false/u);
  assert.doesNotMatch(workflow, /pull_request|bureau reconcile|bureau run|contents: write/u);
  const updater = await read("scripts/update-maintenance-intent.mjs");
  assert.doesNotMatch(updater, /request\("POST", "\/issues"/u);
  assert.doesNotMatch(updater, /labels: \[LABELS\.(approved|ready)\]/u);
});

test("service and container use the same single owner and explicit maintenance-only source", async () => {
  const service = await read("deployment/bureau-maintenance.service");
  const compose = await read("deployment/compose.yaml");
  const launcher = await read("deployment/run-owner.sh");
  assert.match(service, /flock --nonblock --no-fork .*\/owner\.lock/u);
  assert.match(service, /MemoryMax=8G/u);
  assert.match(service, /CPUQuota=200%/u);
  assert.match(service, /TasksMax=256/u);
  assert.match(service, /KillMode=mixed/u);
  assert.match(service, /TimeoutStopSec=70min/u);
  assert.match(compose, /read_only: true/u);
  assert.match(compose, /pids_limit: 256/u);
  assert.doesNotMatch(compose, /^\s+ports:|privileged: true/mu);
  assert.match(launcher, /--config-subdir \.bureau\/maintenance/u);
  assert.match(launcher, /BUREAU_DEPLOYMENT_APPROVED/u);
  assert.doesNotMatch(launcher, /\brm\b|repair|git clean|truncate/u);
});

test("durable admission rejects temporary/Windows filesystems and tool writability", () => {
  for (const type of [0xef53, 0x58465342, 0x9123683e]) assert.equal(durableFilesystem(type), true);
  for (const type of [0x01021994, 0x01021997, 0x794c7630, 0, NaN]) assert.equal(durableFilesystem(type), false);
  const mounts = "1 0 8:1 / / ro,relatime - ext4 /dev/disk rw\n"
    + "2 1 8:1 /tools /opt/tools rw,relatime - ext4 /dev/disk rw\n";
  assert.equal(readOnlyMount(mounts, "/opt/browser"), true);
  assert.equal(readOnlyMount(mounts, "/opt/tools"), false);
  assert.equal(readOnlyMount(mounts, "/opt/tools/package"), false);
});
