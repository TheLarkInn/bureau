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

import { collect, CONTRAST, measureFor, selectorsFor, verdict } from "../../web/statelab/checks.mjs";
import { assertAdapter, PUBLISH_EVENT, runPath } from "../../web/statelab/driver.mjs";
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
      if (op?.intercept) {
        await intercept(page, op.intercept);
      }
      // Only a pre-surface intercept changes how the page is waited for. An
      // intent route leaves the boot untouched, so the state still has to
      // settle like every other one — skipping the barrier there would judge a
      // half-rendered page and call the race a state.
      const preSurface = PRE_SURFACE.has(op?.intercept);
      // The assignment stack remembers its expanded card in `sessionStorage`.
      // Every walk starts from a fresh session so a path means the same thing
      // however many times it has been walked in this page before.
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
 * quietly rewritten underneath the run. An intent added later is now held by
 * default, and a body that cannot be parsed is held too.
 */
const READ_INTENTS = new Set(["derive-work-source", "resolve-repo"]);

async function intercept(page, kind) {
  const routes = {
    "block-renderer": () => page.route(/app\.mjs$/u, (route) => route.abort("failed")),
    "block-editor-renderer": () => page.route(/editor\/index\.mjs$/u, (route) => route.abort("failed")),
    "stall-state": () => page.route(/\/(state|events)$/u, () => {}),
    "stall-intent": () => page.route(/\/intent$/u, (route) => writes(route) || route.continue()),
    "fail-intent": () => page.route(/\/intent$/u, (route) => writes(route)
      ? route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ ok: false }) })
      : route.continue()),
  };
  await routes[kind]();
}

/** Whether this intent is anything other than one of the two known reads. */
function writes(route) {
  try {
    return !READ_INTENTS.has(JSON.parse(route.request().postData() ?? "{}").kind);
  } catch {
    return true;
  }
}

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
 * Judges the render once it has settled.
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
 */
async function judge(state, page) {
  const deadline = Date.now() + SETTLE_MS;
  let result = await sample(state, page);
  while (result.failures.length > 0 && Date.now() < deadline) {
    await page.waitForTimeout(SETTLE_POLL_MS);
    result = await sample(state, page);
  }
  return result;
}

async function sample(state, page) {
  const snapshot = await page.evaluate(
    ({ source, request }) => new Function(`return (${source})`)()(document, request),
    { source: collect.toString(), request: { selectors: selectorsFor(state), measure: measureFor(state), contrast: CONTRAST } },
  );
  return { snapshot, failures: verdict(state, snapshot, { slack: 2 }) };
}

export { expect } from "@playwright/test";
