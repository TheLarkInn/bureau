// The interception semantics, checked without a browser.
//
// These decide what a saving, refused or stalled screen actually is. The lab
// installs them inside its frame and the browser suite routes the same
// conditions, so if this module drifts the two surfaces disagree about what a
// state means — and the screen a reviewer approves stops being the screen CI
// asserts.
//
// Offline by construction: pure module, a stub window, no network.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  installIntercept,
  IN_FRAME,
  isPreSurface,
  PASS_RUN,
  READ_INTENTS,
  reachesHost,
  refusalFor,
  servableInFrame,
  withoutPassRun,
  withPassRun,
} from "../web/statelab/intercept.mjs";
import { STATES } from "../web/statelab/registry.mjs";

/** A window stub with just the surface `installIntercept` replaces. */
function windowStub() {
  const calls = [];
  return {
    calls,
    Response: globalThis.Response,
    EventSource: class Native {},
    fetch(input, init) {
      calls.push({ url: typeof input === "string" ? input : input?.url, init });
      return Promise.resolve({ ok: true, native: true });
    },
  };
}

const RESOLVED = Symbol("resolved");

/** Whether a promise settles at all, without hanging the test on one that never does. */
async function settles(promise) {
  const pending = Symbol("pending");
  const result = await Promise.race([
    promise.then(() => RESOLVED, () => RESOLVED),
    new Promise((resolve) => setTimeout(() => resolve(pending), 25)),
  ]);
  return result === RESOLVED;
}

const post = (kind) => ["./intent", { method: "POST", body: JSON.stringify({ kind }) }];

const del = (confirm) => ["./intent", { method: "POST", body: JSON.stringify({ kind: "delete", input: { name: "reviewer", confirm } }) }];

/**
 * The preflight refusal is scoped to the preflight, and that scoping is the
 * whole claim.
 *
 * A shim that refused every `delete` would stage two screens at once — the read
 * that could not answer, and a removal that was declined — and the probe would
 * be asserting whichever the page happened to draw first. So all three answers
 * are read in one pass, and by *outcome* rather than by whether they settle:
 * the unconfirmed delete is answered in-frame with a refusal, the confirmed one
 * is left to the floor (which rejects it, because it writes), and an unrelated
 * read still reaches the host.
 */
test("a refused preflight refuses the read and leaves every other intent alone", async () => {
  const win = windowStub();
  installIntercept(win, "refuse-preflight");

  const answered = async (promise) => {
    const value = await promise.catch(() => null);
    return value === null ? "rejected" : (value.native ? "reached the host" : await value.json());
  };

  assert.deepEqual(
    {
      preflight: await answered(win.fetch(...del(false))),
      confirmed: await answered(win.fetch(...del(true))),
      read: await answered(win.fetch(...post("resolve-repo"))),
    },
    {
      // `postIntent` turns a non-`ok` answer into `null`, which is the branch
      // that renders `DeleteControl`'s own "could not inspect" sentence.
      preflight: { ok: false },
      // Refused by the floor and not by this shim: a confirmed delete writes,
      // so it may not reach the host and may not be answered `ok` either.
      confirmed: "rejected",
      read: "reached the host",
    },
  );
});

/**
 * The other end of the same scoping: held rather than refused.
 *
 * Read by whether each request settles, because that is the difference this
 * condition makes. `DeleteControl` clears its `busy` flag in `.then`, so a
 * preflight that never settles is the "Checking…" screen and nothing else can
 * produce it — and the confirmed delete must still be refused by the floor
 * rather than quietly held here, or the state would be two screens at once.
 */
test("a held preflight hangs the read and leaves every other intent alone", async () => {
  const win = windowStub();
  installIntercept(win, "stall-preflight");

  assert.deepEqual(
    {
      preflight: await settles(win.fetch(...del(false))),
      confirmed: await settles(win.fetch(...del(true))),
      read: await settles(win.fetch(...post("resolve-repo"))),
    },
    { preflight: false, confirmed: true, read: true },
  );
});

