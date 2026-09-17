import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { promisify } from "node:util";
import { authoringProvenance, changedPaths } from "../lib/provenance.mjs";
import { MAX_LISTING_BYTES, MAX_RUN_DIRECTORIES, MAX_RUN_LOG_BYTES, observeRun, readRunListing, readRunLog } from "../lib/run-observation.mjs";
import { inspectEventLog, inspectEvents, MAX_RUN_LOG_EVENTS, runListingResult } from "../web/run-evidence.mjs";
import { filterOperationRuns, operationRun, operationsOverview, safeguards, STALE_EVIDENCE_MS } from "../web/operations.mjs";
import { navigationTarget } from "../web/navigation.mjs";
import { canvasAccess, mutationRefusal } from "../web/access-policy.mjs";

const NOW = 1_800_000_000_000;
const SOURCE = { remote: "https://example.invalid/config.git", reference: "main", commit: "abc123" };
const ASSIGNMENT = { name: "fix-tests", pipeline: "repair", limits: {}, repos: ["work"], work: {} };
const STATE = { dir: ".bureau", validation: { state: "validated", ok: true }, pipelines: { repair: {} },
  config: { view: { assignments: [ASSIGNMENT] } } };

function events(runId = "run-test", tail = []) {
  return [{ kind: "run_started", data: { run_id: runId, assignment: ASSIGNMENT.name,
    snapshot: { pipeline: { name: "repair" }, config_source: SOURCE } } }, ...tail]
    .map((event, seq) => ({ seq, at_ms: NOW - 1000 + seq, ...event }));
}

function observed(tail = []) {
  return observeRun("run-test", { events: events("run-test", tail), error: null, warning: null });
}

test("overview separates sample, unvalidated, accepted, rejected, and pending authoring", () => {
  const cases = [
    [{ state: "fixture", ok: true }, "Sample configuration"],
    [{ state: "crash", ok: false, message: "CLI output was invalid" }, "Validation unavailable"],
    [{ state: "validated", ok: true }, "Working tree validated"],
    [{ state: "validated", ok: false, errors: ["Broken pipeline"] }, "Invalid configuration"],
  ];
  for (const [validation, verdict] of cases) {
    const model = operationsOverview({ ...STATE, validation, plan: { writes: ["draft.yaml"], removals: [] } }, {});
    assert.deepEqual([model.configuration.verdict, model.configuration.pending, model.configuration.errors], [verdict, 1, validation.errors ?? []]);
  }
});

test("configured safeguards preserve zero, omission, approval, and the default deadline", () => {
  const result = safeguards({ ...ASSIGNMENT, limits: { maxCostPerDayUsd: 0 }, work: { approvalLabel: "reviewed" } });
  assert.deepEqual(result.slice(0, 7), ["concurrent runs: unlimited", "runs/hour: unlimited", "runs/day: unlimited",
    "open PRs: unlimited", "USD/day: 0", "run deadline: 24h (default)", "approval label: reviewed"]);
});

test("read-only policy is explicit, cannot be relaxed by input, and invalid environment fails closed", () => {
  assert.deepEqual([undefined, "0", "1", "", "unexpected"].map((setting) =>
    canvasAccess({ readOnly: false }, { readOnly: false }, setting).mode),
  ["local", "local", "read-only", "read-only", "read-only"]);
  const access = canvasAccess({ readOnly: true });
  assert.deepEqual(["navigate", "operations", "reconcile-now", "retry", "save-plan", "unknown-write"]
    .map((name) => Boolean(mutationRefusal(access, name))), [false, false, true, true, true, true]);
});

test("missing, unknown, and incomplete access policies never authorize mutations", () => {
  const policies = [undefined, null, {}, { mode: "unknown" }, { mode: "read-only" }, { mode: "read-only", reason: "" }];
  assert.deepEqual(policies.map((access) => [
    Boolean(mutationRefusal(access, "save", "action")),
    Boolean(mutationRefusal(access, "save-plan")),
    mutationRefusal(access, "describe", "action"),
  ]), policies.map(() => [true, true, null]));
  assert.deepEqual(["save", "create", "set_field"].map((name) =>
    mutationRefusal(canvasAccess(), name, "action")), [null, null, null]);
});

