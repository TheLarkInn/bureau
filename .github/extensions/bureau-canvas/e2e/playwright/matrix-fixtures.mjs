// Fixtures for the registry-driven state matrix.
//
// The matrix never writes config — every state is reached by publishing a
// fixture payload into the page — so one read-only canvas host per worker is
// enough, instead of one per test. That is the difference between a suite that
// runs in a minute and one that boots two hundred servers.
//
// Offline like every other suite here: `BUREAU_CANVAS_TEST=1` points the
// binary lookup at a path that does not exist, the runs root is a committed
// fixture directory, and nothing reaches the network or a model.

import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { test as base } from "@playwright/test";

import { collect, CONTRAST, deadlineVerdict, measureFor, selectorsFor, verdict } from "../../web/statelab/checks.mjs";
import { assertAdapter, PUBLISH_EVENT, runPath } from "../../web/statelab/driver.mjs";
import { isPreflight, offeredAsLive, PASS_STARTED, reachesHost, refusalFor, withoutPassRun, withPassRun } from "../../web/statelab/intercept.mjs";
import { staging } from "./gallery-paths.mjs";

const SERVE = fileURLToPath(new URL("../../serve.mjs", import.meta.url));
/*
 * Only the header's config path comes from here. Under `BUREAU_CANVAS_TEST=1`
 * the binary lookup is pointed at a path that does not exist, so `buildState`
 * answers from the committed bundled sample rather than from this directory —
 * which is why an edit to the repository's own `.bureau/` cannot move a single
 * fixture. Worth saying out loud, because the argument reads the other way.
 */
const CONFIG = fileURLToPath(new URL("../../../../../.bureau/", import.meta.url));
const RUNS = fileURLToPath(new URL("../../test/fixtures/runs/", import.meta.url));
/**
 * Where this run's shots go: its staging directory, not the gallery itself.
 * `global-teardown.mjs` publishes it over the gallery when the run rendered
 * anything, which is what keeps a filtered run from deleting a gallery it never
 * refilled — and what keeps two runs in one checkout apart. Specs need no other
 * path.
 *
 * It is a function rather than a constant because it refuses when the run has
 * no staging directory, and Playwright evaluates spec modules while merely
 * *listing* tests — with no `globalSetup`, and so no staging. As a constant
 * that refusal ran at import and took listing down for every spec in the
 * directory, including the eight that have nothing to do with the gallery.
 */
export const galleryDir = staging;

/** Marks a page whose init script already clears the surface's session memory. */
const FRESH = Symbol("fresh-session");

/** Writes the floor had to hold because the state declared no intercept. */
const UNGUARDED = Symbol("unguarded-writes");

async function bootCanvas() {
  const child = spawn(process.execPath, [SERVE, "--dir", CONFIG], {
    env: { ...process.env, BUREAU_CANVAS_TEST: "1", BUREAU_CANVAS_RUNS: RUNS },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const url = await new Promise((resolve, reject) => {
    let output = "";
    const deadline = setTimeout(() => reject(new Error(`canvas host did not print a URL; stderr: ${stderr}`)), 20_000);
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const match = output.match(/Bureau canvas: (http:\/\/127\.0\.0\.1:\d+\/)/u);
      if (match) {
        clearTimeout(deadline);
        resolve(match[1]);
      }
    });
    child.once("exit", (code) => reject(new Error(`canvas host exited ${code} before boot; stderr: ${stderr}`)));
  });
  return { child, url };
}

export const test = base.extend({
  /** One read-only host per worker, plus the payload every fixture projects. */
  host: [
    async ({}, use) => {
      const { child, url } = await bootCanvas();
      const base = await fetch(new URL("/state", url)).then((response) => response.json());
      await mkdir(galleryDir(), { recursive: true });
      await use({ url, base });
      child.kill("SIGTERM");
    },
    { scope: "worker" },
  ],

  /**
   * Every page in this suite sits on the write floor and stands still, before
   * it is navigated and before any spec can add a route of its own.
   *
   * Here rather than in `pageAdapter` because both are properties of the
   * *suite*, not of the driver: `specs/state-lab.spec.mjs` drives the lab
   * through its own `page.goto` and never builds an adapter, so a floor that
   * arrived with the adapter would have left the one spec that clicks through
   * a UI the registry does not enumerate as the only one uncovered.
   */
  page: async ({ page }, use) => {
    await holdWrites(page);
    await freezeMotion(page);
    await use(page);
  },

  /** A page that records every console error and page error it ever saw. */
  watched: async ({ page }, use) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(`pageerror: ${error}`));
    page.on("console", (message) => {
      if (message.type() === "error") {
        errors.push(`console: ${message.text()}`);
      }
    });
    page.on("requestfailed", (request) => {
      // Not filtered here: a state that causes a failed request declares it in
      // `allowErrors`, so the registry stays the only place that says which
      // failures are the state and which are the bug.
      errors.push(`requestfailed: ${request.url()} ${request.failure()?.errorText ?? ""}`);
    });
    await use({ page, errors });
  },
});

