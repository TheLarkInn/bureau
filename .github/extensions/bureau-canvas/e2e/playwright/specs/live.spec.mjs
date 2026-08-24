import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, SAMPLE, test } from "../fixtures.mjs";

async function openPipeline(page, canvas) {
  await page.goto(canvas.url);
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
}

/** Writes one live run into this worker's run root, before the page is opened. */
async function seedRun(canvas, runId) {
  const dir = join(canvas.runs, runId);
  const events = [
    { seq: 0, at_ms: 1_800_000_000_000, kind: "run_started", data: { run_id: runId, assignment: SAMPLE.assignment, pipeline: "agent-eligible-pipeline" } },
    { seq: 1, at_ms: 1_800_000_001_000, kind: "step_started", data: { run_id: runId, step: "implement" } },
  ];
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
}

test("Live distinguishes an idle reconciler from a broken listing", async ({ page, canvas }) => {
  await openPipeline(page, canvas);

  await expect(page.getByTestId("live-count")).toHaveAttribute("data-count", "0");
  await page.getByRole("tab", { name: /live, 0 runs in progress/iu }).click();
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "idle");
  await expect(page.getByText("A reconcile loop is not itself a run.")).toBeVisible();
  await expect(page.getByLabel("Live run")).toBeDisabled();
  await expect(page.getByRole("button", { name: "Run reconcile now" })).toBeEnabled();
});

test("Live advertises and follows the newest run without a manual pick", async ({ page, canvas }) => {
  const runId = "live-fixture-run";
  await seedRun(canvas, runId);
  await openPipeline(page, canvas);

  await expect(page.getByTestId("live-count")).toHaveAttribute("data-count", "1");
  await page.getByRole("tab", { name: /live, 1 run in progress/iu }).click();
  await page.getByLabel("Live run").selectOption(runId);
  await expect(page.locator(".flow-card.overlay-running")).toBeVisible();
});

test("Run reconcile now stays busy until the pass finishes, then says what it did", async ({ page, canvas }) => {
  let finish = () => {};
  const held = new Promise((resolve) => {
    finish = resolve;
  });
  await page.route("**/intent", async (route) => {
    const body = route.request().postDataJSON();
    if (body?.kind !== "reconcile-now") {
      await route.continue();
      return;
    }
    await held;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, output: "no eligible work" }),
    });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();

  await page.getByRole("button", { name: "Run reconcile now" }).click();
  await expect(page.getByRole("button", { name: "Reconciling…" })).toBeDisabled();
  finish();
  await expect(page.getByRole("status")).toHaveText("Reconcile pass finished. It claimed no work for this pipeline.");
  await expect(page.getByRole("button", { name: "Run reconcile now" })).toBeEnabled();
});

/*
 * The pass reports on itself, and moves nothing it did not start.
 *
 * A refused pass that still selected a run was drawing a paused run the reader
 * never asked for beside the sentence saying the pass could not be started —
 * two claims about the same click that cannot both be true.
 */
test("a refused reconcile pass moves nothing", async ({ page, canvas }) => {
  await seedRun(canvas, "already-running");
  await page.route("**/intent", async (route) => {
    if (route.request().postDataJSON()?.kind !== "reconcile-now") {
      await route.continue();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: false, error: "bureau binary not available" }),
    });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "available");

  await page.getByRole("button", { name: "Run reconcile now" }).click();

  await expect(page.locator(".run-control-error")).toHaveText("bureau binary not available");
  await expect(page.getByLabel("Live run")).toHaveValue("");
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "available");
  await expect(page.locator(".flow-card.overlay-running")).toHaveCount(0);
});
