import { spawn } from "node:child_process";
import { cp, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test as base } from "@playwright/test";

import { holdOffline, offlineFindings } from "./offline.mjs";

const SERVE = fileURLToPath(new URL("../../serve.mjs", import.meta.url));
const SCRATCH_ROOT = fileURLToPath(new URL("../../../../../target/canvas-playwright/", import.meta.url));
const CONFIG_FIXTURE = fileURLToPath(new URL("../../../../../.bureau/", import.meta.url));

/** The one finished run the replay specs scrub through. */
export const RUN_ID = "replay-fixture-run";
const RUN_START = 1_700_000_000_000;

/** What an agent step streams: a Copilot CLI transcript. */
export const TRANSCRIPT = [
  "● Read DESIGN.md",
  "  │ DESIGN.md",
  "  └ L1:120 (120 lines read)",
  "",
  "I read the contract before editing.",
  "",
].join("\n");

/** What a deterministic step streams: one v2 contract line. */
export const CONTRACT = '{"schema":"v2","outcome":"success","outputs":{"tests":42},'
  + '"artifacts":[],"trust":"derived","message":"suite passed"}';

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

/**
 * A `runs/` dir holding one finished run of the fixture pipeline, so the
 * replay specs have a real event log to scrub without a `bureau` binary or a
 * live engine. Step names match `agent-eligible-pipeline`, because overlays
 * key on step name, and the output covers both shapes a step can produce: an
 * agent's CLI transcript and a deterministic step's contract line.
 */
async function scratchRuns() {
  await mkdir(SCRATCH_ROOT, { recursive: true });
  const dir = await mkdtemp(join(SCRATCH_ROOT, "runs-"));
  const events = [
    { kind: "run_started", data: { run_id: RUN_ID, assignment: SAMPLE.assignment } },
    { kind: "step_started", data: { step: "implement" } },
    { kind: "output", data: { step: "implement", stream: "combined", data: TRANSCRIPT } },
    { kind: "step_finished", data: { step: "implement", outcome: "success" } },
    { kind: "step_started", data: { step: "verify" } },
    { kind: "output", data: { step: "verify", stream: "combined", data: `${CONTRACT}\n` } },
    { kind: "step_finished", data: { step: "verify", outcome: "success" } },
    { kind: "step_started", data: { step: "review" } },
    { kind: "step_finished", data: { step: "review", outcome: "success" } },
    { kind: "run_finished", data: { outcome: "success" } },
  ].map((event, seq) => ({ seq, at_ms: RUN_START + seq * 1000, ...event }));
  await mkdir(join(dir, RUN_ID), { recursive: true });
  await writeFile(join(dir, RUN_ID, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
  return dir;
}

async function bootCanvas(dir, runs) {
  const child = spawn(process.execPath, [SERVE, "--dir", dir], {
    env: { ...process.env, BUREAU_CANVAS_TEST: "1", BUREAU_CANVAS_RUNS: runs },
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
      const match = output.match(/Bureau dashboard: (http:\/\/127\.0\.0\.1:\d+\/)/u);
      if (match) {
        clearTimeout(deadline);
        resolveUrl(match[1]);
      }
    });
    child.once("exit", (code) => rejectUrl(new Error(`canvas host exited ${code} before boot; stderr: ${stderr}`)));
  });
  return { child, url };
}

async function resetView(page, url) {
  await page.goto(url);
  await page.evaluate(() => fetch("./intent", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "back-to-config" }),
    }));
}

export const test = base.extend({
  /**
   * Every page in this suite sits on the offline floor before it is navigated.
   *
   * The header above has claimed this suite is offline since it was written, and
   * nothing in it could contradict the claim: a `fetch` to a public host added
   * to `web/app.mjs`, swallowing its own rejection, left all 147 tests green.
   * The floor refuses the request and the throw below is what makes the refusal
   * a check — an abort on its own shows up as some unrelated control missing,
   * which is a bug report nobody can read.
   *
   * It is asserted after `use` rather than inside a spec so that it holds for
   * every test in the suite including ones written later, and so no spec has to
   * remember to ask.
   */
  page: async ({ page }, use) => {
    const reached = await holdOffline(page);
    await use(page);
    const findings = offlineFindings(reached);
    if (findings.length) {
      throw new Error(findings.map((finding) => finding.detail).join("\n"));
    }
  },

  /** One canvas host and scratch config per test, so writes are always safe. */
  canvas: async ({}, use) => {
    const dir = await scratchConfig();
    const runs = await scratchRuns();
    const { child, url } = await bootCanvas(dir, runs);
    await use({ url, dir, runs });
    child.kill("SIGTERM");
    await rm(dir, { recursive: true, force: true });
    await rm(runs, { recursive: true, force: true });
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
    await resetView(page, canvas.url);
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
    await resetView(page, canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("link", { name: "Edit" }).click();
    await page.locator(".editor-shell").waitFor();
    await use({ page, errors });
  },
});

export { expect } from "@playwright/test";
