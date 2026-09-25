import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { CATEGORIES, intentBlock } from "./maintenance-contract.mjs";
import { VERIFY_TIMEOUT_SECONDS, verifyDeadline } from "./maintenance-deadline.mjs";
import { verifyDraft, verifyHandoff } from "./maintenance-lifecycle.mjs";
import { RECHECK_AFTER_MS, RECHECK_RESERVE_MS, verifyObserved } from "./maintenance-observe.mjs";
import { clear, draft, handoff } from "./maintenance-publish.mjs";
import { POLICY, fakeForge, fixture } from "./maintenance-test-support.mjs";

const DEADLINE = verifyDeadline("verify-clear");
const LATER = "2026-09-17T18:00:00Z";
const AGAIN = `re-observed ${RECHECK_AFTER_MS / 1000} s after the first observation`;
const UNREPORTED = "source scan was not durably reported";
const NO_DRAFT = "expected one finding-marker issue, observed 0";

function snapshot(forge) {
  const issues = structuredClone([...forge.issues.values()]);
  const comments = structuredClone(Object.fromEntries(forge.comments));
  return { issue: (number) => issues.find((issue) => issue.number === number),
    issues: () => issues, comments: (number) => comments[number] ?? [] };
}

function fakeClock(start) {
  const clock = { time: start, waits: [] };
  clock.now = () => clock.time;
  clock.wait = async (ms) => { clock.waits.push(ms); clock.time += ms; };
  return clock;
}

// Each observation starts with the source read, so it selects the next view;
// the last view stays served. An Error view is a forge transport failure.
function served(views, clock, readMs) {
  const reads = [];
  let round = -1;
  const read = (name, ...args) => {
    reads.push(name);
    clock.time += readMs;
    const view = views[Math.min(round, views.length - 1)];
    if (view instanceof Error) throw view;
    return structuredClone(view[name](...args));
  };
  return { reads, api: { issue: async (number) => { round += 1; return read("issue", number); },
    issues: async () => read("issues"), comments: async (number) => read("comments", number) } };
}

function editIntent(forge, value, cycle) {
  const source = forge.issues.get(POLICY.source_issues[value.source.category]);
  const intent = { schema: "bureau-maintenance-intent-v1", ...value.source, cycle };
  delete intent.id;
  source.body = source.body.replace(intentBlock({ ...intent, cycle: value.source.cycle }), intentBlock(intent));
}

// Views before and after the reporter's effects, and the verifier's check.
async function flow(kind) {
  const { source, value } = fixture(kind === "clear" ? "site-responsive" : "chaos", kind === "clear" ? [] : undefined);
  const forge = fakeForge(source);
  const receipt = kind === "handoff" ? await draft(forge.api, value, POLICY) : null;
  const before = snapshot(forge);
  if (kind === "clear") await clear(forge.api, value, POLICY);
  else if (kind === "draft") await draft(forge.api, value, POLICY);
  else await handoff(forge.api, value, receipt, POLICY);
  const after = snapshot(forge);
  const verify = kind === "draft" ? (observed) => verifyDraft(observed, value, POLICY)
    : (observed) => verifyHandoff(observed, value, receipt, POLICY);
  return { forge, value, before, after, verify, category: value.source.category };
}

const tamper = {
  intent: (state) => { editIntent(state.forge, state.value, LATER); return snapshot(state.forge); },
  issuer: (state) => {
    state.forge.issues.get(POLICY.source_issues[state.category]).user.id = 18;
    return snapshot(state.forge);
  },
  duplicate: (state) => {
    state.forge.issues.set(43, { ...state.forge.issues.get(42), number: 43 });
    return snapshot(state.forge);
  },
};

function views(state, names) {
  const pick = { before: () => state.before, after: () => state.after,
    http: () => new Error("forge GET /issues/7 returned HTTP 403") };
  return names.map((name) => (pick[name] ?? (() => tamper[name](state)))());
}