test("every intercept the registry asks for is one this module names", () => {
  const asked = [...new Set(STATES.map((state) => state.intercept).filter(Boolean))].sort();
  const unservable = asked.filter((kind) => !servableInFrame(kind)).sort();

  assert.deepEqual(
    { asked, unservable, preSurface: asked.filter(isPreSurface).sort() },
    {
      // `abort-intent` is asked for by probes alone. It was missing from this
      // list — and so unchecked against this module — for as long as a probe
      // carried its route on the page op and not on the state.
      asked: ["abort-intent", "block-editor-renderer", "block-renderer", "empty-runs", "fail-intent", "fail-runs", "fail-runs-later", "offer-ended-run", "pass-intent", "pass-starts-run", "refuse-preflight", "stall-intent", "stall-preflight", "stall-runs", "stall-state"],
      // The only condition an in-frame shim cannot stage: a module script is
      // not fetched through `window.fetch`.
      unservable: ["block-editor-renderer", "block-renderer"],
      preSurface: ["block-editor-renderer", "block-renderer", "stall-state"],
    },
  );
});

test("a held save holds only writes, and lets the two reads through", async () => {
  const win = windowStub();
  installIntercept(win, "stall-intent");

  const outcomes = {
    write: await settles(win.fetch(...post("set-limits"))),
    unparseable: await settles(win.fetch("./intent", { method: "POST", body: "{" })),
    read: await settles(win.fetch(...post("resolve-repo"))),
    otherUrl: await settles(win.fetch("./state")),
    readsAreKnown: [...READ_INTENTS].sort(),
  };

  assert.deepEqual(outcomes, {
    write: false,
    // An intent whose body cannot be read is held rather than sent, so an
    // intent added later cannot reach the contributor's own `.bureau/`.
    unparseable: false,
    read: true,
    otherUrl: true,
    readsAreKnown: ["derive-work-source", "resolve-repo"],
  });
});

/**
 * The pass that started something, as the two projections its report depends
 * on. Both halves have to hold or the sentence is not the one that names a run.
 *
 * `reconcileNow` snapshots the run ids it can already see before it posts, and
 * `newRunSince` attributes only a run absent from that snapshot *and* stamped
 * after the click. So withholding is not decoration: over an unchanged listing
 * the run is already in `known`, the pass reports "claimed no work", and **Open
 * in Replay** is never drawn. The second half is a projection rather than a
 * replacement — the run keeps its own summary, so the hand-off lands on a run
 * the host really holds a log for, and only `started_at` moves.
 */
test("a pass that starts a run withholds it first, then returns it stamped after the click", () => {
  const listing = { runs: [{ run_id: "other", started_at: "2026-01-01T00:00:00.000Z", live: true }, { run_id: PASS_RUN, assignment: "agent-eligible", started_at: "2026-01-01T00:00:00.000Z", live: true }] };
  const clicked = Date.parse("2026-08-24T10:00:00.000Z");
  const after = withPassRun(listing, clicked).runs.find((run) => run.run_id === PASS_RUN);

  assert.deepEqual(
    {
      before: withoutPassRun(listing).runs.map((run) => run.run_id),
      after: withPassRun(listing, clicked).runs.map((run) => run.run_id),
      stamped: Date.parse(after.started_at) >= clicked,
      // Finished, because `reconcile --now` drains before it returns — which is
      // why the hand-off is to Replay and not to the live-only picker.
      settled: [after.live, after.assignment],
      // A listing that does not carry the run is left exactly as served: the
      // shim reports no run rather than inventing one Replay cannot open.
      absent: withPassRun({ runs: [{ run_id: "other" }] }, clicked).runs.map((run) => run.run_id),
    },
    {
      before: ["other"],
      after: ["other", PASS_RUN],
      stamped: true,
      settled: [false, "agent-eligible"],
      absent: ["other"],
    },
  );
});

/**
 * The floor's allowance, which is wider than a save intercept's by one entry.
 *
 * `matrix-fixtures.mjs` denies `./intent` under *every* state now, so this
 * predicate is the only thing standing between a path nobody modelled and the
 * contributor's own config. The delete preflight has to be on it — `remove()`
 * answers an unconfirmed delete with its referrer report and writes nothing,
 * and the matrix reaches the host for exactly that one screen — and the
 * confirmed delete beside it must not be, which is the pair worth pinning:
 * they differ only by a nested `confirm`, so a predicate that read the `kind`
 * alone would wave a real deletion through.
 */
