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
export function isPreflight(body) {
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
 * The run a staged reconcile pass is made to have started.
 *
 * The same committed log `ENDED_RUN` names, in a different role: there it is a
 * finished run reported as live, here it is a finished run the listing did not
 * carry until the pass answered. Reusing it is deliberate — a hand-off to
 * Replay has to land on a run whose log really exists, or the button would be
 * asserted against a screen that cannot draw.
 */
export const PASS_RUN = "run-finished";

/**
 * The listing before the pass: the host's own, minus the run it is about to
 * start.
 *
 * Withholding is what makes the report exact rather than lucky. `reconcileNow`
 * snapshots the run ids it can already see and only attributes a run that is
 * *absent* from that snapshot and started after the click, so a pass staged
 * over an unchanged listing can only ever report "claimed no work".
 */
export function withoutPassRun(payload) {
  return { ...payload, runs: (payload?.runs ?? []).filter((run) => run.run_id !== PASS_RUN) };
}

/**
 * The listing after it, with that run back and stamped after the click.
 *
 * A projection of the host's own answer, like `offeredAsLive`: the run keeps
 * its own summary and only `started_at` moves, so the run the button hands to
 * Replay is one the host really holds a log for. A listing that does not carry
 * it is left exactly as served — the shim reports no run rather than inventing
 * one Replay could not open. `atMs` is when the pass answered, necessarily
 * after the click that issued it, which is the bound `newRunSince` applies so
 * that a background reconciler's run cannot be reported as this click's doing.
 */
export function withPassRun(payload, atMs) {
  const started = (payload?.runs ?? []).find((run) => run.run_id === PASS_RUN);
  if (!started) {
    return payload;
  }
  return {
    ...payload,
    runs: [...withoutPassRun(payload).runs, { ...started, live: false, started_at: new Date(atMs).toISOString() }],
  };
}

/** The answer a pass that started one run gives. */
export const PASS_STARTED = { ok: true, output: "started 1 run" };

/**
 * The host's blocking answer to a delete preflight.
 *
 * `lib/crud.mjs` `remove()` answers an unconfirmed delete with
 * `{action, kind, name, confirmed: false, referrers, blocking}`, and `blocking`
 * is `blocksDelete(found)` — true the moment anything still points at the
 * entity. `DeleteControl` draws that answer as a screen the registry declared
 * and nothing ever rendered: the count, the referrer list, the sentence saying
 * to repoint them, and a Confirm that is withheld rather than merely
 * discouraged.
 *
 * The referrers are not invented. They are the host's own report for deleting
 * the role `implementer` out of the committed sample, and `test/preflight.test.mjs`
 * pins this payload against `referrers()` run over that payload — so a change to
 * what `lib/preflight.mjs` reports fails there rather than leaving a probe
 * rendering a shape the host stopped producing.
 *
 * What the harness supplies is the *mount point*, and only that. The two places
 * `DeleteControl` mounts are an assignment card and the orphan strip, and both
 * are entities nothing refers to — which is exactly what
 * `delete-is-offered-only-where-nothing-refers` says, and why this answer has to
 * be staged rather than clicked to.
 */
export const BLOCKED_PREFLIGHT = {
  ok: true,
  result: {
    action: "delete",
    kind: "role",
    name: "implementer",
    confirmed: false,
    referrers: [
      { source: "preflight", severity: "referrer", kind: "assignment", name: "agent-eligible", message: "assignment `agent-eligible` runs role `implementer`" },
      { source: "preflight", severity: "referrer", kind: "step", name: "agent-eligible-pipeline/implement", message: "step `implement` in `agent-eligible-pipeline` runs role `implementer`" },
    ],
    blocking: true,
  },
};

/**
 * The kinds an in-frame shim can serve.
 *
 * `fetch` and `EventSource` are ordinary window properties, so a same-origin
 * frame can replace them before the page's deferred module scripts run. A
 * `<script type="module">` is not fetched through `window.fetch`, though, so
 * blocking a renderer is the one condition that has to stay with the suite —
 * which is why it is absent here rather than silently failing.
 */
export const IN_FRAME = new Set(["stall-state", "stall-intent", "fail-intent", "refuse-preflight", "stall-preflight", "block-preflight", "pass-intent", "pass-starts-run", "abort-intent", "offer-ended-run", "empty-runs", "stall-runs", "fail-runs", "fail-runs-later"]);

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
  if (kind === "pass-starts-run") {
    passStartsRun(win);
    return;
  }
  if (kind === "refuse-preflight" || kind === "stall-preflight" || kind === "block-preflight") {
    interceptPreflight(win, kind);
    return;
  }
  if (kind) {
    stallIntent(win, kind);
  }
}

