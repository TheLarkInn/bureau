// Assignment overview shows operational assignment state. Roles and commands
// belong to pipeline steps; the assignment's scaffold-era `role` and `verify`
// fields have no effect during engine execution and must not masquerade as
// runtime controls.

import { test, expect } from "../fixtures.mjs";

test.describe("pipeline owns execution details", () => {
  test("the assignment overview omits scaffold-only role and verify rows", async ({ card }) => {
    const labels = await card.page.locator(".assignment-detail > .detail-row > .detail-label").allTextContents();

    expect(labels).not.toContain("role");
    expect(labels).not.toContain("verify");
    expect(labels).toContain("pipeline");
  });

  test("the assignment links to its actual pipeline", async ({ card }) => {
    await expect(card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" })).toBeVisible();
  });

  test("agent roles are visible on the pipeline steps that actually use them", async ({ card }) => {
    await card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    const nodes = card.page.locator(".flow-card");

    await expect(nodes.filter({ hasText: "implement" })).toContainText("role: implementer");
    await expect(nodes.filter({ hasText: "review" })).toContainText("role: reviewer");
  });

  test("the pipeline view provides an explicit editor action", async ({ card }) => {
    await card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();

    await expect(card.page.getByRole("link", { name: "Edit pipeline" })).toHaveClass(/btn/u);
  });

  test("the pipeline viewer names terminal effects in human language", async ({ card }) => {
    await card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();

    await expect(card.page.locator(".terminal-pill--done")).toContainText("Publish");
    await expect(card.page.locator(".terminal-pill--escalate")).toContainText("Needs human");
    await expect(card.page.locator(".terminal-pill--abort")).toHaveCount(0);
  });

  test("terminal cards are visible and occupy distinct positions", async ({ card }) => {
    await card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    const positions = await card.page.locator(".terminal-pill").evaluateAll((items) =>
      items.map((item) => {
        const box = item.getBoundingClientRect();
        return `${Math.round(box.left)}:${Math.round(box.top)}`;
      }));

    expect(new Set(positions).size).toBe(positions.length);
  });

  test("compact pipeline view keeps the graph and side panel on screen", async ({ card }) => {
    await card.page.setViewportSize({ width: 800, height: 900 });
    await card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();

    await expect(card.page.locator(".pipeline-flow")).toBeVisible();
    await expect(card.page.locator(".side-panel")).toBeVisible();
    expect(await card.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });

  test("the pipeline viewer returns to assignments with consistent copy", async ({ card }) => {
    await card.page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await card.page.getByRole("button", { name: "Assignments" }).click();

    await expect(card.page.getByRole("heading", { name: "Assignments" })).toBeVisible();
  });
});