/** The driver adapter for a Playwright page. */
/**
 * Both pages publish state on two channels: a `fetch("./state")` on mount and
 * an `EventSource("./events")` whose `state` event the host emits the instant
 * the channel connects. The surface renders as soon as *either* lands, so
 * waiting on the surface alone proves only that one of them arrived — and the
 * loser can then overwrite a fixture published in between with the host's own
 * payload. Measured margin between the two is tens of milliseconds, which is
 * not a budget worth spending under a fully-parallel suite.
 *
 * So wait for both: the surface, and the first SSE `state` delivery, recorded
 * by a wrapper installed before any page script runs. The wrapper only
 * observes; it neither swallows the event nor changes delivery order.
 *
 * Intercepted states skip this — stalling holds `/events` open on purpose, so
 * a flag that never sets is the state under review, not a hang.
 */
const SSE_FLAG = "__bureauSseState";

async function armSseBarrier(page) {
  await page.addInitScript((flag) => {
    window[flag] = false;
    const Native = window.EventSource;
    if (!Native) {
      return;
    }
    class Observed extends Native {
      constructor(...args) {
        super(...args);
        super.addEventListener("state", () => {
          window[flag] = true;
        });
      }
    }
    window.EventSource = Observed;
  }, SSE_FLAG);
}

async function settled(page, target) {
  await page.locator(target === "editor" ? ".editor-tabs" : ".app-header").first().waitFor({ state: "visible" });
  await page.waitForFunction((flag) => window[flag] === true, SSE_FLAG);
}

export function pageAdapter(page, host) {
  return assertAdapter({
    async goto(target, op) {
      // The assignment stack remembers its expanded card in `sessionStorage`.
      // Every walk starts from a fresh session so a path means the same thing
      // however many times it has been walked in this page before.
      //
      // The write floor is already installed by the `page` fixture, which is
      // what makes a state's own route safe to add here: Playwright offers a
      // request to the most recently registered matching handler first, so a
      // `stall-intent` or `fail-intent` added now answers its own writes and
      // the floor only ever sees what nothing else claimed.
      if (!page[FRESH]) {
        await page.addInitScript(() => {
          try {
            sessionStorage.clear();
          } catch {
            // storage disabled; nothing to clear
          }
        });
        await armSseBarrier(page);
        page[FRESH] = true;
      }
      if (op?.intercept) {
        await intercept(page, op.intercept);
      }
      // Only a pre-surface intercept changes how the page is waited for. An
      // intent route leaves the boot untouched, so the state still has to
      // settle like every other one — skipping the barrier there would judge a
      // half-rendered page and call the race a state.
      const preSurface = PRE_SURFACE.has(op?.intercept);
      const path = target === "editor" ? "editor.html" : "index.html";
      await page.goto(new URL(path, host.url).href, { waitUntil: preSurface ? "commit" : "load" });
      if (!preSurface) {
        await settled(page, target);
      }
    },
    publish: (state) =>
      page.evaluate(
        ({ name, detail }) => window.dispatchEvent(new CustomEvent(name, { detail })),
        { name: PUBLISH_EVENT, detail: state },
      ),
    click: (selector) => page.locator(selector).first().click(),
    fill: (selector, value) => page.locator(selector).first().fill(String(value)),
    select: (selector, value) => page.locator(selector).first().selectOption(String(value)),
    press: (selector, key) => page.locator(selector).first().press(key),
    drag: (selector, dx, dy) => dragBy(page, selector, dx, dy),
    wait: (selector) => page.locator(selector).first().waitFor({ state: "visible" }),
    present: (selector) => page.locator(selector).first().waitFor({ state: "attached" }),
    waitGone: (selector) => page.locator(selector).first().waitFor({ state: "hidden" }),
  });
}

