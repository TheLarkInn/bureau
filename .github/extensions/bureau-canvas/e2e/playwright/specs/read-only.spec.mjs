import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, SAMPLE, test } from "../fixtures.mjs";

test.use({ entryView: "operations", readOnly: true });
const PIPELINE = "agent-eligible-pipeline";

async function seed(canvas, id, paused) {
  const events = [
    { kind: "run_started", data: { run_id: id, assignment: SAMPLE.assignment, snapshot: { pipeline: { name: PIPELINE } } } },
    { kind: "step_started", data: { step: "implement" } },
    ...(paused ? [{ kind: "output", data: { stream: "run", data: "run paused at a step boundary: operator pause" } }] : []),
  ].map((event, seq) => ({ ...event, seq, at_ms: Date.now() - 1000 + seq }));
  await mkdir(join(canvas.runs, id));
  await writeFile(join(canvas.runs, id, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
}

test("managed read-only mode keeps inspection reachable and disables configuration mutations with a reason", async ({ page, canvas }) => {
  await page.setViewportSize({ width: 375, height: 900 });
  await page.goto(canvas.url);
  await expect(page.locator("#read-only-notice")).toContainText("Managed dispatch belongs to the daemon");
  await expect(page.getByRole("heading", { name: "Configuration source (read-only)", exact: true })).toBeVisible();
  await page.getByRole("button", { name: `Inspect assignment ${SAMPLE.assignment}`, exact: true }).click();
  await expect(page.locator(".assignment-head")).toHaveAttribute("aria-expanded", "true");
  for (const selector of [".ws-value", ".runtime-value", ".terminal-label-value", ".repos-value", ".limits-value",
    '[data-testid="create-open"]', '[data-testid="delete-start"]']) {
    await expect(page.locator(selector)).toBeDisabled();
    await expect(page.locator(selector)).toHaveAttribute("aria-describedby", "read-only-notice");
  }
  await page.getByRole("button", { name: `Open pipeline ${PIPELINE}`, exact: true }).click();
  await expect(page.getByRole("button", { name: "Edit pipeline", exact: true })).toBeDisabled();
  await expect(page.getByTestId("design-surface-transitions")).toBeEnabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("read-only Live and Replay inspect actual runs without admitting any run control", async ({ page, canvas }) => {
  await seed(canvas, "active-read-only", false);
  await seed(canvas, "paused-read-only", true);
  await page.goto(canvas.url);
  await page.getByRole("button", { name: `Inspect Live for ${SAMPLE.assignment}`, exact: true }).click();
  await expect(page.getByTestId("reconcile-now")).toBeDisabled();
  await page.getByLabel("Live run", { exact: true }).selectOption("active-read-only");
  await expect(page.getByTestId("run-pause")).toBeDisabled();
  await expect(page.getByTestId("run-cancel")).toBeDisabled();
  await page.getByLabel("Live run", { exact: true }).selectOption("paused-read-only");
  await expect(page.getByTestId("run-resume")).toBeDisabled();
  await page.getByTestId("mode-replay").click();
  await page.getByLabel("Replay run", { exact: true }).selectOption("paused-read-only");
  await expect(page.getByLabel("Replay position", { exact: true })).toBeVisible();
});

test("a forged write request cannot override host policy and direct editor URLs cannot save or clone", async ({ page, canvas }) => {
  await page.goto(canvas.url);
  const result = await page.evaluate(async () => {
    const response = await fetch("./intent", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "reconcile-now", readOnly: false, access: { mode: "local" }, config_subdir: ".bureau" }) });
    return { status: response.status, body: await response.json() };
  });
  expect([result.status, result.body.ok, result.body.access.mode]).toEqual([403, false, "read-only"]);
  expect(result.body.error).toContain("Managed dispatch belongs to the daemon");
  await page.goto(new URL("editor.html", canvas.url).href);
  await expect(page.getByRole("heading", { name: "Read-only pipeline inspection", exact: true })).toBeVisible();
  await expect(page.locator(".editor-shell")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Save changes", exact: true })).toHaveCount(0);
  await page.getByRole("link", { name: "Return to dashboard", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
});