test("the floor lets a read and the delete preflight through, and nothing else", () => {
  const verdicts = [
    ["derive-work-source", { kind: "derive-work-source", url: "https://example.com" }],
    ["resolve-repo", { kind: "resolve-repo", url: "https://example.com" }],
    ["delete preflight", { kind: "delete", input: { kind: "role", name: "implementer" } }],
    ["delete confirmed", { kind: "delete", input: { kind: "role", name: "implementer", confirm: true } }],
    ["save-plan", { kind: "save-plan" }],
    ["save-pipeline", { kind: "save-pipeline" }],
    ["discard-plan", { kind: "discard-plan" }],
    ["create", { kind: "create", input: { kind: "role", name: "new" } }],
    ["set-limits", { kind: "set-limits" }],
    ["cancel-run", { kind: "cancel-run" }],
    ["an unparseable body", null],
    ["an intent added later", { kind: "some-intent-invented-after-this-test" }],
  ].map(([name, body]) => `${name}: ${reachesHost(body) ? "reaches the host" : "held"}`);

  assert.deepEqual(verdicts, [
    "derive-work-source: reaches the host",
    "resolve-repo: reaches the host",
    "delete preflight: reaches the host",
    "delete confirmed: held",
    "save-plan: held",
    "save-pipeline: held",
    "discard-plan: held",
    "create: held",
    "set-limits: held",
    "cancel-run: held",
    "an unparseable body: held",
    "an intent added later: held",
  ]);
});

test("a refused save answers 200 with the body the host would send", async () => {
  const win = windowStub();
  installIntercept(win, "fail-intent");

  const pipeline = await (await win.fetch(...post("save-pipeline"))).json();
  const plan = await (await win.fetch(...post("save-plan"))).json();
  const status = (await win.fetch(...post("save-plan"))).status;

  assert.deepEqual(
    { status, plan, pipelineOk: pipeline.ok, pipelineSaysWhy: pipeline.findings.length },
    // A refusal is `{ ok: false }` in a 200 body, never a 500 — and the one
    // state whose whole subject is *why* a save was refused carries findings.
    { status: 200, plan: { ok: false }, pipelineOk: false, pipelineSaysWhy: 1 },
  );
});

test("an aborted save rejects rather than answering, which is the other end", async () => {
  const win = windowStub();
  installIntercept(win, "abort-intent");

  const rejected = await win.fetch(...post("set-limits")).then(() => false, () => true);
  const readPassed = (await win.fetch(...post("derive-work-source"))).native;

  assert.deepEqual({ rejected, readPassed, known: IN_FRAME.has("abort-intent") }, { rejected: true, readPassed: true, known: true });
});

test("a held payload closes both channels, not just the fetch", async () => {
  const win = windowStub();
  const Native = win.EventSource;
  installIntercept(win, "stall-state");
  const source = new win.EventSource("./events");
  let delivered = false;
  source.addEventListener("state", () => {
    delivered = true;
  });

  assert.deepEqual(
    {
      state: await settles(win.fetch("./state")),
      events: await settles(win.fetch("./events")),
      other: await settles(win.fetch("./intent", { method: "POST", body: JSON.stringify({ kind: "set-limits" }) })),
      replaced: win.EventSource !== Native,
      delivered,
    },
    // Stalling the fetch alone still renders a full surface, because the SSE
    // channel writes the current state the moment it connects.
    { state: false, events: false, other: true, replaced: true, delivered: false },
  );
});

test("a stalled write hangs on its own shim rather than on the floor beneath it", async () => {
  const win = windowStub();
  installIntercept(win, "stall-intent");

  assert.deepEqual(
    {
      write: await settles(win.fetch(...post("save-plan"))),
      read: (await win.fetch(...post("resolve-repo"))).native,
    },
    // Order is the claim: the floor is installed first so the kind's shim
    // wraps it and is asked first. A write therefore hangs — which is the
    // "Saving…" screen — instead of being rejected by the floor, and what the
    // shim declines to claim falls through both and reaches the host.
    //
    // The shim is the stricter of the two: `isWrite` holds the delete
    // preflight that the floor's `reachesHost` lets by. Nothing crosses the
    // two, so the difference costs no screen — and the safe direction for a
    // condition that exists to hold saves is to hold more, not less.
    { write: false, read: true },
  );
});

