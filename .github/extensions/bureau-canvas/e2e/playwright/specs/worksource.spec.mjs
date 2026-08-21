// Work source: paste the page you already have open, and confirm what the
// URL actually says before anything is written.

import { test, expect } from "../fixtures.mjs";

const ADO_BOARD = "https://onedrive.visualstudio.com/EFun/_boards/board/t/Web/Backlog%20items?System.AssignedTo=%40me";
const GITHUB_ISSUES = "https://github.com/TheLarkInn/bureau/issues?q=is%3Aopen+label%3Aagent-eligible";

test.describe("work source field", () => {
  test("shows the forge and source at rest", async ({ card }) => {
    await expect(card.page.locator(".ws-value")).toHaveText("github · TheLarkInn/bureau");
  });

  test("clicking it opens a paste box and nothing else", async ({ card }) => {
    await card.page.locator(".ws-value").click();

    await expect(card.page.locator(".paste-input")).toBeFocused();
    await expect(card.page.locator(".derived")).toHaveCount(0);
  });

  test("an Azure DevOps board URL derives the forge, project team and WIQL", async ({ card }) => {
    await card.page.locator(".ws-value").click();
    await card.page.locator(".paste-input").fill(ADO_BOARD);

    await expect(card.page.locator(".pill--ado")).toHaveText("ado");
    await expect(card.page.locator(".derived code").first()).toHaveText("EFun/Web");
    await expect(card.page.locator(".derived code").nth(1)).toHaveText("[System.AssignedTo] = @Me");
  });

  test("a board URL says plainly that column rules are not in the link", async ({ card }) => {
    await card.page.locator(".ws-value").click();
    await card.page.locator(".paste-input").fill(ADO_BOARD);

    await expect(card.page.locator(".note--warn")).toContainText("board column and swimlane rules are not in the URL");
  });

  test("a GitHub issues URL carries its search query through verbatim", async ({ card }) => {
    await card.page.locator(".ws-value").click();
    await card.page.locator(".paste-input").fill(GITHUB_ISSUES);

    await expect(card.page.locator(".pill--github")).toHaveText("github");
    await expect(card.page.locator(".derived code").nth(1)).toHaveText("is:open label:agent-eligible");
    await expect(card.page.locator(".derived .note")).toContainText("derived exactly from the URL");
  });

  test("a URL it cannot read is refused with a reason, and cannot be used", async ({ card }) => {
    await card.page.locator(".ws-value").click();
    await card.page.locator(".paste-input").fill("https://gitlab.com/owner/repo/-/issues");

    await expect(card.page.locator(".note--err")).toContainText("unrecognized host");
    await expect(card.page.getByRole("button", { name: "Use this" })).toBeDisabled();
  });

  test("nothing can be used until a URL resolves", async ({ card }) => {
    await card.page.locator(".ws-value").click();

    await expect(card.page.getByRole("button", { name: "Use this" })).toBeDisabled();
  });

  test("cancelling restores the committed value", async ({ card }) => {
    await card.page.locator(".ws-value").click();
    await card.page.locator(".paste-input").fill(ADO_BOARD);
    await card.page.getByRole("button", { name: "Cancel" }).click();

    await expect(card.page.locator(".ws-value")).toHaveText("github · TheLarkInn/bureau");
  });
});