/**
 * The intercepts that must be in place before the page has a surface at all.
 * Everything else routes a request the page makes later, so the load and the
 * settle barrier proceed normally.
 */
const PRE_SURFACE = new Set(["block-renderer", "block-editor-renderer", "stall-state"]);

/**
 * Request-level conditions a state can be under.
 *
 * The first three are pre-surface: blocking the renderer module is how a CDN
 * or a proxy breaks this page in the field, and stalling the payload is what a
 * slow CLI looks like. Stalling has to cover `/events` as well as `/state`,
 * because the SSE channel writes the current state the moment it connects, so
 * blocking the fetch alone still renders a full surface.
 *
 * The last two are the two ends of a save. They route `./intent`, which is
 * what every `set-*` posts to, so the save is answered inside the browser and
 * the shared host is never written to. Stalling holds `busy` on, which is the
 * "Saving…" screen; refusing returns a non-`ok` response, and `postIntent`
 * maps that to `null`, which is the branch that renders each editor's own
 * fallback sentence. Both are exact rather than timing-dependent, which is why
 * they can be enumerated states instead of a note about what cannot be shown.
 *
 * They match on the intent's `kind`, not on the URL, because `./intent` is
 * also how the page *reads*: `derive-work-source` builds the paste preview and
 * `resolve-repo` looks a repository up, and neither writes anything. Routing
 * the whole endpoint stalled the preview the save state is supposed to be
 * saving — the editor sat with no derivation, so there was nothing to submit.
 *
 * So the two reads are named and everything else is held. Naming the *writes*
 * instead is how this was first written, and it was wrong in the dangerous
 * direction: `save-pipeline`, `save-plan`, `discard-plan` and the create and
 * delete intents all failed a `set-` prefix test and would have gone straight
 * to the host — which is the contributor's own `.bureau/`, shared by every
 * state on that worker. Four constraints exist to keep those intents out of
 * the matrix, so nothing would have failed; the config would just have been
 * quietly rewritten underneath the run. An intent added later is held by
 * default, and a body that cannot be parsed is held too.
 *
 * Held *by whom* is the second half, and it was missing for a while. Default-
 * deny is a property of this route, and this route was installed only for a
 * state that asked for one — so "no write reaches the host" was a fact about
 * which paths happened to click what, not a guarantee. `holdWrites` installs
 * the same deny unconditionally now, under every state, so the next path to
 * click a Save it never modelled is stopped and named instead of landing on
 * disk. `installFloor` does the same job inside the lab's frame, where it
 * matters more still: that host is pointed at a contributor's own `.bureau/`.
 *
 * `reachesHost` and `refusalFor` come from `web/statelab/intercept.mjs`, which
 * the lab installs inside its frame. One definition, two applications: the
 * screen a reviewer browses is under the same condition CI asserts — and one
 * predicate for what writes, so the floor and the save intercepts cannot
 * disagree about a request the way they once did about the delete preflight.
 */

/**
 * The floor under every page in this suite: `./intent` is held unless it
 * writes nothing.
 *
 * The default-deny above is a property of the *route*, and until now the route
 * was only installed for states that asked for one — so "no intent reaches the
 * host" held by coincidence of which paths happened to click what, not by
 * construction. It happened to be true, and the next state to click a Save
 * without declaring an intercept would have rewritten the contributor's own
 * `.bureau/`, shared by every state on this worker, with nothing failing.
 *
 * So the deny is unconditional now, and a held write is recorded rather than
 * merely blocked: aborting alone would surface as some unrelated control
 * failing its checks, which is a bug report nobody can read. `judge` turns each
 * recorded kind into a named failure against the state that provoked it.
 */
async function holdWrites(page) {
  page[UNGUARDED] = [];
  await page.route(/\/intent$/u, (route) => {
    const body = intentBody(route);
    if (reachesHost(body)) {
      route.continue();
      return;
    }
    page[UNGUARDED].push(body?.kind ?? "an unparseable intent");
    route.abort("failed");
  });
}