async function run(kind, names, { start = 5_000, readMs = 500, deadline = DEADLINE, verify } = {}) {
  const state = await flow(kind);
  const clock = fakeClock(start);
  const { api, reads } = served(views(state, names), clock, readMs);
  let outcome = "verified";
  try {
    await verifyObserved(api, state.category, POLICY, verify ?? state.verify, { deadline, clock });
  } catch (error) {
    outcome = error.message;
  }
  return [outcome, reads.length, clock.waits];
}

test("a stale read is re-observed once and verification passes only on fresh state", async () => {
  const wait = [5_000 + RECHECK_AFTER_MS - 6_500];
  const cases = [
    ["clear", ["after"], ["verified", 3, []]],
    ["clear", ["before", "after"], ["verified", 6, wait]],
    ["draft", ["before", "after"], ["verified", 6, wait]],
    ["handoff", ["before", "after"], ["verified", 6, wait]],
    ["clear", ["before", "before"], [`${UNREPORTED}; ${AGAIN}: ${UNREPORTED}`, 6, wait]],
    ["draft", ["before", "before"], [`${NO_DRAFT}; ${AGAIN}: ${NO_DRAFT}`, 6, wait]],
    ["handoff", ["before", "before"], [`${UNREPORTED}; ${AGAIN}: ${UNREPORTED}`, 6, wait]],
    ["clear", ["before", "http"], [`${UNREPORTED}; ${AGAIN}: forge GET /issues/7 returned HTTP 403`, 4, wait]],
  ];
  const observed = [];
  for (const [kind, names] of cases) observed.push(await run(kind, names));
  assert.deepEqual(observed, cases.map((entry) => entry[2]));
});

test("transport, trust, intent, duplicate and evidence failures fail fast with no extra reads", async () => {
  const cases = [
    ["clear", ["http"], {}, ["forge GET /issues/7 returned HTTP 403", 1, []]],
    ["clear", ["intent"], {}, ["source intent changed during handoff", 3, []]],
    ["clear", ["issuer"], {}, ["forge identity differs from the reviewed numeric issuer", 3, []]],
    ["draft", ["duplicate"], {}, ["expected one finding-marker issue, observed 2", 3, []]],
    ["clear", ["after"], { verify: (observed) => verifyHandoff(observed, {}, null, POLICY) },
      ["missing or incomplete deterministic evidence", 3, []]],
    ["clear", ["before"], { start: 30_000 },
      [`${UNREPORTED}; the step deadline leaves no time to re-observe the forge`, 3, []]],
  ];
  const observed = [];
  for (const [kind, names, options] of cases) observed.push(await run(kind, names, options));
  assert.deepEqual(observed, cases.map((entry) => entry[3]));
});

async function pinnedVerifyTimeouts(category) {
  const url = new URL(`../.bureau/maintenance/pipelines/maintenance-${category}.yaml`, import.meta.url);
  const text = (await readFile(url, "utf8")).replace(/\r\n/gu, "\n");
  return Object.fromEntries(text.split(/(?=^- name: )/mu).slice(1)
    .map((block) => [/^- name: (\S+)/u.exec(block)[1], Number(/^ {2}timeout_secs: (\d+)$/mu.exec(block)?.[1])])
    .filter(([name]) => name.startsWith("verify-")));
}

test("one wait and re-observation fit inside every pinned verify step timeout", async () => {
  const problems = [];
  for (const category of CATEGORIES) {
    const pinned = await pinnedVerifyTimeouts(category);
    if (Object.keys(pinned).sort().join() !== Object.keys(VERIFY_TIMEOUT_SECONDS).sort().join()) problems.push(pinned);
    for (const [step, seconds] of Object.entries(pinned)) {
      const deadline = verifyDeadline(step);
      // The latest first observation that still earns a re-observation.
      const start = deadline - RECHECK_AFTER_MS - RECHECK_RESERVE_MS;
      const [outcome, reads, waits] = await run("clear", ["before", "after"], { start, readMs: 2_000, deadline });
      const row = [seconds * 1000 === deadline, start >= 20_000, outcome, reads, waits.length];
      if (row.join() !== [true, true, "verified", 6, 1].join()) problems.push([category, step, ...row]);
    }
  }
  assert.deepEqual(problems, []);
});
