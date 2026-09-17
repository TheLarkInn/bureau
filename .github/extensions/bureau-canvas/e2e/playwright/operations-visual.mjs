import { OPERATIONS_VISUAL_NOW, operationsVisualFixture } from "../../test/support/operations-visual.mjs";
import { expect, pageAdapter } from "./matrix-fixtures.mjs";

export async function enterOperationsVisual(page, host, variant) {
  const { state, listing } = operationsVisualFixture(host.base, variant);
  await page.clock.setFixedTime(OPERATIONS_VISUAL_NOW);
  await page.route("**/runs", (route) => route.fulfill({ json: listing }));
  const adapter = pageAdapter(page, host);
  await adapter.goto("config");
  await adapter.publish(state);
  await expect(page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Operations", exact: true })).toHaveAttribute("aria-current", "page");
  await expect(page.getByText("Current adopted source not observed", { exact: true })).toBeVisible();
  if (variant === "unavailable") {
    await expect(page.locator(".ops-count")).toHaveText(["Unknown", "Unknown", "Unknown", "Unknown"]);
    await expect(page.locator("#read-only-notice")).toBeVisible();
    await expect(page.getByLabel("Configuration validation errors")).toContainText("assignments/work.yaml: invalid YAML");
    await expect(page.getByText("No current run evidence to display.", { exact: true })).toBeVisible();
  } else {
    await expect(page.locator(".ops-count")).toHaveText(["4", "1", "1", "1"]);
    await expect(page.locator(".ops-run")).toHaveCount(5);
    await expect(page.locator('[data-run-id="corrupt-log"]')).toContainText("Invalid JSON");
    await expect(page.locator(".ops-run-facts dd:nth-of-type(2)")).toHaveText(Array(5).fill("Unknown"));
    await page.getByText("Recorded sources: 1 distinct revision", { exact: true }).click();
    await page.getByText("Configured safeguards", { exact: true }).click();
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}
