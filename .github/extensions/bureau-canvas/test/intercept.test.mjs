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
  READ_INTENTS,
  reachesHost,
  refusalFor,
  servableInFrame,
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

test("every intercept the registry asks for is one this module names", () => {
  const asked = [...new Set(STATES.map((state) => state.intercept).filter(Boolean))].sort();
  const unservable = asked.filter((kind) => !servableInFrame(kind)).sort();

  assert.deepEqual(
    { asked, unservable, preSurface: asked.filter(isPreSurface).sort() },
    {
      asked: ["block-editor-renderer", "block-renderer", "fail-intent", "stall-intent", "stall-state"],
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