test("ordinary observed active, paused, resumed, failed and human-needed states are distinct", () => {
  const pause = { kind: "output", data: { stream: "run", data: "run paused at a step boundary: review required" } };
  const start = { kind: "step_started", data: { step: "verify" } };
  const cases = [
    [[start], "active"], [[pause], "paused"], [[pause, start], "active"],
    [[{ kind: "run_finished", data: { outcome: "failure", terminal: "abort" } }], "failed"],
    [[{ kind: "run_finished", data: { outcome: "blocked", terminal: "escalate" } }], "attention"],
    [[{ kind: "run_finished", data: { outcome: "success", terminal: "done" } }], "completed"],
    [[{ kind: "run_finished", data: { outcome: "new-outcome" } }], "attention"],
  ];
  assert.deepEqual(cases.map(([tail]) => operationRun(observed(tail), STATE, NOW).status), cases.map(([, status]) => status));
});

test("freshness is bounded at five minutes and never asserts that the daemon stopped", () => {
  const run = observed();
  const at = run.evidence.last_at_ms;
  assert.deepEqual([at + STALE_EVIDENCE_MS, at + STALE_EVIDENCE_MS + 1, at - 1]
    .map((now) => operationRun(run, STATE, now).label), ["Active (observed)", "Unfinished, stale evidence", "Unfinished, stale evidence"]);
});

test("unknown accounting stays unknown, while explicitly measured zero is retained", () => {
  const costs = [undefined, null, 0, 1.25, -1, "0", NaN];
  assert.deepEqual(costs.map((cost_usd) => operationRun(observed([
    { kind: "run_finished", data: { outcome: "success", cost_usd } },
  ]), STATE, NOW).cost), ["Unknown", "Unknown", "0.00 USD", "1.25 USD", "Unknown", "Unknown", "Unknown"]);
});

test("recorded pipeline/source wins over current assignment and absent pipelines have no misleading link", () => {
  const state = { ...STATE, pipelines: {}, config: { view: { assignments: [{ ...ASSIGNMENT, pipeline: "changed" }] } } };
  const model = operationsOverview(state, { runs: [observed()], observation: { state: "ready" } }, NOW);
  assert.deepEqual([model.runs[0].pipeline, model.runs[0].target, model.recorded_sources], ["repair", null, [SOURCE]]);
});

test("attention comes first and search/filter acts on run identity and actual state", () => {
  const active = observed();
  const failed = { ...observed([{ kind: "run_finished", data: { outcome: "failure" } }]), run_id: "older-failed" };
  const model = operationsOverview(STATE, { runs: [active, failed], observation: { state: "ready" } }, NOW);
  assert.deepEqual([model.runs.map((run) => run.run_id), model.counts, filterOperationRuns(model.runs, "failed", "REPAIR").length],
    [["older-failed", "run-test"], { attention: 1, active: 1, paused: 0, failed: 1 }, 1]);
});

test("missing observation is not a healthy zero and a readable empty directory is", () => {
  assert.deepEqual([{}, { observation: { state: "missing" } }, { observation: { state: "error" } },
    { observation: { state: "ready" } }].map((listing) => operationsOverview(STATE, listing).counts),
  [null, null, null, { attention: 0, active: 0, paused: 0, failed: 0 }]);
});

test("the shared Live and Replay listing does not turn limited or missing observations into healthy zero", () => {
  assert.deepEqual([undefined, "ready", "missing", "limited", "error"].map((state) =>
    runListingResult({ runs: [], ...(state ? { observation: { state } } : {}) }).status),
  ["ready", "ready", "error", "error", "error"]);
  assert.throws(() => runListingResult({ observation: { state: "ready" } }), /no run array/u);
});

test("complete corrupt records fail, only a torn last append is tolerated with a warning", () => {
  const first = JSON.stringify(events()[0]);
  const complete = inspectEventLog(`${first}\nnot-json\n`);
  const torn = inspectEventLog(`${first}\n{"seq":1`);
  assert.deepEqual([Boolean(complete.error), complete.warning, torn.error, Boolean(torn.warning), torn.events.length],
    [true, null, null, true, 1]);
});

