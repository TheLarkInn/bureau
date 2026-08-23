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
});
