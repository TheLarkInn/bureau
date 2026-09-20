import { test, expect, screenshot } from "./fixture.mjs";

test("public page loads local assets and names its boundaries", async ({ page }) => {
  await expect(page).toHaveTitle("Bureau - agent work, with operating discipline");
  await expect(page.getByRole("main")).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
  await expect(page.getByText("Illustrative sample")).toBeVisible();
  await expect(page.getByText("This public site is not that dashboard.")).toBeVisible();
  await expect(page.locator("form, input, iframe")).toHaveCount(0);
  expect(await page.evaluate(() => performance.getEntriesByType("resource")
    .every(({ name }) => new URL(name).origin === location.origin))).toBe(true);
});

test("sample outcome controls are keyboard accessible and do not execute work", async ({ page }) => {
  await page.keyboard.press("Tab");
  await expect(page.getByRole("link", { name: "Skip to content" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("main")).toBeFocused();
  const states = [
    ["Needs changes", "Verification failed. Publication waits."],
    ["Budget reached", "No headroom. No new run."],
    ["Passes", "Ready for human review"],
  ];
  for (const [name, result] of states) {
    const button = page.getByRole("button", { name, exact: true });
    await button.focus();
    await page.keyboard.press("Enter");
    await expect(button).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator("[data-result-title]")).toHaveText(result);
  }
});

test("all ten scenario links remain navigable through filtering", async ({ page }) => {
  for (const [name, count] of [["Plan & understand", 4], ["Build & verify", 3], ["Run & maintain", 3], ["All scenarios", 10]]) {
    const button = page.getByRole("button", { name, exact: true });
    await button.focus();
    await page.keyboard.press("Space");
    await expect(page.locator(".scenario:visible")).toHaveCount(count);
    await expect(page.locator(".scenario-count")).toHaveText(`${count} scenarios`);
  }
  for (const link of await page.locator(".scenario a").all()) {
    await expect(link).toHaveAttribute("href", /^https:\/\/github\.com\/TheLarkInn\/bureau\/blob\/main\/docs\/scenarios\.md#[a-z-]+$/u);
  }
});

test("release architectures show actual asset conventions without a version claim", async ({ page }) => {
  for (const [name, target] of [["ARM64", "aarch64"], ["x86-64", "x86_64"]]) {
    await page.getByRole("button", { name, exact: true }).click();
    await expect(page.locator("[data-asset]")).toHaveText(`bureau-v<version>-${target}-unknown-linux-musl.tar.gz`);
  }
  await expect(page.getByRole("link", { name: "Get the Linux release" }))
    .toHaveAttribute("href", "https://github.com/TheLarkInn/bureau/releases/latest");
});

test("320px, mobile, tablet, and desktop remain contained", async ({ page }, testInfo) => {
  for (const width of [320, 375, 768, 1440]) {
    await page.setViewportSize({ width, height: 960 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await page.locator('[data-outcome="budget"]').click();
    await expect(page.getByText("No headroom. No new run.")).toBeVisible();
    await page.locator('[data-outcome="pass"]').click();
    if (width !== 320) await screenshot(page, testInfo.outputPath(`viewport-${width}.png`), true);
    if (testInfo.project.name === "repository-path" && [375, 1440].includes(width)) {
      await page.evaluate(() => window.scrollTo(0, 0));
      await screenshot(page, testInfo.outputPath(`top-${width}.png`));
    }
  }
});

test("reduced motion and narrow reflow keep content reachable", async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 480 });
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollBehavior)).toBe("auto");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await page.getByRole("link", { name: "Start with Bureau" }).click();
  await expect(page.getByRole("heading", { name: "Set up your first assignment." })).toBeInViewport();
});

test("the content and navigation work without JavaScript", async ({ browser, site }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 375, height: 812 } });
  try {
    const page = await context.newPage();
    await page.goto(site.url);
    await expect(page.locator(".scenario:visible")).toHaveCount(10);
    await expect(page.locator("[data-enhance]:visible")).toHaveCount(0);
    await page.getByRole("link", { name: "Start with Bureau" }).click();
    await expect(page.getByRole("heading", { name: "Set up your first assignment." })).toBeInViewport();
  } finally { await context.close(); }
});

test("unknown pages return a real 404 with a base-correct home link", async ({ page, site }) => {
  const response = await page.goto(`${site.url}missing`);
  expect(response.status()).toBe(404);
  await page.getByRole("link", { name: "Return to Bureau" }).click();
  await expect(page).toHaveURL(site.url);
});
