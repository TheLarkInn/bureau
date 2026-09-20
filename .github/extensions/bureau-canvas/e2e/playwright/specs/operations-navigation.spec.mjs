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
  await page.getByRole("button", { name: "Runs per day limit", exact: true }).click();
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
  const path = join(canvas.dir, "pipelines", `${PIPELINE}.yaml`);
  const rolePath = join(canvas.dir, "roles", "navigation-review.yaml");
  const original = await readFile(path, "utf8");
  await observeStateUpdates(page);
  await page.goto(canvas.url);
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await page.getByRole("button", { name: "+ New pipeline or role", exact: true }).click();
  await page.getByLabel("Kind", { exact: true }).selectOption("role");
  await page.getByLabel("Name", { exact: true }).fill("navigation-review");
  const creating = page.waitForResponse((response) => response.url().endsWith("/intent")
    && response.request().postDataJSON()?.kind === "create");
  await page.getByRole("button", { name: "Create role", exact: true }).click();
  const created = await (await creating).json();
  expect(created.ok, created.error).toBe(true);
  expect(created.state.plan).toEqual({ writes: [rolePath], removals: [] });
  await expect(readFile(rolePath, "utf8")).rejects.toMatchObject({ code: "ENOENT" });
  await expect(page.getByTestId("draft-save")).toBeVisible();
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: `Open pipeline ${PIPELINE}`, exact: true }).click();
  await page.getByRole("link", { name: "Edit pipeline", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Pipeline editor", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Pipeline", exact: true })).toHaveAttribute("aria-pressed", "true");
  const graph = page.getByRole("tablist", { name: "Pipeline editor view", exact: true })
    .getByRole("tab", { name: "graph", exact: true });
  await graph.click();
  await page.locator('[data-ref="verify"]').click();
  const panel = page.locator(".editor-panel");
  await expect(panel.getByRole("heading", { name: "verify", exact: true })).toBeVisible();
  // Label text includes the textarea's contents; its accessible textbox name is exactly "run".
  const command = panel.getByRole("textbox", { name: "run", exact: true });
  const save = page.getByRole("button", { name: "Save changes", exact: true });
  await expect(command).toHaveValue("cargo test --offline");
  await expect(save).toBeDisabled();
  await command.fill("node --version");
  await expect(save).toBeEnabled();
  expect(await readFile(path, "utf8")).toBe(original);
  const savedPlan = await update(page, { kind: "save-plan" });
  expect(savedPlan.state.navigation).toMatchObject({ view: "pipeline", pipeline: PIPELINE, mode: "design" });
  expect(savedPlan.state.plan).toBeNull();
  expect(await readFile(rolePath, "utf8")).toContain("navigation-review");
  const refreshed = await update(page, { kind: "operations", refresh: true });
  expect(refreshed.state.navigation).toEqual(savedPlan.state.navigation);
  await expect(graph).toHaveAttribute("aria-selected", "true");
  await expect(command).toHaveValue("node --version");
  await expect(save).toBeEnabled();
  expect(await readFile(path, "utf8")).toBe(original);

  // The standard offline fixture stubs CLI validation, not HTTP or file writes.
  const before = await stateVersion(page);
  const reply = page.waitForResponse((response) => response.url().endsWith("/intent")
    && response.request().postDataJSON()?.kind === "save-pipeline");
  await save.click();
  const saved = await (await reply).json();
  expect(saved.ok, saved.error).toBe(true);
  expect(saved.state.navigation).toEqual(savedPlan.state.navigation);
  expect(saved.state.plan).toBeNull();
  await receivedUpdate(page, before);
  await expect(page.locator(".editor-status")).toHaveText("saved");
  await expect(save).toBeDisabled();
  const persisted = await readFile(path, "utf8");
  expect(persisted).toContain("node --version");
  await command.fill("node --help");
  await expect(save).toBeEnabled();
  const final = await update(page, { kind: "operations", refresh: true });
  expect(final.state.navigation).toEqual(savedPlan.state.navigation);
  await expect(command).toHaveValue("node --help");
  await expect(graph).toHaveAttribute("aria-selected", "true");
  await expect(save).toBeEnabled();
  expect(await readFile(path, "utf8")).toBe(persisted);
});
