import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test as base } from "@playwright/test";

const SERVE = fileURLToPath(new URL("../../serve.mjs", import.meta.url));

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
async function bootCanvas() {
  const child = spawn(process.execPath, [SERVE], {
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

export const test = base.extend({
  /** One canvas host per worker, serving the bundled sample. */
  canvas: [
    async ({}, use) => {
      const { child, url } = await bootCanvas();
      await use({ url });
      child.kill("SIGTERM");
    },
    { scope: "worker" },
  ],

  /** The canvas page with the assignment card expanded, watching for errors. */
  card: async ({ page, canvas }, use) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    page.on("console", (message) => {
      if (message.type() === "error") {
        errors.push(message.text());
      }
    });
    await page.goto(canvas.url);
    await page.locator(".assignment-card").first().waitFor();
    await page.locator(".assignment-head").first().click();
    await page.locator(".assignment-detail").waitFor();
    await use({ page, errors });
  },
});

export { expect } from "@playwright/test";
