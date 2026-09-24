import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { CheckFailure, chaosResult, siteResult } from "./maintenance-checks.mjs";
import { CATEGORIES } from "./maintenance-contract.mjs";
import {
  CHILD_DEADLINE, START_RESERVE_MS, STEP_TIMEOUT_SECONDS, checkKind, checkWaitSeconds, childDeadline,
  holdMs, lockBusy, pruneWaitSeconds, requiredStepMs, stepDeadline,
} from "./maintenance-deadline.mjs";
import { failureResult } from "./maintenance-step.mjs";

const LOCKED_STEPS = ["detect", "reproduce", "validate-patch", "full-gates"];
const BUSY = "maintenance command lock busy for 30 s; the check did not run";

async function pipelineTimeouts(category) {
  const url = new URL(`../.bureau/maintenance/pipelines/maintenance-${category}.yaml`, import.meta.url);
  const text = (await readFile(url, "utf8")).replace(/\r\n/gu, "\n");
  return Object.fromEntries(text.split(/(?=^- name: )/mu).slice(1)
    .map((block) => [/^- name: (\S+)/u.exec(block)[1], Number(/^ {2}timeout_secs: (\d+)$/mu.exec(block)?.[1])]));
}

function othersHoldMs(category) {
  return CATEGORIES.filter((other) => other !== category).reduce((total, other) => total + holdMs(checkKind(other)), 0);
}

test("step deadlines mirror every pinned pipeline and cover one hold by each other category", async () => {
  const problems = [];
  for (const category of CATEGORIES) {
    const pinned = await pipelineTimeouts(category);
    const steps = Object.keys(STEP_TIMEOUT_SECONDS[category]);
    if (steps.join() !== LOCKED_STEPS.join()) problems.push([category, steps]);
    for (const step of LOCKED_STEPS) {
      const deadline = stepDeadline(category, step);
      const waitMs = pruneWaitSeconds(deadline, checkKind(category, step === "full-gates"), START_RESERVE_MS) * 1000;
      const row = [pinned[step] * 1000 === deadline, requiredStepMs(category, step) <= deadline,
        waitMs >= othersHoldMs(category)];
      if (row.includes(false)) problems.push([category, step, ...row]);
    }
  }
  assert.deepEqual(problems, []);
});

test("lock waits leave the complete hold and exit work inside the step deadline", () => {
  const cases = [
    ["chaos", 990_000, 60_000, 480, 555], ["site", 990_000, 60_000, 630, 705],
    ["gates", 1_740_000, 60_000, 630, 705], ["chaos", 990_000, 540_500, "refused", 74],
    ["site", 990_000, 764_999, "refused", 0], ["site", 990_000, 765_001, "refused", "refused"],
  ];
  const attempt = (wait) => {
    try {
      return wait();
    } catch (error) {
      return /cannot cover the command lock hold; the check did not run/u.test(error.message) ? "refused" : error.message;
    }
  };
  const observed = cases.map(([kind, deadline, now]) => [kind, deadline, now,
    attempt(() => pruneWaitSeconds(deadline, kind, now)), attempt(() => checkWaitSeconds(deadline, kind, now))]);
  assert.deepEqual(observed, cases);
});

test("missing or unknown step deadlines refuse instead of waiting unbounded", () => {
  const messages = [() => pruneWaitSeconds(undefined, "chaos", 0), () => stepDeadline("chaos", "intake"),
    () => stepDeadline("unknown", "detect")].map((call) => {
    try {
      return call();
    } catch (error) {
      return error.message;
    }
  });
  assert.deepEqual(messages, ["maintenance lock wait requires the step deadline",
    "no maintenance step deadline for chaos:intake", "no maintenance step deadline for unknown:detect"]);
});

function drive(options, actions) {
  const events = [];
  const timers = new Map();
  const control = childDeadline(options, (reason) => events.push(reason), {
    schedule: (callback, ms) => { timers.set(timers.size + 1, callback); events.push(ms); return timers.size; },
    cancel: (id) => { if (timers.delete(id)) events.push("cancel"); },
  });
  for (const action of actions) {
    if (action === "fire") [...timers.values()].forEach((callback) => callback());
    else if (action === "acquire") control.acquire();
    else events.push(control.problem(75));
  }
  return events;
}

test("the run deadline starts at lock acquisition and exit 75 before it is lock contention", () => {
  const phased = { timeoutMs: 150_000, lockWaitMs: 30_000, locked: true };
  const cases = [
    [{ timeoutMs: 150_000, locked: false }, ["exit75", "fire"], [150_000, null, CHILD_DEADLINE]],
    [phased, ["exit75", "fire"], [35_000, BUSY, BUSY]],
    [phased, ["acquire", "acquire", "exit75", "fire"], [35_000, "cancel", 150_000, null, CHILD_DEADLINE]],
    [{ timeoutMs: 30_000, locked: true }, ["exit75", "acquire", "exit75", "fire"], [30_000, BUSY, null, CHILD_DEADLINE]],
  ];
  assert.deepEqual(cases.map(([options, actions]) => drive(options, actions)), cases.map((entry) => entry[2]));
});

test("lock contention and unacquired exits are blocked evidence, never success or no-work", async () => {
  const busy = { code: 75, signal: null, problem: lockBusy(30_000), stdout: "", stderr: "" };
  const checks = [[busy, "site"], [busy, "chaos"], [{ ...busy, problem: null }, "site"], [{ ...busy, problem: null }, "chaos"]];
  const accepted = checks.filter(([run, kind]) => {
    try {
      return Boolean(kind === "site" ? siteResult(run, "site-accessibility") : chaosResult(run, 1));
    } catch {
      return false;
    }
  });
  const request = { step: "detect", inputs: {} };
  const results = [await failureResult(new CheckFailure(BUSY, ""), request, async () => []),
    await failureResult(new Error(`Cargo target cleanup skipped: ${BUSY}`), request)];
  assert.deepEqual([accepted, results.map(({ outcome, outputs, message }) => [outcome, outputs, message])],
    [[], [["blocked", {}, BUSY], ["blocked", {}, `Cargo target cleanup skipped: ${BUSY}`]]]);
});
