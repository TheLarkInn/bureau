import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "../fixtures.mjs";

test.describe("assignment work rules", () => {
  test("shows the filter, approval gate, and branch prefix at rest", async ({ card }) => {
    const chips = card.page.locator(".runtime-value .chip");

    await expect(chips).toHaveText([
      "is:open label:agent-eligible",
      "no approval label",
      "branches: bureau/",
    ]);
  });

  test("edits all runtime fields through one reviewable draft", async ({ card, canvas }) => {
    const path = join(canvas.dir, "assignments", "agent-eligible.yaml");
    const before = await readFile(path, "utf8");
    await card.page.locator(".runtime-value").click();
    await card.page.getByLabel("Work-item filter").fill("is:open label:ready");
    await card.page.getByLabel("Approval label (optional)").fill("approved");
    await card.page.getByLabel("Branch prefix").fill("agent/");
    await card.page.getByRole("button", { name: "Save work rules" }).click();

    await expect(card.page.locator("[data-testid='draft-bar']")).toBeVisible();
    await expect(card.page.locator(".runtime-value")).toContainText("branches: agent/");
    expect(await readFile(path, "utf8")).toBe(before);
  });

  test("Escape abandons local work-rule edits", async ({ card }) => {
    await card.page.locator(".runtime-value").click();
    await card.page.getByLabel("Branch prefix").fill("discarded/");
    await card.page.keyboard.press("Escape");

    await expect(card.page.locator(".runtime-value")).toContainText("branches: bureau/");
  });

  test("collapsing an assignment cannot silently discard an open editor", async ({ card }) => {
    await card.page.locator(".runtime-value").click();
    await card.page.getByLabel("Branch prefix").fill("unsaved/");
    card.page.once("dialog", (dialog) => dialog.dismiss());
    await card.page.locator(".assignment-head").click();

    await expect(card.page.locator(".assignment-runtime-editor")).toBeVisible();
    await expect(card.page.getByLabel("Branch prefix")).toHaveValue("unsaved/");
  });

  test("assignment deletion is reachable and remains a draft until saved", async ({ card, canvas }) => {
    const path = join(canvas.dir, "assignments", "agent-eligible.yaml");
    await card.page.getByRole("button", { name: "Delete" }).click();
    await expect(card.page.locator("[data-testid='preflight']")).toContainText("Nothing references this");
    await card.page.getByRole("button", { name: "Confirm delete" }).click();

    await expect(card.page.locator("[data-testid='draft-bar']")).toBeVisible();
    await expect(card.page.locator(".assignment-card")).toHaveCount(0);
    expect(await readFile(path, "utf8")).toContain("name: agent-eligible");
  });
});