async function intercept(page, kind) {
  const routes = {
    "block-renderer": () => page.route(/app\.mjs$/u, (route) => route.abort("failed")),
    "block-editor-renderer": () => page.route(/editor\/index\.mjs$/u, (route) => route.abort("failed")),
    "stall-state": () => page.route(/\/(state|events)$/u, () => {}),
    // The live listing a moment after the watched run reached its terminal.
    // A committed log is live or finished and never both, so this is the only
    // way a static log renders the screen a reader is left on when the run they
    // picked ends under them. `offeredAsLive` is the lab's own projection, so
    // both hosts report the same listing.
    "offer-ended-run": () => page.route(/\/runs$/u, async (route) => {
      const response = await route.fetch();
      route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(offeredAsLive(await response.json())) });
    }),
    "empty-runs": () => page.route(/\/runs$/u, (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ runs: [] }) })),
    // The read that has not come back. Every other listing route is an answer;
    // this is the wait, and it is the only condition under which the Live badge
    // legitimately carries no `data-count` at all.
    "stall-runs": () => page.route(/\/runs$/u, () => {}),
    "fail-runs": () => page.route(/\/runs$/u, (route) =>
      route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "run listing unavailable" }) })),
    // Serves the real listing once, then refuses. A run can only be selected
    // from a listing that answered, so this is the only way to reach the screen
    // where a run is being watched while the listing has since failed.
    "fail-runs-later": () => {
      let served = 0;
      return page.route(/\/runs$/u, (route) => {
        served += 1;
        return served === 1
          ? route.continue()
          : route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "run listing unavailable" }) });
      });
    },
    "stall-intent": () => page.route(/\/intent$/u, (route) => writes(route) || route.continue()),
    "fail-intent": () => page.route(/\/intent$/u, (route) => writes(route)
      ? route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(refusalFor(intentKind(route))) })
      : route.continue()),
    // The one refusal `fail-intent` cannot stage, because the request it is
    // about is deliberately not a write. `reachesHost` lets the unconfirmed
    // delete through to the host so the confirmation prompt has referrers to
    // draw — which also means the *failure* of that read was unrenderable, and
    // `DeleteControl`'s `!preflight` branch draws a note there that no state
    // ever showed. Scoped to the preflight alone so everything else on the page
    // still answers normally: the state is one refused read, not a dead host.
    "refuse-preflight": () => page.route(/\/intent$/u, (route) => isPreflight(intentBody(route))
      ? route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: false }) })
      : route.continue()),
    // The end a write can also have: it worked. Only the reconcile pass needs
    // it — its success is three sentences about what the pass did, and no state
    // rendered any of them, so `reconcileResult` was a selector nothing used.
    // The answer is synthesised here and the host is never written to, so the
    // listing the report re-reads is unchanged and "claimed no work" is exact.
    "pass-intent": () => page.route(/\/intent$/u, (route) => writes(route)
      ? route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, output: "no eligible work" }) })
      : route.continue()),
    // The other end of a pass: it started a run, and said which. Both halves
    // are one condition — the listing withholds that run until the write has
    // answered, so `newRunSince` can attribute it to this click and the report
    // names it. Without the withholding the run is already in `known` and the
    // pass reports "claimed no work"; without the answer no report is drawn at
    // all. This is the only condition under which **Open in Replay** exists.
    "pass-starts-run": () => {
      let answered = null;
      return Promise.all([
        page.route(/\/intent$/u, (route) => {
          if (!writes(route)) {
            return route.continue();
          }
          answered = Date.now();
          return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(PASS_STARTED) });
        }),
        page.route(/\/runs$/u, async (route) => {
          const response = await route.fetch();
          const payload = await response.json();
          const listing = answered === null ? withoutPassRun(payload) : withPassRun(payload, answered);
          return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(listing) });
        }),
      ]);
    },
    // The third end of a save, and the one that used to have no screen: the
    // request never reaches a responder at all, so `fetch` rejects rather than
    // answering. `fail-intent` cannot stand in for it — a 500 resolves, and it
    // is precisely the difference between resolving and rejecting that decided
    // whether the page cleared its `busy` flag.
    "abort-intent": () => page.route(/\/intent$/u, (route) => writes(route)
      ? route.abort("failed")
      : route.continue()),
  };
  await routes[kind]();
}

/** Whether this intent would write: anything the floor would not let through. */
function writes(route) {
  return !reachesHost(intentBody(route));
}

function intentKind(route) {
  return intentBody(route)?.kind ?? null;
}

function intentBody(route) {
  try {
    return JSON.parse(route.request().postData() ?? "{}");
  } catch {
    return null;
  }
}

