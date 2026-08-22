// The state lab, checked the way a reviewer uses it.
//
// The lab is a deliverable, not scaffolding: if it stops rendering states or
// stops agreeing with the registry, the matrix is no longer reviewable by a
// human. So the suite opens it, drives it, and asserts that what it reports
// about a state matches what the registry says about that state.

import { STATES } from "../../../web/statelab/registry.mjs";
import { expect, test } from "../matrix-fixtures.mjs";

/** A state the lab can drive itself: no request interception involved. */
const DRIVABLE = STATES.filter((state) => !state.intercept);

async function openLab(page, host) {
  const errors = [];
  page.on("pageerror", (error) => errors.push(`pageerror: ${error}`));
  page.on("console", (message) => {
    if (message.type() === "error") {
      errors.push(`console: ${message.text()}`);
    }
  });
  await page.goto(new URL("statelab.html", host.url).href);
  await page.locator(".state-item").first().waitFor();
  return errors;
}

test("the lab lists every state in the registry and boots clean", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const listed = await page.locator(".state-item .state-id").allTextContents();

  expect(errors).toEqual([]);
  expect(listed.sort()).toEqual(STATES.map((state) => state.id).sort());
});

test("the lab reports the registry's own counts", async ({ page, host }) => {
  await openLab(page, host);
  const metrics = await page.locator("#summary .metric").allTextContents();
  const joined = metrics.join(" ");

  expect(joined).toContain(String(STATES.length));
  expect(await page.locator("#constraints details").count()).toBeGreaterThan(0);
});

test("selecting a state drives the production page and passes its own checks", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const target = DRIVABLE.find((state) => state.id.includes("field:limits") && state.id.includes("fieldState:dirty"));

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await page.locator("#detail h2", { hasText: target.id }).waitFor();
  await page.locator("#detail .expectations").waitFor();
  await expect(page.locator("#detail .expectations li.bad")).toHaveCount(0);
  await expect(page.locator("#detail .expectations li.ok").first()).toBeVisible();
  // The rendered surface is the real page, inside the lab's frame.
  await expect(page.frameLocator("#stage-frame").locator(".limits-editor")).toBeVisible();
  expect(errors).toEqual([]);
});

test("a replay state with a run selected passes its checks in the lab too", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const target = DRIVABLE.find((state) => state.id.includes("mode:replay+run:finished"));

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await page.locator("#detail .expectations").waitFor();
  await expect(page.locator("#detail .expectations li.bad")).toHaveCount(0);
  await expect(page.frameLocator("#stage-frame").locator(".replay-timeline")).toBeVisible();
  expect(errors).toEqual([]);
});

test("the compact control resizes the stage and re-runs the entry path", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const target = DRIVABLE.find((state) => state.id.endsWith("card:expanded"));

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await page.locator('[data-viewport="compact"]').click();
  await expect(page.locator("#viewport-note")).toContainText("760");
  await page.locator("#detail .expectations").waitFor();
  await expect(page.frameLocator("#stage-frame").locator(".assignment-detail")).toBeVisible();
  expect(errors).toEqual([]);
});

test("a state that needs request interception says so rather than failing quietly", async ({ page, host }) => {
  await openLab(page, host);
  const intercepted = STATES.find((state) => state.intercept);

  await page.locator(".state-item", { hasText: intercepted.id }).first().click();
  await expect(page.locator("#detail .note--warn")).toContainText(intercepted.intercept);
});

test("the lab explains why an excluded combination is not a state", async ({ page, host }) => {
  await openLab(page, host);
  const rules = page.locator("#constraints details");

  await expect(rules.first()).toContainText("excluded");
  await rules.first().locator("summary").click();
  await expect(rules.first().locator("pre.example")).toBeVisible();
});

test("the lab links the transition DAG in both directions", async ({ page, host }) => {
  await openLab(page, host);
  const child = DRIVABLE.find((state) => state.id.includes("field:limits+fieldState:dirty"));

  await page.locator(".state-item", { hasText: child.id }).first().click();
  await page.locator("#detail .expectations").waitFor();
  const inbound = page.locator("#detail .edges .linkish").first();
  await expect(inbound).toContainText("←");
  await inbound.click();
  await expect(page.locator("#detail h2")).not.toHaveText(child.id);
});
