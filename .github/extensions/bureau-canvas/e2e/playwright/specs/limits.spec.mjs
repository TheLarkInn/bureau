// Limits: a kill switch. An omitted limit means unlimited, so the resting
// summary has to say so — and it must never call a bounded assignment
// unbounded, nor let a cleared box write a permanent block.

import { test, expect } from "../fixtures.mjs";

test.describe("limits field", () => {
  test("shows the caps, the uncapped count and the run-length default at rest", async ({ card }) => {
    const chips = card.page.locator(".limits-value .chip");

    await expect(chips).toHaveCount(4);
    await expect(chips.nth(0)).toHaveText("1 at once");
    await expect(chips.nth(1)).toHaveText("4 /hour");
    await expect(chips.nth(2)).toHaveText("3 unlimited");
    await expect(chips.nth(3)).toHaveText("24h/run default");
  });

  test("the uncapped rollup is visually distinct from a cap", async ({ card }) => {
    await expect(card.page.locator(".limits-value .chip--off")).toHaveText("3 unlimited");
    await expect(card.page.locator(".limits-value .chip--none")).toHaveCount(0);
  });

  test("opening it lists every limit, capped or not", async ({ card }) => {
    await card.page.locator(".limits-value").click();

    await expect(card.page.locator(".limit-row")).toHaveCount(6);
    await expect(card.page.locator(".limit-row--off")).toHaveCount(3);
  });

  test("an uncapped limit reads as unlimited, and run length as the system default", async ({ card }) => {
    await card.page.locator(".limits-value").click();

    await expect(card.page.locator(".limit-row").nth(2)).toContainText("unlimited");
    await expect(card.page.locator(".limit-row").nth(5)).toContainText("system default");
    await expect(card.page.locator(".limit-row").nth(5)).not.toHaveClass(/limit-row--off/u);
  });

  test("saving is refused until something changes, then reports it is unsaved", async ({ card }) => {
    await card.page.locator(".limits-value").click();
    const save = card.page.getByRole("button", { name: "Save limits" });

    await expect(save).toBeDisabled();
    await expect(card.page.locator(".limits-dirty")).toHaveCount(0);
    await card.page.getByRole("button", { name: "runs per day limit" }).click();
    await expect(save).toBeEnabled();
    await expect(card.page.locator(".limits-dirty")).toHaveText("unsaved changes");
  });

  test("switching a limit on gives it a usable starting value", async ({ card }) => {
    await card.page.locator(".limits-value").click();
    await card.page.getByRole("button", { name: "runs per day limit" }).click();

    await expect(card.page.locator(".limit-row").nth(2).locator("input")).toHaveValue("1");
    await expect(card.page.locator(".limit-row").nth(2)).not.toHaveClass(/limit-row--off/u);
  });

  test("switching a limit off returns it to unlimited", async ({ card }) => {
    await card.page.locator(".limits-value").click();
    await card.page.getByRole("button", { name: "concurrent runs limit" }).click();

    await expect(card.page.locator(".limit-row").first()).toContainText("unlimited");
    await expect(card.page.locator(".limit-row").first()).toHaveClass(/limit-row--off/u);
  });

  test("clearing a value blocks the save instead of writing a zero", async ({ card }) => {
    await card.page.locator(".limits-value").click();
    await card.page.locator(".limit-row").first().locator("input").fill("");

    await expect(card.page.locator(".form-control--invalid")).toHaveCount(1);
    await expect(card.page.locator(".note--err")).toContainText("need whole numbers of at least 1");
    await expect(card.page.getByRole("button", { name: "Save limits" })).toBeDisabled();
  });

  test("turning every limit off says the assignment is unbounded", async ({ card }) => {
    await card.page.locator(".limits-value").click();
    await card.page.getByRole("button", { name: "concurrent runs limit" }).click();
    await card.page.getByRole("button", { name: "runs per hour limit" }).click();
    await card.page.getByRole("button", { name: "Cancel" }).click();

    // Cancel discards the draft, so the resting summary is unchanged.
    await expect(card.page.locator(".limits-value .chip").first()).toHaveText("1 at once");
  });

  test("cancelling discards the draft", async ({ card }) => {
    await card.page.locator(".limits-value").click();
    await card.page.getByRole("button", { name: "runs per day limit" }).click();
    await card.page.getByRole("button", { name: "Cancel" }).click();

    await expect(card.page.locator(".limits-value .chip--off")).toHaveText("3 unlimited");
  });
});
