// The state lab, checked the way a reviewer uses it.
//
// The lab is a deliverable, not scaffolding: if it stops rendering states or
// stops agreeing with the registry, the matrix is no longer reviewable by a
// human. So the suite opens it, drives it, and asserts that what it reports
// about a state matches what the registry says about that state.

import { STATES } from "../../../web/statelab/registry.mjs";
import { servableInFrame } from "../../../web/statelab/intercept.mjs";
import { VIEWPORTS } from "../../../web/statelab/selectors.mjs";
import { expect, test } from "../matrix-fixtures.mjs";

/** A state the lab can drive itself, interception included. */
const DRIVABLE = STATES.filter((state) => servableInFrame(state.intercept));

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
  // This host serves the bundled sample, so the fixtures land on the payload
  // they were written against and the lab has nothing to warn about. The note
  // exists for a contributor opening the lab against their own `.bureau/`,
  // where the same state id renders different content.
  await expect(page.locator("#base-note")).toBeHidden();
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
  const stage = page.locator("#stage");

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await expect(stage).toHaveJSProperty("offsetWidth", VIEWPORTS.desktop.width);

  await page.locator('[data-viewport="compact"]').click();
  // The stage's own width, not the note beside it. The note is a label this
  // control writes; asserting it would have held with the two lines that
  // actually resize the stage deleted, which is the whole of what the button
  // does — and the render underneath would have stayed desktop-wide while the
  // lab claimed a compact viewport.
  await expect(stage).toHaveJSProperty("offsetWidth", VIEWPORTS.compact.width);
  await expect(page.locator("#viewport-note")).toContainText(`${VIEWPORTS.compact.width}`);
  await page.locator("#detail .expectations").waitFor();
  await expect(page.frameLocator("#stage-frame").locator(".assignment-detail")).toBeVisible();
  expect(errors).toEqual([]);
});

test("a state the lab cannot install blanks the stage rather than showing the last render", async ({ page, host }) => {
  await openLab(page, host);
  const drivable = DRIVABLE.find((state) => state.id.endsWith("card:expanded"));
  const blocked = STATES.find((state) => state.intercept && !servableInFrame(state.intercept));

  // Drive a state the lab can produce first, so there is a real render on the
  // stage for the next selection to inherit. Returning early used to leave it
  // there beside the intercepted state's description — the lab presenting a
  // screen it never produced as that state's render, which is the one thing a
  // review surface may not do.
  await page.locator(".state-item", { hasText: drivable.id }).first().click();
  await expect(page.frameLocator("#stage-frame").locator(".assignment-detail")).toBeVisible();

  await page.locator(".state-item", { hasText: blocked.id }).first().click();
  await expect(page.locator("#detail .note--warn")).toContainText(blocked.intercept);
  await expect(page.locator("#stage-frame")).toHaveAttribute("src", "about:blank");
});

test("the lab renders a save state itself, under the same refusal the suite asserts", async ({ page, host }) => {
  const errors = await openLab(page, host);
  // `fail-intent` is installed inside the frame before the page's modules run,
  // so a refused save is a screen a reviewer can look at rather than a note
  // saying the browser suite has it. This is the claim that a third of the
  // registry stopped being unreachable in the lab.
  const refused = STATES.find((state) => state.intercept === "fail-intent");

  await page.locator(".state-item", { hasText: refused.id }).first().click();
  await page.locator("#detail .expectations").waitFor();

  await expect(page.locator("#stage-frame")).not.toHaveAttribute("src", "about:blank");
  await expect(page.locator("#detail .expectations li.bad")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("the lab explains why an excluded combination is not a state", async ({ page, host }) => {
  await openLab(page, host);
  const rules = page.locator("#constraints details");

  await expect(rules.first()).toContainText("pruned here");
  await rules.first().locator("summary").click();
  await expect(rules.first().locator("pre.example")).toBeVisible();
});

/**
 * The per-rule tallies are order-dependent by construction, so they cannot
 * answer "why is *this* combination not a state?". The picker can, and this is
 * the assertion that keeps it wired to `violations()` rather than to a list of
 * states someone remembered to update.
 */
test("the picker judges any combination a reviewer assembles", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const verdict = page.locator("#picker-verdict");

  await expect(verdict).toHaveAttribute("data-verdict", "reachable");

  await page.locator('#picker select[aria-label="card"]').selectOption("expanded");
  await expect(verdict).toHaveAttribute("data-verdict", "excluded");
  await expect(verdict).toContainText("boot-has-no-regions");
  await expect(verdict.locator("details")).not.toHaveCount(0);

  await page.locator('#picker select[aria-label="card"]').selectOption("n/a");
  await expect(verdict).toHaveAttribute("data-verdict", "reachable");
  expect(errors).toEqual([]);
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
