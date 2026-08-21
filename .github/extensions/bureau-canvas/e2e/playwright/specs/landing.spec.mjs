// The landing: assignments are the top level, and the relation graph that
// used to be the whole view is secondary.

import { test, expect } from "../fixtures.mjs";

test.describe("assignment landing", () => {
  test("global creation is a single labelled action at rest", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    await expect(page.getByRole("button", { name: "+ New pipeline or role" })).toBeVisible();
    await expect(page.locator("[data-testid='create-bar']")).toHaveCount(0);
  });

  test("opening creation reveals a labelled compact form with pipeline first", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();

    await expect(page.locator("[data-testid='create-bar']")).toBeVisible();
    await expect(page.getByLabel("Kind")).toHaveValue("pipeline");
    await expect(page.getByLabel("Name")).toBeFocused();
    await expect(page.getByRole("button", { name: "Create pipeline" })).toBeDisabled();
  });

  test("the create form can be dismissed without changing config", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Name").fill("temporary");
    await page.getByRole("button", { name: "Cancel" }).click();

    await expect(page.locator("[data-testid='create-bar']")).toHaveCount(0);
    await expect(page.locator(".assignment-card")).toHaveCount(1);
  });

  test("lists one card per assignment, collapsed", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    await expect(page.locator(".assignment-card")).toHaveCount(1);
    await expect(page.locator(".assignment-detail")).toHaveCount(0);
  });

  test("a collapsed card summarises its source, repo count and limit count", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    await expect(page.locator(".assignment-glance")).toHaveText("TheLarkInn/bureau · agent-eligible-pipeline · 1 repo · 2 limits");
  });

  test("the header pluralizes every config count correctly", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    await expect(page.locator(".app-header .summary"))
      .toHaveText("1 assignment · 2 roles · 1 repo · 1 pipeline");
  });

  test("clicking a card expands it in place and marks it expanded", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    const head = page.locator(".assignment-head").first();

    await expect(head).toHaveAttribute("aria-expanded", "false");
    await head.click();
    await expect(head).toHaveAttribute("aria-expanded", "true");
    await expect(page.locator(".assignment-detail")).toBeVisible();
  });

  test("the relation graph starts collapsed", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    // `<details>` hides rather than unmounts, so the cards exist but must not
    // be visible until the section is opened.
    await expect(page.locator(".relation-section")).not.toHaveJSProperty("open", true);
    await expect(page.locator(".relation-section .config-flow")).toBeHidden();
  });

  test("opening the relation graph renders the config cards", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.locator(".relation-section summary").click();

    await expect(page.locator(".relation-section .relation-card--assignment")).toHaveCount(1);
    await expect(page.locator(".relation-section .relation-card--role")).toHaveCount(2);
    await expect(page.locator(".relation-section").getByRole("button", { name: "Delete" })).toHaveCount(0);
  });

  test("expanding a card raises no console or page errors", async ({ card }) => {
    await expect(card.page.locator(".assignment-detail")).toBeVisible();

    expect(card.errors).toEqual([]);
  });
});