/**
 * The delete preflight: held, refused, or answered with referrers.
 *
 * Every other condition here goes through `stallIntent`, which only ever claims
 * a request `isWrite` says is one — and the unconfirmed delete deliberately is
 * not, because `reachesHost` lets it through so the confirmation prompt can
 * draw the referrers the host reports. That made these the conditions no other
 * shim could stage, and so three of `DeleteControl`'s own screens that no state
 * ever rendered: the refusal, the round trip before it, and the answer that
 * comes back blocking.
 *
 * The round trip matters on its own. Pressing Delete does not open a prompt, it
 * asks the host a question, and while that question is outstanding the button
 * says "Checking…" and stops accepting presses — which is the whole of what
 * stops a reader queueing three preflights against one card. It is the same
 * two-ended contract the confirmation carries, on the request that comes first.
 *
 * Only the preflight is claimed. Everything else — including the confirmed
 * delete — falls through to the floor, so this stages one slow, failed or
 * blocking read rather than a host that has stopped answering.
 */
function interceptPreflight(win, kind) {
  const native = win.fetch.bind(win);
  win.fetch = (input, init) => {
    if (!/\/intent$/u.test(urlOf(input)) || !isPreflight(bodyOf(init))) {
      return native(input, init);
    }
    if (kind === "stall-preflight") {
      return forever();
    }
    // `Promise.resolve` rather than the bare response: `postIntent` calls
    // `.then` on whatever this returns, so a shim answering with the response
    // itself is a `TypeError` on the page rather than a refusal on the card.
    return Promise.resolve(jsonIn(win, kind === "block-preflight" ? BLOCKED_PREFLIGHT : { ok: false }));
  };
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
  pinRuns(win);
  const native = win.fetch.bind(win);
  win.fetch = (input, init) => {
    if (!/\/intent$/u.test(urlOf(input)) || reachesHost(bodyOf(init))) {
      return native(input, init);
    }
    return Promise.reject(new TypeError(`the state lab refused an unmodelled write to ./intent (${kindOf(init) ?? "unreadable body"})`));
  };
}

/**
 * The read half of the same floor: run reads go to the pinned sample.
 *
 * The lab renders every state over `/sample`, but Replay and Live never read
 * `/state` — they read `./runs` and `./runs/:id/events`, which answer from the
 * reader's own `~/.bureau/runs`. So the config half of each run state was
 * pinned and the run half was not: `mode:replay` drew whatever runs this
 * machine had, and on a machine with none it drew no run at all, under the same
 * state id CI screenshots against four committed logs.
 *
 * Rewriting here rather than in the page keeps the frame the production page —
 * it still asks for `./runs` — and puts the pin beside the write floor, which
 * is the other property the lab guarantees about the host regardless of which
 * state is being rendered.
 *
 * Installed *before* the write floor so it ends up underneath it, which is what
 * the run-condition shims need: `offer-ended-run` and `pass-starts-run` match
 * the page's own `./runs` spelling and then delegate to `native` for the body
 * they project, so the listing they decorate has to be the sample's by the time
 * that call lands.
 */
function pinRuns(win) {
  const native = win.fetch.bind(win);
  win.fetch = (input, init) => {
    const url = urlOf(input);
    return /\/runs(\/[A-Za-z0-9][A-Za-z0-9._-]*\/events)?$/u.test(url) && !/\/sample\/runs/u.test(url)
      ? native(url.replace(/\/runs(?=$|\/)/u, "/sample/runs"), init)
      : native(input, init);
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

/**
 * A pass that really started something: the write answers `ok`, and the listing
 * it is read back through gains the run that answer is about.
 *
 * This is the only staged pass whose report names a run, so it is the only
 * condition under which **Open in Replay** is drawn at all. Both halves have to
 * be one condition: an answer without the listing change reports "claimed no
 * work", and a listing change without the answer is a run nothing attributes to
 * this click. The stamp is taken when the pass answers rather than when the
 * shim is installed, because `newRunSince` compares it against the click.
 */
function passStartsRun(win) {
  const native = win.fetch.bind(win);
  let answered = null;
  win.fetch = async (input, init) => {
    if (/\/intent$/u.test(urlOf(input)) && isWrite(init)) {
      answered = Date.now();
      return jsonIn(win, PASS_STARTED);
    }
    if (!/\/runs$/u.test(urlOf(input))) {
      return native(input, init);
    }
    const response = await native(input, init);
    if (!response.ok) {
      return response;
    }
    const payload = await response.json();
    return jsonIn(win, answered === null ? withoutPassRun(payload) : withPassRun(payload, answered));
  };
}

function jsonIn(win, body) {
  return new win.Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
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
