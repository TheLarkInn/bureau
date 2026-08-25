// A graph that never draws is the worst failure this canvas has: the nodes are
// in the DOM, so nothing errors, and the surface simply says "this pipeline has
// no steps" by drawing nothing.
//
// React Flow measures each node once, from a ResizeObserver delivery, and its
// `updateNodeInternals` returns without applying anything when the viewport
// element is not queryable at that moment. A node's box never changes again, so
// the observer never fires a second time: a delivery that loses that race
// leaves the graph blank permanently. It showed up here as editor specs failing
// under load with a node that resolved but was never visible.
//
// These specs withhold node measurements from the observer entirely — harsher
// than the real race, which only loses the first delivery — and require every
// React Flow surface to draw anyway.

import { test, expect } from "../fixtures.mjs";

/**
 * Drops every ResizeObserver delivery that carries a React Flow node, so the
 * only way a node can be measured is the repair path.
 */
async function withholdNodeMeasurements(page) {
  await page.addInitScript(() => {
    const Native = window.ResizeObserver;
    window.ResizeObserver = class extends Native {
      constructor(callback) {
        super((entries, observer) => {
          if (entries.some((entry) => entry.target?.hasAttribute?.("data-id"))) {
            return;
          }
          callback(entries, observer);
        });
      }
    };
  });
}

test.describe("graph measurement", () => {
  test("the pipeline editor draws its steps", async ({ page, canvas }) => {
    await withholdNodeMeasurements(page);
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("link", { name: "Edit" }).click();

    await expect(page.locator('[data-ref="verify"]')).toBeVisible();
  });

  test("the pipeline viewer draws its steps", async ({ page, canvas }) => {
    await withholdNodeMeasurements(page);
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();

    await expect(page.locator(".pipeline-flow .react-flow__node").first()).toBeVisible();
  });

  test("the relation graph draws its cards", async ({ page, canvas }) => {
    await withholdNodeMeasurements(page);
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("link", { name: "Edit" }).click();
    await page.getByRole("button", { name: "Relations" }).click();

    await expect(page.locator(".relation-flow .react-flow__node").first()).toBeVisible();
  });

  /**
   * The repair budget belongs to the moment a graph is on screen, not to the
   * moment it mounts.
   *
   * The editor mounts its Relations pane `hidden` behind the Pipeline tab, so
   * that graph exists — and can spend repairs — long before anyone can see it.
   * This waits out the whole budget (5 x 80ms, with room to spare) with the tab
   * still shut, so every repair the guard is willing to spend on a pane nobody
   * is looking at is already gone by the time the tab opens. A graph that draws
   * anyway is one whose budget was held until it could land.
   *
   * The guard used to read the store's `width`/`height` for this, which cannot
   * answer it: React Flow records a pane measuring zero as 500x500, so a hidden
   * pane read as measurable from mount and burned the budget behind the tab.
   */
  test("a graph revealed after its budget would have burned still draws", async ({ page, canvas }) => {
    await withholdNodeMeasurements(page);
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("link", { name: "Edit" }).click();
    await expect(page.locator('[data-ref="verify"]')).toBeVisible();

    await page.waitForTimeout(1000);
    await page.getByRole("button", { name: "Relations" }).click();

    await expect(page.locator(".relation-flow .react-flow__node").first()).toBeVisible();
  });
});
