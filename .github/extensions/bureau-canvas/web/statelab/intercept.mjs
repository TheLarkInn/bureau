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
 * which holds `./intent` whether or not a state declared an intercept, and now
 * also the one definition of "this request writes" that the save intercepts
 * read.
 *
 * Wider than `READ_INTENTS` by exactly one entry, and the difference is not a
 * drift: the delete preflight belongs on this list and not that one, because
 * `lib/crud.mjs` `remove()` answers an unconfirmed delete with the referrer
 * report and writes nothing. It is a read that only looks like a write, which
 * is what `a-preflight-answers-with-the-hosts-own-config` is a `harness` rule
 * about.
 *
 * There used to be a second, narrower predicate beside this one, reading
 * `READ_INTENTS` directly, and the gap between them was exactly that entry. It
 * meant a `stall-intent` held the preflight as well as the removal — so the
 * screen the confirmation is reached *through* never answered, and neither end
 * of a delete could be rendered. One definition holds both ends now: the
 * unconfirmed delete is answered, the confirmed one is held.
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
 * The run whose log has already reached its terminal.
 *
 * Named here rather than in `paths.mjs` because both hosts install the same
 * condition from this module, and the condition is "this run id is offered as
 * live" — which is a fact about the route, not about a path.
 */
export const ENDED_RUN = "run-finished";

/**
 * The kinds an in-frame shim can serve.
 *
 * `fetch` and `EventSource` are ordinary window properties, so a same-origin
 * frame can replace them before the page's deferred module scripts run. A
 * `<script type="module">` is not fetched through `window.fetch`, though, so
 * blocking a renderer is the one condition that has to stay with the suite —
 * which is why it is absent here rather than silently failing.
 */
export const IN_FRAME = new Set(["stall-state", "stall-intent", "fail-intent", "pass-intent", "abort-intent", "offer-ended-run", "empty-runs", "stall-runs", "fail-runs", "fail-runs-later"]);

/** Whether the lab can produce this state itself. */
export function servableInFrame(kind) {
  return !kind || IN_FRAME.has(kind);
}

/** Whether the condition applies before the page has a surface at all. */
export function isPreSurface(kind) {
  return kind === "stall-state" || kind === "block-renderer" || kind === "block-editor-renderer";
}

/**
 * Installs the write floor and then `kind` into a frame window whose document
 * is still parsing.
 *
 * Deliberately total: it either patches the window or does nothing, and never
 * reports a condition it did not apply. `servableInFrame` is the guard — a kind
 * this module cannot serve gets no floor either, because the lab refuses to
 * render that state at all.
 *
 * Order is load-bearing. The floor goes on first so the kind's shim wraps it
 * and is offered every request first: a `stall-intent` still hangs its own
 * writes, and only what it declines to claim — the reads — falls through to the
 * floor and on to the host. Installed the other way round, the floor would
 * reject a write before the state that exists to stall it ever saw it.
 */
export function installIntercept(win, kind) {
  if (!servableInFrame(kind)) {
    return;
  }
  installFloor(win);
  if (kind === "stall-state") {
    stallState(win);
    return;
  }
  if (kind === "offer-ended-run") {
    offerEndedRun(win);
    return;
  }
  if (kind === "empty-runs" || kind === "stall-runs" || kind === "fail-runs" || kind === "fail-runs-later") {
    interceptRuns(win, kind);
    return;
  }
  if (kind) {
    stallIntent(win, kind);
  }
}

/**
 * The live listing a moment after the watched run reached its terminal.
 *
 * A run is live exactly while its log holds no `run_finished` event, so a
 * committed log is one or the other and the live picker can never offer this
 * one. What a reader does is pick a run *while* it is running and stay on the
 * screen; the listing is what still names it, and the log is what has ended.
 * Reporting the ended run as live is that instant, and it is the only way a
 * static log can produce a screen whose subject is a run ending under the
 * reader.
 *
 * A projection of the host's own answer rather than a replacement for it: the
 * other runs are passed through exactly as served.
 */
export function offeredAsLive(payload) {
  return { ...payload, runs: (payload?.runs ?? []).map((run) => (run.run_id === ENDED_RUN ? { ...run, live: true } : run)) };
}