test("the ended run is offered as live, and no other listing is touched", async () => {
  const listing = { runs: [{ run_id: "run-live", live: true }, { run_id: "run-finished", live: false }, { run_id: "run-paused", live: true }] };
  const win = windowStub();
  win.fetch = (input) => Promise.resolve(new Response(JSON.stringify(listing), { status: 200, headers: { "Content-Type": "application/json" }, url: String(input) }));
  installIntercept(win, "offer-ended-run");
  const served = await (await win.fetch("./runs")).json();
  const untouched = await (await win.fetch("./state")).json();

  assert.deepEqual(
    {
      // The whole listing is passed through; exactly one entry is relabelled,
      // which is the instant after the watched run reached its terminal.
      served: served.runs.map((run) => [run.run_id, run.live]),
      // A projection, not a replacement: any other request is the host's own.
      untouched: untouched.runs.map((run) => [run.run_id, run.live]),
      pureOfInput: listing.runs.find((run) => run.run_id === "run-finished").live,
      known: IN_FRAME.has("offer-ended-run"),
      // It routes `./runs`, so it is not a pre-surface condition: the page
      // boots and settles normally and only the picker's poll is answered.
      preSurface: isPreSurface("offer-ended-run"),
    },
    {
      served: [["run-live", true], ["run-finished", true], ["run-paused", true]],
      untouched: [["run-live", true], ["run-finished", false], ["run-paused", true]],
      pureOfInput: false,
      known: true,
      preSurface: false,
    },
  );
});

test("empty and unavailable run listings are different host conditions", async () => {
  const results = [];
  for (const kind of ["empty-runs", "fail-runs"]) {
    const win = windowStub();
    installIntercept(win, kind);
    const response = await win.fetch("./runs");
    results.push([kind, response.status, await response.json(), IN_FRAME.has(kind)]);
  }

  assert.deepEqual(results, [
    ["empty-runs", 200, { runs: [] }, true],
    ["fail-runs", 503, { error: "run listing unavailable" }, true],
  ]);
});

test("a state that asked for no condition still sits on the write floor", async () => {
  const win = windowStub();
  const native = win.fetch;
  installIntercept(win, null);
  const floored = win.fetch;
  installIntercept(win, "block-renderer");

  assert.deepEqual(
    {
      floorInstalled: win.fetch !== native,
      // `block-renderer` is not servable here, so it must be a no-op rather
      // than a condition this module claims to have applied — and it adds no
      // floor either, because the lab refuses to render that state at all.
      unservableIsANoOp: win.fetch === floored,
      write: await win.fetch(...post("save-plan")).then(() => "sent", () => "refused"),
      read: await win.fetch(...post("derive-work-source")).then((response) => (response.native ? "sent" : "refused")),
      otherUrl: (await win.fetch("./state")).native,
      refusalIsPlain: refusalFor("set-limits"),
    },
    {
      floorInstalled: true,
      unservableIsANoOp: true,
      write: "refused",
      read: "sent",
      otherUrl: true,
      refusalIsPlain: { ok: false },
    },
  );
});

/*
 * The two hosts implement the same vocabulary.
 *
 * A condition is defined once here and applied twice — the lab installs it in
 * its frame, the browser suite routes it with `page.route` — and nothing held
 * the two lists to each other. A kind added to one and not the other is not a
 * loud failure: the lab would render the state and the suite would throw
 * `routes[kind] is not a function`, or worse, the suite would route a kind the
 * lab silently declines to serve and the screen a reviewer approved would stop
 * being the screen CI asserts. That is the exact drift this module's header
 * says it exists to prevent, and it was unasserted.
 *
 * `matrix-fixtures.mjs` imports `@playwright/test`, so it cannot be imported
 * from an offline test. Its route table is read out of the source instead,
 * which is enough: the keys are literals, and a missing one is the whole bug.
 *
 * The suite's table is `IN_FRAME` plus exactly the two conditions an in-frame
 * shim provably cannot stage — a `<script type="module">` is not fetched
 * through `window.fetch` — so the relationship is an equality, not a subset.
 */
const SUITE_ONLY = ["block-editor-renderer", "block-renderer"];

test("the lab and the browser suite serve the same set of conditions", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(new URL("../e2e/playwright/matrix-fixtures.mjs", import.meta.url), "utf8");
  const table = source.slice(source.indexOf("const routes = {"));
  const routed = [...table.matchAll(/^\s{4}"([a-z-]+)":/gmu)].map((match) => match[1]).sort();

  assert.deepEqual(routed, [...IN_FRAME, ...SUITE_ONLY].sort());
});