/*
 * The refusal shape lives in `web/statelab/intercept.mjs` as `refusalFor`,
 * beside the rest of the interception semantics, so the lab installs exactly
 * the refusal this suite asserts.
 *
 * The shape is load-bearing. `extension.mjs` writes every intent answer through
 * `sendJson`, which is hard-coded to HTTP 200 — a refusal is `{ ok: false }` in
 * a 200 body, never a 500. Answering with a 500 was therefore not "the host
 * refused" but "the host broke". And a refused `save-pipeline` always carries
 * the findings that say why the write was reverted (`lib/pipeline.mjs` reverts
 * only when `roundTrip` returns some); dropping the body left the panel with
 * nothing to draw but the words "save failed", so the one state whose whole
 * subject is *why a save was refused* was rendering the one refusal that gives
 * no reason.
 */

async function dragBy(page, selector, dx, dy) {
  const box = await page.locator(selector).first().boundingBox();
  const from = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + dx / 2, from.y + dy / 2, { steps: 6 });
  await page.mouse.move(from.x + dx, from.y + dy, { steps: 6 });
  await page.mouse.up();
}

/** Walks a state's entry path and returns the registry's verdict on the render. */
export async function enterState(state, page, host) {
  await runPath(state.ops, pageAdapter(page, host), host.base);
  return judge(state, page);
}

/**
 * Applies further operations to a page already sitting in some state, then
 * judges it as `state`. This is how a transition is walked as a transition
 * rather than as a second entry from scratch.
 */
export async function applyOps(ops, state, page, host) {
  await runPath(ops, pageAdapter(page, host), host.base);
  return judge(state, page);
}

const SETTLE_MS = 5000;
const SETTLE_POLL_MS = 100;
/**
 * How many consecutive samples must agree before a render is called finished.
 *
 * One was not enough. React Flow appends the relation graph's edges in a pass
 * after it has measured the cards, and under a fully-parallel suite that pass
 * lands wherever the scheduler puts it — so a single repeat caught the surface
 * mid-draw and filed a graph of disconnected boxes. Measured on this tree, two
 * runs of one matrix disagreed on 81 of 500 renders; widening the window to
 * three agreeing samples took that to about 60.
 *
 * It is not zero, and the honest reason is that no wait can make it zero: the
 * same states are stable at one worker and drift at four, so what is being
 * waited out is CPU contention rather than a step the page takes. A
 * MutationObserver gate was tried here instead of the poll — the document
 * itself reporting it had stopped, rather than agreement across sampled
 * instants — and measured 61 of 500, no better than the poll for an extra
 * round trip per sample. So the poll stays, the residue is #116, and that is
 * why the twin audit reports rather than gates.
 */
const SETTLE_REPEATS = 3;

/**
 * Judges the render once it has settled, and settles on the render itself.
 *
 * The verdict is a single `page.evaluate`, not a locator, so nothing about it
 * retries — it judges whichever frame it lands on. Most paths are settled by
 * the time it runs, but some add nodes to a React Flow graph after the surface
 * is up: a fixture's orphan cards arrive with the payload, and React Flow lays
 * a new node out at `visibility: hidden` until its measurement lands. Sampling
 * between those two moments reports a control missing that is one frame away.
 *
 * So it re-samples to a deadline, the way `web/statelab/lab.mjs` already does
 * and the way Playwright's own `expect` does. It reports whatever the last
 * look found, so a state that is genuinely wrong still fails — it just takes
 * the full budget to say so.
 *
 * Meeting the expectations is not the same as being finished, though, and that
 * gap is what made the gallery undiffable. No state promises the relation
 * graph's *edges* — the shared renderer draws them a frame after React Flow has
 * measured the cards — so the loop stopped as soon as the cards were up and the
 * capture landed on a graph of disconnected boxes about half the time. Sixty-
 * seven of 250 renders came out differently on a second run of the same tree,
 * and a reviewer diffing two galleries could not tell a change from a frame.
 *
 * The rule is therefore the render, not a list of what to wait for: a state is
 * settled when its own signature stops changing. That needs no knowledge of
 * which surfaces animate or which library measures late, and it cannot fall out
 * of date the way a hand-kept list of selectors does.
 *
 * "Stops changing" needed a wider window than one poll, which `SETTLE_REPEATS`
 * above explains and measures.
 *
 * What answers when the window runs out is `deadlineVerdict`, and the rule is
 * there rather than here so the offline suite can hold it. The short version:
 * a failure that flickers is the harness and a failure that stays is the
 * product, so an observed clean look wins only while the failures are not yet
 * sustained.
 *
 * The *snapshot* is always the last look, whichever sample supplied the
 * verdict, and those two being allowed to disagree is deliberate. The verdict
 * answers "was this state ever correct inside its settle window"; the snapshot
 * describes what is on the page now — which is the frame `state-matrix.spec.mjs`
 * screenshots a moment later and files a signature for. Handing back an earlier
 * snapshot filed a signature for a frame the gallery does not show, so the twin
 * audit compared descriptions of renders nobody could look at.
 */
