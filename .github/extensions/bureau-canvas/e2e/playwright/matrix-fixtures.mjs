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

import { collect, CONTRAST, MEASURED, selectorsFor, verdict } from "../../web/statelab/checks.mjs";
import { assertAdapter, PUBLISH_EVENT, runPath } from "../../web/statelab/driver.mjs";

const SERVE = fileURLToPath(new URL("../../serve.mjs", import.meta.url));
const CONFIG = fileURLToPath(new URL("../../../../../.bureau/", import.meta.url));
const RUNS = fileURLToPath(new URL("../../test/fixtures/runs/", import.meta.url));
export const GALLERY = fileURLToPath(new URL("../gallery/", import.meta.url));

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
      await mkdir(GALLERY, { recursive: true });
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
      if (!request.url().includes("statelab-intercept")) {
        errors.push(`requestfailed: ${request.url()} ${request.failure()?.errorText ?? ""}`);
      }
    });
    await use({ page, errors });
  },
});

/** The driver adapter for a Playwright page. */
export function pageAdapter(page, host) {
  return assertAdapter({
    async goto(target, op) {
      if (op?.intercept) {
        await intercept(page, op.intercept);
      }
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
        page[FRESH] = true;
      }
      const path = target === "editor" ? "editor.html" : "index.html";
      await page.goto(new URL(path, host.url).href, { waitUntil: op?.intercept ? "commit" : "load" });
      if (!op?.intercept) {
        // The page fetches `/state` on mount. Publishing a fixture before that
        // lands would be overwritten by the server's own payload a moment
        // later, so wait for the surface the initial state produces.
        await page.locator(target === "editor" ? ".editor-tabs" : ".app-header").first().waitFor({ state: "visible" });
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
 * The two pre-surface states. Blocking the renderer module is how a CDN or a
 * proxy breaks this page in the field; stalling the payload is what a slow CLI
 * looks like. Both are request-level, so they are produced here rather than by
 * a production flag that only tests would ever set.
 *
 * Stalling has to cover `/events` as well as `/state`: the SSE channel writes
 * the current state the moment it connects, so blocking the fetch alone still
 * renders a full surface.
 */
async function intercept(page, kind) {
  if (kind === "block-renderer") {
    await page.route(/app\.mjs$/u, (route) => route.abort("failed"));
    return;
  }
  await page.route(/\/(state|events)$/u, () => {});
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

async function judge(state, page) {
  const snapshot = await page.evaluate(
    ({ source, request }) => new Function(`return (${source})`)()(document, request),
    { source: collect.toString(), request: { selectors: selectorsFor(state), measure: MEASURED, contrast: CONTRAST } },
  );
  return { snapshot, failures: verdict(state, snapshot, { slack: 2 }) };
}

export { expect } from "@playwright/test";