function offerEndedRun(win) {
  const native = win.fetch.bind(win);
  win.fetch = async (input, init) => {
    const response = await native(input, init);
    if (!/\/runs$/u.test(urlOf(input)) || !response.ok) {
      return response;
    }

    return new win.Response(JSON.stringify(offeredAsLive(await response.json())), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  };
}

/**
 * Stages the run-listing states that cannot come from the shared fixture.
 *
 * `fail-runs-later` serves the real listing once and refuses every read after
 * it. That ordering is the whole point: a run can only be *selected* from a
 * listing that answered, so the screen where a reader is watching a run while
 * the listing has since failed is unreachable if the very first read fails.
 * It is also the screen where the two halves of this surface can contradict
 * each other, which is why it has to be stageable at all.
 *
 * `stall-runs` is the read that has not come back yet — the badge before it has
 * a number. Every other run state describes an answer; this is the only one that
 * describes the wait, and it is the state `liveCountLoading` was declared for
 * and never rendered in.
 */
function interceptRuns(win, kind) {
  const native = win.fetch.bind(win);
  let served = 0;
  win.fetch = async (input, init) => {
    if (!/\/runs$/u.test(urlOf(input))) {
      return native(input, init);
    }
    served += 1;
    if (kind === "fail-runs-later" && served === 1) {
      return native(input, init);
    }
    if (kind === "stall-runs") {
      return forever();
    }
    const failing = kind === "fail-runs" || kind === "fail-runs-later";
    return new win.Response(
      JSON.stringify(failing ? { error: "run listing unavailable" } : { runs: [] }),
      { status: failing ? 503 : 200, headers: { "Content-Type": "application/json" } },
    );
  };
}

/**
 * The lab's write floor: `./intent` is refused unless it writes nothing.
 *
 * The same guarantee `matrix-fixtures.mjs` gives CI, on the surface where it
 * matters most — the lab is the one host pointed at a contributor's *own*
 * `.bureau/`, and a state that clicks a Save it never modelled would rewrite it
 * for real. Every state gets one, including the ones that asked for no
 * condition, so "the lab does not act on the host" is a property of the lab
 * rather than of which paths happen to click what.
 *
 * A rejection rather than a stall, because that is what the page must survive:
 * `postIntent` sees a `fetch` that failed, which is the branch a `TypeError`
 * from an offline host would take.
 */
export function installFloor(win) {
  const native = win.fetch.bind(win);
  win.fetch = (input, init) => {
    if (!/\/intent$/u.test(urlOf(input)) || reachesHost(bodyOf(init))) {
      return native(input, init);
    }
    return Promise.reject(new TypeError(`the state lab refused an unmodelled write to ./intent (${kindOf(init) ?? "unreadable body"})`));
  };
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

/** The four ends of a write: held, refused, dropped, or answered. */
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
    // `pass-intent` is the only one that answers `ok`. It exists for the
    // reconcile pass, whose *success* is a screen — three distinct sentences
    // about what the pass did — that no state rendered, leaving
    // `reconcileResult` a declared selector with no reference anywhere. The
    // pass writes nothing here: the answer is synthesised in-frame, and the
    // listing it then re-reads is the host's own unchanged one, which is what
    // makes "it claimed no work" the deterministic sentence.
    const answer = kind === "pass-intent" ? { ok: true, output: "no eligible work" } : refusalFor(kindOf(init));
    return Promise.resolve(new win.Response(JSON.stringify(answer), {
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
  return bodyOf(init)?.kind ?? null;
}

function bodyOf(init) {
  try {
    return JSON.parse(init?.body ?? "{}");
  } catch {
    return null;
  }
}

/**
 * Anything the floor would not let through — unparseable bodies included.
 *
 * The same predicate `reachesHost` uses, rather than a second one that agreed
 * with it about everything but the delete preflight. That disagreement had a
 * cost: a `stall-intent` held the *unconfirmed* delete too, so the preflight
 * that the confirmation is reached through never answered, and the two ends of
 * a delete could not be rendered at all. Reading the floor's own definition
 * holds exactly the writes and answers exactly the reads, which is what lets
 * `field: delete` carry a lifecycle like every other field.
 */
function isWrite(init) {
  return !reachesHost(bodyOf(init));
}
