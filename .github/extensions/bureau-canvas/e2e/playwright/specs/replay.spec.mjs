// Replay in the width the canvas actually gets. The panel is far narrower
// than a desktop window, and the toolbar controls used to render past its
// right edge: present in the DOM, impossible to click. So these assert the
// controls are reachable, not merely that they exist.

import { test, expect, RUN_ID } from "../fixtures.mjs";

async function openReplay(page, canvas) {
  await page.goto(canvas.url);
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
  await page.getByRole("tab", { name: "replay" }).click();
  await expect(page.locator(".run-picker option")).toHaveCount(2);
  await page.selectOption(".run-picker", RUN_ID);
  await page.locator(".replay-timeline").waitFor();
  // The timeline renders as soon as a run is picked, but its range only
  // exists once the event log has been fetched.
  await page.waitForFunction(() => {
    const scrubber = document.querySelector(".replay-scrubber");
    return scrubber && Number(scrubber.max) > Number(scrubber.min);
  });
}

/** Clicks a step card so the log panel follows it. */
async function selectStep(page, name) {
  await page.locator(".flow-card", { has: page.locator(`h2:text-is("${name}")`) })
    .click({ position: { x: 20, y: 12 } });
  await expect(page.locator(".step-log-title")).toHaveText(name);
}

test.describe("replay in a narrow panel", () => {
  test.use({ viewport: { width: 920, height: 900 } });

  test("the timeline is on screen and nothing covers Play", async ({ page, canvas }) => {
    await openReplay(page, canvas);

    const reach = await page.locator(".replay-timeline").evaluate((bar) => {
      const play = [...bar.querySelectorAll("button")].find((button) => button.textContent === "Play");
      const box = play.getBoundingClientRect();
      const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
      const shell = document.querySelector(".view-shell--pipeline");
      return {
        onScreen: box.right <= window.innerWidth && box.left >= 0,
        clickable: Boolean(hit && (hit === play || play.contains(hit))),
        fits: shell.scrollWidth <= shell.clientWidth,
      };
    });

    expect(reach).toEqual({ onScreen: true, clickable: true, fits: true });
  });

  test("scrubbing the run marks each step with the outcome it reached", async ({ page, canvas }) => {
    await openReplay(page, canvas);
    const scrubber = page.locator(".replay-scrubber");

    // At the start nothing has run; at the end all three steps have.
    const atStart = await page.locator(".flow-card.overlay-pending").count();
    await scrubber.fill(await scrubber.getAttribute("max"));
    await expect(page.locator(".flow-card.overlay-success")).toHaveCount(3);
    expect(atStart).toBe(3);
  });

  test("an agent step's log is blocks, not the CLI's drawing characters", async ({ page, canvas }) => {
    await openReplay(page, canvas);
    await page.locator(".replay-scrubber").fill(await page.locator(".replay-scrubber").getAttribute("max"));
    await selectStep(page, "implement");

    const tool = page.locator(".log-tool");
    await expect(tool.locator(".log-tool-name")).toHaveText("Read DESIGN.md");
    await expect(tool.locator(".log-tool-result")).toHaveText("L1:120 (120 lines read)");
    await expect(page.locator(".log-note")).toHaveText("I read the contract before editing.");
  });

  test("the log never shows the glyphs it parsed away", async ({ page, canvas }) => {
    await openReplay(page, canvas);
    await page.locator(".replay-scrubber").fill(await page.locator(".replay-scrubber").getAttribute("max"));
    await selectStep(page, "implement");

    const shown = await page.locator(".step-log").innerText();
    expect([shown.includes("●"), shown.includes("└"), shown.includes("│")]).toEqual([false, false, false]);
  });

  test("a deterministic step's log is its contract, read out", async ({ page, canvas }) => {
    await openReplay(page, canvas);
    await page.locator(".replay-scrubber").fill(await page.locator(".replay-scrubber").getAttribute("max"));
    await selectStep(page, "verify");

    const result = page.locator(".log-result");
    await expect(result.locator(".outcome-pill")).toHaveText("success");
    await expect(result.locator(".log-result-message")).toHaveText("suite passed");
    await expect(result.locator(".log-outputs")).toHaveText("tests42");
  });
});