test("physical framing distinguishes malformed envelopes from an unframed final tail", () => {
  const first = `${JSON.stringify(events()[0])}\n`;
  const malformed = '{"seq":"invalid","at_ms":0,"kind":"output","data":{}}';
  const framed = inspectEventLog(`${first}${malformed}\n`);
  const tail = inspectEventLog(`${first}${malformed}`);
  const badPrefix = inspectEventLog(`${first}${malformed}\n{"seq":`);
  assert.deepEqual([Boolean(framed.error), tail.error, Boolean(tail.warning), tail.events.length, Boolean(badPrefix.error)],
    [true, null, true, 1, true]);
});
test("malformed envelopes, unsupported kinds and payload substitutes are explicit errors", () => {
  const valid = events();
  const cases = [null, [null], [{ ...valid[0], seq: -1 }], [{ ...valid[0], seq: 0.5 }], [{ ...valid[0], data: [] }],
    [{ ...valid[0], kind: "future-event" }], [{ ...valid[0], at_ms: Number.MAX_SAFE_INTEGER }],
    events("run-test", [{ kind: "group_started", data: { members: "wrong" } }])];
  assert.equal(cases.every((input) => typeof inspectEvents(input) === "string"), true);
});

test("supported sequence gaps, duplicates and nonzero starts preserve file-order replay", () => {
  const records = events("run-test", [
    { kind: "step_started", data: { step: "verify" } },
    { kind: "step_started", data: { step: "verify" } },
    { kind: "run_finished", data: { outcome: "success" } },
  ]).map((event, index) => ({ ...event, seq: [5, 999, 999, 2][index] }));
  const log = inspectEventLog(records.map((event) => JSON.stringify(event)).join("\n"));
  const run = operationRun(observeRun("run-test", log), STATE, NOW);
  assert.deepEqual([log.error, log.warning, log.events.map((event) => event.seq), run.status, run.target?.mode],
    [null, null, [5, 999, 999, 2], "completed", "replay"]);
});

test("listing preserves corrupt, missing and empty logs instead of dropping their runs", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-operations-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  for (const id of ["valid", "corrupt", "missing", "empty"]) await mkdir(join(dir, id));
  await writeFile(join(dir, "valid", "events.jsonl"), `${JSON.stringify(events("valid")[0])}\n`);
  await writeFile(join(dir, "corrupt", "events.jsonl"), "{not json}\n");
  await writeFile(join(dir, "empty", "events.jsonl"), "");
  const listing = await readRunListing(dir, NOW);
  assert.deepEqual([listing.observation.state, listing.runs.map((run) => [run.run_id, run.evidence.state, run.live])],
    ["ready", [["corrupt", "invalid", false], ["empty", "invalid", false], ["missing", "invalid", false], ["valid", "readable", true]]]);
});

test("directory failure and missing directory remain distinct, including identity mismatch", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-operations-path-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await mkdir(join(dir, "wrong"));
  await writeFile(join(dir, "wrong", "events.jsonl"), `${JSON.stringify(events()[0])}\n`);
  const missing = await readRunListing(join(dir, "absent"), NOW);
  const unreadable = await readRunListing(join(dir, "wrong", "events.jsonl"), NOW);
  assert.deepEqual([missing.observation.state, unreadable.observation.state, (await readRunLog(dir, "wrong")).error],
    ["missing", "error", "Recorded run identity does not match its directory."]);
});

test("navigation requires exact run attribution and keeps controls out of configuration routes", () => {
  const run = observed();
  const target = { view: "pipeline", pipeline: "repair", mode: "live", run_id: run.run_id };
  assert.deepEqual(navigationTarget(target, STATE, [run]), target);
  for (const input of [{ ...target, pipeline: "elsewhere" }, { ...target, run_id: "missing" }, { ...target, mode: "design" },
    { view: "operations", run_id: run.run_id }, { view: "config", assignment: "missing" }, { view: "cloud" },
    { view: "config", assignment: false }, { ...target, run_id: "" }, { ...target, mode: null }, { ...target, run_id: "../path" }]) {
    assert.throws(() => navigationTarget(input, STATE, [run]));
  }
});

test("authoring provenance is read-only, config-scoped, and never guesses a clean commit", async () => {
  const calls = [];
  const known = await authoringProvenance(".bureau", { git: async (dir, args) => {
    calls.push([dir, args]);
    return args[0] === "rev-parse" ? "authoring-commit\n" : " M .bureau/assignments/work.yaml\0?? .bureau/roles/new.yaml\0";
  } });
  const unknown = await authoringProvenance(".bureau", { git: async () => { throw new Error("not a repository"); } });
  assert.deepEqual([known.commit, known.changes.length, unknown.state, unknown.changes, calls[1][1].slice(-2)], ["authoring-commit", 2, "unavailable", null, ["--", "."]]);
});

