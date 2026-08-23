// The request-level conditions a state can be under, defined once.
//
// A save state is not a click away from the state beside it — it is the same
// screen under a host that is slow, refusing, or gone. That condition lives in
// the network, so it is described here and applied two ways: the browser suite
// routes it with `page.route`, and the lab installs it inside the frame before
// the page's modules run. Both read these semantics from this module, so the
// screen a reviewer browses and the screen CI asserts cannot drift apart.

/**
 * The two intents that only *read*. `./intent` is not just how the page saves:
 * `derive-work-source` builds the paste preview and `resolve-repo` looks a
 * repository up. Holding those stalls the very derivation a save state is
 * supposed to be saving, so they are named and everything else is held.
 *
 * Everything else is held *by default*, which is the safe direction: an intent
 * added later is withheld from the shared host rather than written to it.
 */
export const READ_INTENTS = new Set(["derive-work-source", "resolve-repo"]);

/**
 * What may reach the real host at all — the floor under the browser suite,
 * which holds `./intent` whether or not a state declared an intercept.
 *
 * Wider than `READ_INTENTS` by exactly one entry, and the difference is not a
 * drift. A save intercept holds writes so that a *saving* or *refused* screen
 * can be rendered; the floor holds them so a state that declared no intercept
 * cannot write to the contributor's own `.bureau/` by omission. The delete
 * preflight belongs on the second list and not the first: `lib/crud.mjs`
 * `remove()` answers an unconfirmed delete with the referrer report and writes
 * nothing, so it is a read — and it is the one intent the matrix deliberately
 * lets through, which is what `a-preflight-answers-with-the-hosts-own-config`
 * is a `harness` rule about.
 */
export function reachesHost(body) {
  return READ_INTENTS.has(body?.kind) || isPreflight(body);
}

/** An unconfirmed delete: `remove()` reports its referrers and writes nothing. */
function isPreflight(body) {
  return body?.kind === "delete" && !body?.input?.confirm;
}

/**
 * A refusal in the shape the host actually answers with.
 *
 * `extension.mjs` writes every intent answer through `sendJson`, which is
 * hard-coded to HTTP 200 — a refusal is `{ ok: false }` in a 200 body, never a
 * 500. A refused `save-pipeline` also carries the findings that say why the
 * write was reverted; without them the panel has nothing to draw but "save
 * failed", which is the one refusal that gives no reason.
 */
export function refusalFor(kind) {
  if (kind !== "save-pipeline") {
    return { ok: false };
  }
  return {
    ok: false,
    findings: [{
      message: "step `verify` names `implement` in `on.success`, and no step in this pipeline is called `implement`.",
    }],
  };
}

/**
 * The kinds an in-frame shim can serve.
 *
 * `fetch` and `EventSource` are ordinary window properties, so a same-origin
 * frame can replace them before the page's deferred module scripts run. A
 * `<script type="module">` is not fetched through `window.fetch`, though, so
 * blocking a renderer is the one condition that has to stay with the suite —
 * which is why it is absent here rather than silently failing.
 */
export const IN_FRAME = new Set(["stall-state", "stall-intent", "fail-intent", "abort-intent"]);

/** Whether the lab can produce this state itself. */
export function servableInFrame(kind) {
  return !kind || IN_FRAME.has(kind);
}

/** Whether the condition applies before the page has a surface at all. */
export function isPreSurface(kind) {
  return kind === "stall-state" || kind === "block-renderer" || kind === "block-editor-renderer";
}

/**
 * Installs `kind` into a frame window whose document is still parsing.
 *
 * Deliberately total: it either patches the window or does nothing, and never
 * reports a condition it did not apply. `servableInFrame` is the guard.
 */
export function installIntercept(win, kind) {
  if (!servableInFrame(kind) || !kind) {
    return;
  }
  if (kind === "stall-state") {
    stallState(win);
    return;
  }
  stallIntent(win, kind);
}

/**
 * The payload never arrives, on either channel.
 *
 * Stalling the fetch alone still renders a full surface, because the SSE
 * channel writes the current state the moment it connects — so the connection
 * is refused too, by a stand-in that never delivers anything.
 */
function stallState(win) {
  const native = win.fetch.bind(win);
  win.fetch = (input, init) => (/\/(state|events)$/u.test(urlOf(input)) ? forever() : native(input, init));
  win.EventSource = class {
    constructor() {
      this.readyState = 0;
    }

    addEventListener() {}

    removeEventListener() {}

    close() {}
  };
}

/** The three ends of a save, applied to writes only. */
function stallIntent(win, kind) {
  const native = win.fetch.bind(win);
  win.fetch = (input, init) => {
    if (!/\/intent$/u.test(urlOf(input)) || !isWrite(init)) {
      return native(input, init);
    }
    if (kind === "stall-intent") {
      return forever();
    }
    if (kind === "abort-intent") {
      return Promise.reject(new TypeError("Failed to fetch"));
    }
    return Promise.resolve(new win.Response(JSON.stringify(refusalFor(kindOf(init))), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    }));
  };
}

/** A request that is never answered, which is what a hung host looks like. */
function forever() {
  return new Promise(() => {});
}

function urlOf(input) {
  return typeof input === "string" ? input : String(input?.url ?? input);
}

function kindOf(init) {
  try {
    return JSON.parse(init?.body ?? "{}").kind ?? null;
  } catch {
    return null;
  }
}

/** Anything other than one of the two known reads, unparseable bodies included. */
function isWrite(init) {
  const kind = kindOf(init);
  return kind === null || !READ_INTENTS.has(kind);
}
