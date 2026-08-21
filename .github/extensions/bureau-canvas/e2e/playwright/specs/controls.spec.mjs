// Cross-surface control inventory. Every visible control needs a name a user
// can understand; progressive forms should not occupy the page before asked.

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "../fixtures.mjs";

async function unnamedControls(page) {
  return page.locator("button, input, select, textarea, a[href], summary").evaluateAll((controls) =>
    controls
      .filter((control) => {
        const style = getComputedStyle(control);
        const visible = style.display !== "none" && style.visibility !== "hidden"
          && control.getBoundingClientRect().width > 0 && control.getBoundingClientRect().height > 0;
        if (!visible) return false;
        const labelled = control.getAttribute("aria-label")
          || control.getAttribute("title")
          || control.textContent?.trim()
          || (control.id && document.querySelector(`label[for="${CSS.escape(control.id)}"]`)?.textContent?.trim())
          || control.closest("label")?.textContent?.trim();
        return !labelled;
      })
      .map((control) => `${control.tagName.toLowerCase()}.${control.className}`),
  );
}

test.describe("control system", () => {
  test("every visible config control has a human-readable name", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    expect(await unnamedControls(page)).toEqual([]);
  });

  test("every visible editor control has a human-readable name", async ({ editor }) => {
    expect(await unnamedControls(editor.page)).toEqual([]);
  });

  test("the expanded global create form has no unnamed controls", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();

    expect(await unnamedControls(page)).toEqual([]);
  });

  test("Escape closes global creation and returns focus to its trigger", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Name").press("Escape");

    await expect(page.locator("[data-testid='create-bar']")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "+ New pipeline or role" })).toBeFocused();
  });

  test("the expanded work-source form has no unnamed controls", async ({ card }) => {
    await card.page.locator(".ws-value").click();

    expect(await unnamedControls(card.page)).toEqual([]);
  });

  test("the ranked repos editor and resolver have no unnamed controls", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    expect(await unnamedControls(card.page)).toEqual([]);

    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await card.page.getByLabel("Repository URL").fill("https://github.com/microsoft/rushstack");
    expect(await unnamedControls(card.page)).toEqual([]);
  });

  test("the limits editor has no unnamed controls", async ({ card }) => {
    await card.page.locator(".limits-value").click();

    expect(await unnamedControls(card.page)).toEqual([]);
  });

  test("every step-inspector variant has no unnamed controls", async ({ editor }) => {
    for (const reference of ["implement", "verify"]) {
      await editor.page.locator(`[data-ref="${reference}"]`).click();
      expect(await unnamedControls(editor.page)).toEqual([]);
    }
    for (const kind of ["decision", "concurrent"]) {
      await editor.page.getByLabel("New step kind").selectOption(kind);
      await editor.page.getByRole("button", { name: "+ Add step" }).click();
      expect(await unnamedControls(editor.page)).toEqual([]);
    }
  });

  test("global creation is progressive rather than an unexplained open form", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    await expect(page.getByRole("button", { name: "+ New pipeline or role" })).toBeVisible();
    await expect(page.getByLabel("Kind")).toHaveCount(0);
    await expect(page.getByLabel("Name")).toHaveCount(0);
  });

  test("the global creator only exposes config kinds it can scaffold safely", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();

    await expect(page.getByLabel("Kind").locator("option"))
      .toHaveText(["pipeline", "role"]);
  });

  test("the create button names the selected kind", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("pipeline");
    await page.getByLabel("Name").fill("browser-pipeline");

    await expect(page.getByRole("button", { name: "Create pipeline" })).toBeEnabled();
  });

  test("creating through the UI produces a reviewable draft, not an immediate write", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("role");
    await page.getByLabel("Name").fill("browser-role");
    await page.getByRole("button", { name: "Create role" }).click();

    await expect(page.locator("[data-testid='draft-bar']")).toBeVisible();
    await expect(readFile(join(canvas.dir, "roles", "browser-role.yaml"), "utf8")).rejects.toThrow();
  });

  test("discard removes a pending create without touching disk", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("role");
    await page.getByLabel("Name").fill("discarded-role");
    await page.getByRole("button", { name: "Create role" }).click();
    await page.getByRole("button", { name: "Discard" }).click();

    await expect(page.locator("[data-testid='draft-bar']")).toHaveCount(0);
    await expect(readFile(join(canvas.dir, "roles", "discarded-role.yaml"), "utf8")).rejects.toThrow();
  });

  test("save applies the planned file to the scratch config", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("role");
    await page.getByLabel("Name").fill("saved-role");
    await page.getByRole("button", { name: "Create role" }).click();
    await page.getByRole("button", { name: "Save" }).click();

    await expect(page.locator("[data-testid='draft-bar']")).toHaveCount(0);
    await expect.poll(async () => readFile(join(canvas.dir, "roles", "saved-role.yaml"), "utf8"))
      .toContain("name: saved-role");
  });

  test("duplicate creation stays open and explains why it was refused", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Name").fill("agent-eligible-pipeline");
    await page.getByRole("button", { name: "Create pipeline" }).click();

    await expect(page.locator("[data-testid='create-bar']")).toBeVisible();
    await expect(page.getByRole("alert")).toContainText("already exists");
    await expect(page.locator("[data-testid='draft-bar']")).toHaveCount(0);
  });
});
