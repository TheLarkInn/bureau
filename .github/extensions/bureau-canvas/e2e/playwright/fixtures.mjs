import { spawn } from "node:child_process";
import { cp, mkdir, mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test as base } from "@playwright/test";

const SERVE = fileURLToPath(new URL("../../serve.mjs", import.meta.url));
const SCRATCH_ROOT = fileURLToPath(new URL("../../../../../target/canvas-playwright/", import.meta.url));
const CONFIG_FIXTURE = fileURLToPath(new URL("../../../../../.bureau/", import.meta.url));

/**
 * The config these specs assert against.
 *
 * `BUREAU_CANVAS_TEST=1` deliberately points the binary lookup at a path that
 * does not exist, so the host serves its bundled sample instead of shelling
 * out to `bureau validate`. That is the extension's own hermetic test mode:
 * no binary, no network, and the same committed payload every run. The shape
 * below is what `test/fixtures/committed-payload.json` holds, restated so a
 * spec can be read without opening the fixture.
 */
export const SAMPLE = {
  assignment: "agent-eligible",
  source: "TheLarkInn/bureau",
  filter: "is:open label:agent-eligible",
  repos: [{ name: "bureau", access: "push" }],
  role: { name: "implementer", adapter: "copilot", agent: "/bureau:implementer", minTrust: "maintainer", grants: 3 },
  otherRole: { name: "reviewer", minTrust: "derived", grants: 2 },
  limits: { capped: 2, uncapped: 3 },
};

/** Boots `serve.mjs` on an ephemeral port and resolves the address it prints. */
async function scratchConfig() {
  await mkdir(SCRATCH_ROOT, { recursive: true });
  const dir = await mkdtemp(join(SCRATCH_ROOT, "cfg-"));
  await cp(CONFIG_FIXTURE, dir, { recursive: true });
  return dir;
}

async function bootCanvas(dir) {
  const child = spawn(process.execPath, [SERVE, "--dir", dir], {
    env: { ...process.env, BUREAU_CANVAS_TEST: "1" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const url = await new Promise((resolveUrl, rejectUrl) => {
    let output = "";
    const deadline = setTimeout(() => rejectUrl(new Error(`canvas host did not print a URL; stderr: ${stderr}`)), 20_000);
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const match = output.match(/Bureau canvas: (http:\/\/127\.0\.0\.1:\d+\/)/u);
      if (match) {
        clearTimeout(deadline);
        resolveUrl(match[1]);
      }
    });
    child.once("exit", (code) => rejectUrl(new Error(`canvas host exited ${code} before boot; stderr: ${stderr}`)));
  });
  return { child, url };
}

async function resetView(url) {
  await fetch(new URL("/intent", url), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "back-to-config" }),
  });
}

export const test = base.extend({
  /** One canvas host and scratch config per test, so writes are always safe. */
  canvas: async ({}, use) => {
    const dir = await scratchConfig();
    const { child, url } = await bootCanvas(dir);
    await use({ url, dir });
    child.kill("SIGTERM");
    await rm(dir, { recursive: true, force: true });
  },

  /** The canvas page with the assignment card expanded, watching for errors. */
  card: async ({ page, canvas }, use) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    page.on("console", (message) => {
      if (message.type() === "error") {
        errors.push(message.text());
      }
    });
    await resetView(canvas.url);
    await page.goto(canvas.url);
    await page.locator(".assignment-card").first().waitFor();
    await page.locator(".assignment-head").first().click();
    await page.locator(".assignment-detail").waitFor();
    await use({ page, errors });
  },

  /** The pipeline editor, reached through the same path a user takes. */
  editor: async ({ page, canvas }, use) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    page.on("console", (message) => {
      if (message.type() === "error") {
        errors.push(message.text());
      }
    });
    await resetView(canvas.url);
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("link", { name: "Edit" }).click();
    await page.locator(".editor-shell").waitFor();
    await use({ page, errors });
  },
});

export { expect } from "@playwright/test";
