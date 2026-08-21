// Repos: order is the meaning. The first entry is where the branch lands,
// which the YAML never says, so the field has to.
//
// The bundled sample registers exactly one repo, so reordering and the
// read-only-primary warning are asserted in the offline suite
// (test/landingproblem.test.mjs) where a second repo can be arranged.

import { test, expect } from "../fixtures.mjs";

test.describe("repos field", () => {
  test("marks the primary repo at rest", async ({ card }) => {
    const chips = card.page.locator(".repos-value .repo-chip");

    await expect(chips).toHaveCount(1);
    await expect(chips.first()).toHaveText("bureau · primary");
    await expect(chips.first()).toHaveClass(/repo-chip--primary/u);
  });

  test("opening it ranks the repos and names where the branch lands", async ({ card }) => {
    await card.page.locator(".repos-value").click();

    await expect(card.page.locator(".repo-row")).toHaveCount(1);
    await expect(card.page.locator(".repo-rank").first()).toHaveText("1");
    await expect(card.page.locator(".repo-row--primary .repo-primary")).toHaveText("primary — the branch lands here");
  });

  test("each repo shows the access level the registry grants it", async ({ card }) => {
    await card.page.locator(".repos-value").click();

    await expect(card.page.locator(".repo-row").first().locator(".access")).toHaveText("push");
  });

  test("a lone repo can move neither up nor down", async ({ card }) => {
    await card.page.locator(".repos-value").click();

    await expect(card.page.getByRole("button", { name: "Move bureau up" })).toBeDisabled();
    await expect(card.page.getByRole("button", { name: "Move bureau down" })).toBeDisabled();
  });

  test("every row can be removed by name", async ({ card }) => {
    await card.page.locator(".repos-value").click();

    await expect(card.page.getByRole("button", { name: "Remove bureau" })).toBeEnabled();
  });

  test("saving stays refused when a change would leave no landing repo", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    const save = card.page.getByRole("button", { name: "Save repos" });

    await expect(save).toBeDisabled();
    await card.page.getByRole("button", { name: "Remove bureau" }).click();
    await expect(save).toBeDisabled();
  });

  test("emptying the list says which repo is missing rather than nothing", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "Remove bureau" }).click();

    await expect(card.page.locator(".repos-editor .note").first()).toContainText("No repos yet");
  });

  test("the adder reports when every registered repo is already listed", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();

    await expect(card.page.locator(".repos-editor .note").first()).toContainText("Every registered repo is already listed");
  });

  test("a repository URL resolves to a registry entry", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await card.page.getByLabel("Repository URL").fill("https://github.com/microsoft/rushstack");

    await expect(card.page.locator("#repo-name")).toHaveValue("rushstack");
    await expect(card.page.locator(".repos-preview code").first()).toHaveText("github");
  });

  test("a board URL is refused, because it names no repository", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await card.page.getByLabel("Repository URL").fill("https://onedrive.visualstudio.com/EFun/_boards/board/t/Web/Items");

    await expect(card.page.locator(".repos-editor .note--err")).toContainText("names no repository");
    await expect(card.page.locator(".repos-preview")).toHaveCount(0);
  });

  test("access and credential are chosen, never guessed from the URL", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await card.page.getByLabel("Repository URL").fill("https://github.com/microsoft/rushstack");

    await expect(card.page.locator("#repo-access")).toHaveValue("read");
    await expect(card.page.locator("#repo-credential")).toHaveValue("github-main");
  });

  test("a repo added below the primary is described as context", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await card.page.getByLabel("Repository URL").fill("https://github.com/microsoft/rushstack");

    await expect(card.page.locator(".repos-preview .note").last()).toContainText("the run reads it for context");
  });
});
