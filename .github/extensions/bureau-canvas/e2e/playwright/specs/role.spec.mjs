// Role: a capability summary that opens into a picker, or a new role
// scaffolded at least privilege. Renaming and deleting are deliberately
// absent — a role is shared with every pipeline step that names it.

import { test, expect } from "../fixtures.mjs";

test.describe("role field", () => {
  test("summarises the grants, trust floor and adapter at rest", async ({ card }) => {
    const box = card.page.locator(".rolebox");

    await expect(box.locator(".rolename")).toHaveText("implementer");
    await expect(box.locator(".trust")).toHaveText("maintainer");
    await expect(box.locator(".perm")).toHaveCount(3);
    await expect(box.locator(".note")).toHaveText("copilot · /bureau:implementer");
  });

  test("grants are graded by consequence, not listed flat", async ({ card }) => {
    const box = card.page.locator(".rolebox");

    await expect(box.locator(".perm--write")).toHaveText("repo:write");
    await expect(box.locator(".perm--model")).toHaveText("model:invoke");
  });

  test("the summary is a disclosure control", async ({ card }) => {
    await expect(card.page.locator(".rolebox")).toHaveAttribute("aria-expanded", "false");
  });

  test("opening it previews the selected role before anything is committed", async ({ card }) => {
    await card.page.locator(".rolebox").click();
    await card.page.getByLabel("role").selectOption("reviewer");

    await expect(card.page.locator(".role-preview .trust")).toHaveText("derived");
    await expect(card.page.locator(".role-preview .perm")).toHaveCount(2);
  });

  test("using a role is refused until a different one is chosen", async ({ card }) => {
    await card.page.locator(".rolebox").click();
    const use = card.page.getByRole("button", { name: "Use this role" });

    await expect(use).toBeDisabled();
    await card.page.getByLabel("role").selectOption("reviewer");
    await expect(use).toBeEnabled();
  });

  test("the editor states that renaming and deleting live elsewhere", async ({ card }) => {
    await card.page.locator(".rolebox").click();

    await expect(card.page.locator(".role-editor .note")).toContainText("shared with every pipeline step that names it");
  });

  test("a new role arrives at least privilege", async ({ card }) => {
    await card.page.locator(".rolebox").click();
    await card.page.getByRole("button", { name: "+ New role" }).click();

    await expect(card.page.locator('.perm-toggle[aria-pressed="true"]')).toHaveCount(2);
    await expect(card.page.locator('.perm-toggle[aria-pressed="true"]').first()).toHaveText("repo:read");
    await expect(card.page.locator("#role-trust")).toHaveValue("derived");
  });

  test("read-only grants are reported as such", async ({ card }) => {
    await card.page.locator(".rolebox").click();
    await card.page.getByRole("button", { name: "+ New role" }).click();

    await expect(card.page.locator(".role-editor .note").last())
      .toContainText("A step that never pushes should never hold a token that can");
  });

  test("granting push names the config PR as the only gate on it", async ({ card }) => {
    await card.page.locator(".rolebox").click();
    await card.page.getByRole("button", { name: "+ New role" }).click();
    await card.page.getByRole("button", { name: "repo:push", exact: true }).click();

    await expect(card.page.locator(".note--warn")).toContainText("Review of the config PR is the only gate on that");
  });

  test("creating is refused without a name, or with one already taken", async ({ card }) => {
    await card.page.locator(".rolebox").click();
    await card.page.getByRole("button", { name: "+ New role" }).click();
    const create = card.page.getByRole("button", { name: "Create and use it" });

    await expect(create).toBeDisabled();
    await card.page.locator("#role-name").fill("reviewer");
    await expect(card.page.locator(".note--err")).toContainText("already exists");
    await expect(create).toBeDisabled();
    await card.page.locator("#role-name").fill("patcher");
    await expect(create).toBeEnabled();
  });
});