async function judge(state, page) {
  const deadline = Date.now() + SETTLE_MS;
  let result = await sample(state, page);
  let clean = result.failures.length ? null : result;
  let sustained = result.failures.length ? 1 : 0;
  let agreed = 0;
  let previous = null;
  while (Date.now() < deadline) {
    agreed = result.snapshot.signature === previous ? agreed + 1 : 0;
    if (isSettled(result, agreed)) {
      return result;
    }
    previous = result.snapshot.signature;
    await page.waitForTimeout(SETTLE_POLL_MS);
    result = await sample(state, page);
    sustained = result.failures.length ? sustained + 1 : 0;
    clean ??= result.failures.length ? null : result;
  }
  const source = deadlineVerdict(
    { lastFailed: result.failures.length > 0, sustained, sawClean: Boolean(clean) },
    SETTLE_REPEATS,
  );
  return source === "last" ? result : { snapshot: result.snapshot, failures: clean.failures };
}

function isSettled(result, agreed) {
  return result.failures.length === 0 && agreed >= SETTLE_REPEATS;
}

async function sample(state, page) {
  const snapshot = await page.evaluate(
    ({ source, request }) => new Function(`return (${source})`)()(document, request),
    { source: collect.toString(), request: { selectors: selectorsFor(state), measure: measureFor(state), contrast: CONTRAST } },
  );
  return { snapshot, failures: [...heldWrites(page), ...verdict(state, snapshot, { slack: 2 })] };
}

/**
 * Writes the floor had to hold, reported against the state that posted them.
 *
 * This is not a render fault, so it is not `verdict`'s to find: it says the
 * matrix tried to act on the host and only the floor stopped it. A state that
 * wants one of these screens declares `stall-intent`, `fail-intent` or
 * `abort-intent` and owns the outcome; one that does not is asking for a write
 * it never modelled.
 */
function heldWrites(page) {
  return (page[UNGUARDED] ?? []).map((kind) => ({
    kind: "unguarded-write",
    detail: `\`${kind}\` was posted to ./intent with no intercept declared for it, so this path would have written to the host`,
  }));
}

/**
 * Stops the surface moving, so a render is a state rather than a moment.
 *
 * The assignment caret rotates through 90° over 0.15s when a card opens, and
 * both the screenshot and the measurement land somewhere inside that arc — so
 * one state rendered twice gave two different boxes for it (`12×25` at x=849,
 * `19×25` at x=845) and two different pictures. That was not a rare race: 242
 * of 500 renders differed between two runs of the same tree, which makes a
 * gallery undiffable and a render-distinctness check unusable, because
 * "these two states draw the same screen" cannot be told from "these two files
 * were written a frame apart".
 *
 * Motion is frozen rather than waited out. Waiting needs a list of every
 * animated property on the surface, kept by hand and wrong the moment someone
 * adds a transition; a matrix asserts settled states, and the settled state is
 * exactly what a disabled transition leaves on screen. The caret is also why
 * `caret-color` is here — a blinking text caret in an open field is the same
 * defect with a shorter period.
 *
 * It rides on `addInitScript` so it is in place before the page's own modules
 * run, and it reaches the lab's iframe too, which is what keeps the two hosts
 * agreeing about what a state looks like.
 */
async function freezeMotion(page) {
  await page.addInitScript(() => {
    const install = () => {
      const style = document.createElement("style");
      style.textContent = "*, *::before, *::after { transition: none !important; animation: none !important; caret-color: transparent !important; }";
      document.head.append(style);
    };
    if (document.head) {
      install();
    } else {
      document.addEventListener("DOMContentLoaded", install, { once: true });
    }
  });
}

export { expect } from "@playwright/test";