test("renames and spaces in Git porcelain count once without treating old names as changes", () => {
  assert.deepEqual(changedPaths("R  .bureau/new name.yaml\0.bureau/old name.yaml\0?? .bureau/new.yaml\0"),
    [".bureau/new name.yaml", ".bureau/new.yaml"]);
});

test("oversized logs and event arrays are visibly limited, not allocated as unlimited history", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-operations-bounded-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await mkdir(join(dir, "large"));
  const first = `${JSON.stringify(events("large")[0])}\n`;
  await writeFile(join(dir, "large", "events.jsonl"), first.padEnd(MAX_RUN_LOG_BYTES, " "));
  const exactLog = await readRunLog(dir, "large");
  await writeFile(join(dir, "large", "events.jsonl"), Buffer.alloc(MAX_RUN_LOG_BYTES + 1, 32));
  const log = await readRunLog(dir, "large");
  const records = Array.from({ length: MAX_RUN_LOG_EVENTS + 1 }, (_, seq) => JSON.stringify({
    seq, at_ms: NOW, kind: "output", data: { stream: "run", data: "" },
  })).join("\n");
  const exactEvents = inspectEventLog(records.slice(0, records.lastIndexOf("\n")));
  assert.deepEqual([exactLog.error, exactLog.byte_length, exactEvents.error, exactEvents.events.length],
    [null, MAX_RUN_LOG_BYTES, null, MAX_RUN_LOG_EVENTS]);
  assert.deepEqual([log.limited, log.events, inspectEventLog(records).limited,
    operationRun(observeRun("large", log), STATE, NOW).label], [true, [], true, "Evidence unavailable"]);
});

test("directory enumeration stops at its exact cap and incomplete inventory has no all-clear counts", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-operations-inventory-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  for (let index = 0; index < MAX_RUN_DIRECTORIES; index += 1) await mkdir(join(dir, `run-${index}`));
  assert.equal((await readRunListing(dir, NOW)).observation.state, "ready");
  await mkdir(join(dir, "one-too-many"));
  const listing = await readRunListing(dir, NOW);
  assert.deepEqual([listing.runs.length, listing.observation.state, operationsOverview(STATE, listing, NOW).counts],
    [MAX_RUN_DIRECTORIES, "limited", null]);
});

test("aggregate byte budget bounds retained previews without claiming a complete inventory", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-operations-total-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const capacity = MAX_LISTING_BYTES / MAX_RUN_LOG_BYTES;
  for (let index = 0; index <= capacity; index += 1) {
    const id = `run-${index}`;
    await mkdir(join(dir, id));
    await writeFile(join(dir, id, "events.jsonl"), `${JSON.stringify(events(id)[0])}\n`.padEnd(MAX_RUN_LOG_BYTES, " "));
  }
  const listing = await readRunListing(dir, NOW);
  assert.deepEqual([listing.runs.length, listing.observation.state, operationsOverview(STATE, listing, NOW).counts],
    [capacity, "limited", null]);
});

test("real Git provenance observes only this authoring directory and never fetches", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "bureau-operations-git-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const exec = promisify(execFile);
  const git = (args) => exec("git", ["-c", "core.hooksPath=", "-c", "commit.gpgsign=false", ...args], { cwd: dir, windowsHide: true });
  const config = join(dir, ".bureau");
  await mkdir(config);
  await writeFile(join(config, "source.yaml"), "name: original\n");
  await git(["init", "--quiet"]);
  await git(["add", "."]);
  await git(["-c", "user.name=Bureau test", "-c", "user.email=fixture@example.invalid", "commit", "--quiet",
    "-m", "test: provenance fixture\n\nCo-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>"]);
  await writeFile(join(config, "source.yaml"), "name: edited\n");
  await writeFile(join(config, "untracked.yaml"), "name: untracked\n");
  await writeFile(join(dir, "unrelated.txt"), "outside the authoring directory\n");
  const provenance = await authoringProvenance(config);
  assert.deepEqual([provenance.state, provenance.commit, provenance.changes.sort()],
    ["known", (await git(["rev-parse", "HEAD"])).stdout.trim(), [".bureau/source.yaml", ".bureau/untracked.yaml"]]);
});
