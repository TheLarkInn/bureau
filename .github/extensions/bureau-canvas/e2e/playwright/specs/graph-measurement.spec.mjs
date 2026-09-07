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
    await page.getByRole("tab", { name: "Graph" }).click();

    await expect(page.locator('[data-ref="verify"]')).toBeVisible();
  });

  test("the pipeline viewer draws its steps", async ({ page, canvas }) => {
    await withholdNodeMeasurements(page);
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("tab", { name: "Graph" }).click();

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
});
