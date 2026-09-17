import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "../fixtures.mjs";

test.use({ entryView: "operations" });
const PIPELINE = "agent-eligible-pipeline";

async function observeStateUpdates(page) {
  await page.addInitScript(() => {
    window.__navigationStateEvents = 0;
    const Native = window.EventSource;
    window.EventSource = class extends Native {
      constructor(...args) {
        super(...args);
        this.addEventListener("state", () => {
          requestAnimationFrame(() => { window.__navigationStateEvents += 1; });
        });
      }
    };
  });
}

async function stateVersion(page) {
  await page.waitForFunction(() => window.__navigationStateEvents > 0);
  return page.evaluate(() => window.__navigationStateEvents);
}

async function receivedUpdate(page, before) {
  await page.waitForFunction((version) => window.__navigationStateEvents > version, before);
}

async function update(page, body) {
  const before = await stateVersion(page);
  const result = await page.evaluate(async (input) => (await fetch("./intent", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(input),
  })).json(), body);
  expect(result.ok, result.error).toBe(true);
  await receivedUpdate(page, before);
  return result;
}

test("unconfirmed deletion and authoring refresh preserve a real Configuration field draft", async ({ page, canvas }) => {
  await observeStateUpdates(page);
  await page.goto(canvas.url);
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await page.locator(".assignment-head").first().click();
  const path = join(canvas.dir, "assignments", "agent-eligible.yaml");
  const original = await readFile(path, "utf8");
  await page.locator(".limits-value").click();
  await page.getByRole("button", { name: "runs per day limit", exact: true }).click();
  await page.getByTestId("delete-start").first().click();
  await expect(page.getByTestId("preflight")).toContainText("Nothing references this");
  await update(page, { kind: "operations", refresh: true });
  await expect(page.getByTestId("preflight")).toBeVisible();
  await expect(page.locator(".limits-editor")).toHaveAttribute("data-dirty", "true");
  await page.getByTestId("delete-cancel").click();
  await expect(page.getByTestId("preflight")).toHaveCount(0);
  await expect(page.locator(".limits-editor")).toHaveAttribute("data-dirty", "true");
  await expect(page.getByRole("button", { name: "Configuration", exact: true })).toHaveAttribute("aria-current", "page");
  expect(await readFile(path, "utf8")).toBe(original);
});

test("genuine plan and pipeline saves preserve editor navigation and subsequent drafts", async ({ page, canvas }) => {
  await observeStateUpdates(page);
  await page.goto(canvas.url);
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await page.getByRole("button", { name: "+ New pipeline or role", exact: true }).click();
  await page.getByLabel("Kind", { exact: true }).selectOption("role");
  await page.getByLabel("Name", { exact: true }).fill("navigation-review");
  await page.getByRole("button", { name: "Create role", exact: true }).click();
  await expect(page.getByTestId("draft-save")).toBeVisible();
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: `Open pipeline ${PIPELINE}`, exact: true }).click();
  await page.getByRole("link", { name: "Edit pipeline", exact: true }).click();
  await page.getByRole("tab", { name: "Graph", exact: true }).click();
  await page.locator('[data-ref="verify"]').click();
  const command = page.getByLabel("run", { exact: true });
  await command.fill("node --version");
  const savedPlan = await update(page, { kind: "save-plan" });
  expect(savedPlan.state.navigation).toMatchObject({ view: "pipeline", pipeline: PIPELINE });
  expect(savedPlan.state.plan).toBeNull();
  expect(await readFile(join(canvas.dir, "roles", "navigation-review.yaml"), "utf8")).toContain("navigation-review");
  await update(page, { kind: "operations", refresh: true });
  await expect(page.getByRole("tab", { name: "Graph", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(command).toHaveValue("node --version");
  await expect(page.getByRole("button", { name: "Save changes", exact: true })).toBeEnabled();

  // The standard offline fixture stubs CLI validation, not HTTP or file writes.
  const before = await stateVersion(page);
  const reply = page.waitForResponse((response) => response.url().endsWith("/intent")
    && response.request().postDataJSON()?.kind === "save-pipeline");
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  expect((await (await reply).json()).ok).toBe(true);
  await receivedUpdate(page, before);
  await expect(page.locator(".editor-status")).toHaveText("saved");
  const path = join(canvas.dir, "pipelines", `${PIPELINE}.yaml`);
  expect(await readFile(path, "utf8")).toContain("node --version");
  await command.fill("node --help");
  await update(page, { kind: "operations", refresh: true });
  await expect(command).toHaveValue("node --help");
  await expect(page.getByRole("tab", { name: "Graph", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("button", { name: "Save changes", exact: true })).toBeEnabled();
  expect(await readFile(path, "utf8")).toContain("node --version");
});
