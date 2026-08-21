// Pipeline editor: the operational workflow must not trap the user, repeat
// false warnings, or waste a wide canvas on a vertical stack.

import { test, expect } from "../fixtures.mjs";

async function boxes(locator) {
  return locator.evaluateAll((elements) => elements
    .map((element) => {
      const box = element.getBoundingClientRect();
      return {
        label: element.textContent?.trim().replace(/\s+/gu, " ") ?? "",
        left: box.left,
        right: box.right,
        top: box.top,
        bottom: box.bottom,
      };
    })
    .filter((box) => box.right > box.left && box.bottom > box.top));
}

function overlaps(items) {
  const found = [];
  for (let left = 0; left < items.length; left += 1) {
    for (let right = left + 1; right < items.length; right += 1) {
      const a = items[left];
      const b = items[right];
      if (a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top) {
        found.push([a.label, b.label]);
      }
    }
  }
  return found;
}

test.describe("pipeline editor", () => {
  test("provides a persistent path back to assignments", async ({ editor }) => {
    const back = editor.page.getByRole("button", { name: "Assignments" });

    await expect(back).toBeVisible();
    await back.click();
    await expect(editor.page.locator(".assignment-card")).toBeVisible();
    await expect(editor.page.locator(".assignment-detail")).toBeVisible();
  });

  test("shows Pipeline and Relations as one segmented view switcher", async ({ editor }) => {
    await expect(editor.page.getByRole("button", { name: "Pipeline" })).toHaveAttribute("aria-pressed", "true");
    await expect(editor.page.getByRole("button", { name: "Relations" })).toHaveAttribute("aria-pressed", "false");
  });

  test("switches to relations and back without leaving the editor", async ({ editor }) => {
    await editor.page.getByRole("button", { name: "Relations" }).click();
    await expect(editor.page.getByRole("button", { name: "Relations" })).toHaveAttribute("aria-pressed", "true");
    await expect(editor.page.locator(".relation-flow")).toBeVisible();
    await expect(editor.page.locator(".relation-flow .react-flow__edge")).not.toHaveCount(0);

    await editor.page.getByRole("button", { name: "Pipeline", exact: true }).click();
    await expect(editor.page.locator(".editor-flow")).toBeVisible();
  });

  test("keeps an unsaved pipeline draft while viewing relations", async ({ editor }) => {
    await editor.page.getByRole("button", { name: "+ Add step" }).click();
    await editor.page.getByRole("button", { name: "Relations" }).click();
    await editor.page.getByRole("button", { name: "Pipeline", exact: true }).click();

    await expect(editor.page.locator('[data-ref="step-4"]')).toBeVisible();
    await expect(editor.page.locator(".editor-status")).toContainText(/unsaved|issue/u);
  });

  test("lays sequential steps left to right", async ({ editor }) => {
    const x = [];
    for (const name of ["implement", "verify", "review"]) {
      const box = await editor.page.locator(`[data-ref="${name}"]`).boundingBox();
      x.push(box?.x ?? 0);
    }

    expect(x[0]).toBeLessThan(x[1]);
    expect(x[1]).toBeLessThan(x[2]);
  });

  test("step and terminal cards do not overlap", async ({ editor }) => {
    const cards = await boxes(editor.page.locator(".editor-card, .editor-terminal"));

    expect(overlaps(cards)).toEqual([]);
  });

  test("edge captions do not overlap each other", async ({ editor }) => {
    const captions = await boxes(editor.page.locator(".edge-caption"));

    expect(overlaps(captions)).toEqual([]);
  });

  test("does not report implicit abort defaults as missing branches", async ({ editor }) => {
    await expect(editor.page.locator(".editor-status")).toHaveText("saved");
    await expect(editor.page.getByRole("heading", { name: /Issues/u })).toHaveCount(0);
    await expect(editor.page.locator(".editor-card__issue-count")).toHaveCount(0);
  });

  test("renders terminal effects in human language", async ({ editor }) => {
    await expect(editor.page.locator(".editor-terminal--done")).toContainText("Publish");
    await expect(editor.page.locator(".editor-terminal--done")).toContainText("open a pull request");
    await expect(editor.page.locator(".editor-terminal--escalate")).toContainText("Needs human");
    await expect(editor.page.locator(".editor-terminal--escalate")).toContainText("Comment on the work item");
    await expect(editor.page.locator(".editor-terminal--abort")).toHaveCount(0);
  });

  test("save is disabled until the pipeline changes", async ({ editor }) => {
    await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeDisabled();
    await editor.page.getByRole("button", { name: "+ Add step" }).click();
    await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeEnabled();
  });

  test("compact editor keeps save state and inspector visible", async ({ editor }) => {
    await editor.page.setViewportSize({ width: 800, height: 900 });
    await editor.page.locator('[data-ref="verify"]').click();

    await expect(editor.page.locator(".editor-status")).toBeVisible();
    await expect(editor.page.locator(".editor-panel")).toBeVisible();
    expect(await editor.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });

  test("adding a step selects it and opens a styled inspector", async ({ editor }) => {
    await editor.page.getByRole("button", { name: "+ Add step" }).click();

    await expect(editor.page.locator('[data-ref="step-4"]')).toBeVisible();
    await expect(editor.page.locator(".editor-step")).toBeVisible();
    await expect(editor.page.locator(".editor-step .form-control").first()).toBeVisible();
  });

  for (const kind of ["deterministic", "agent", "decision", "concurrent"]) {
    test(`adds a ${kind} step from the toolbar`, async ({ editor }) => {
      await editor.page.getByLabel("New step kind").selectOption(kind);
      await editor.page.getByRole("button", { name: "+ Add step" }).click();

      await expect(editor.page.locator('[data-ref="step-4"] .kind-label')).toHaveText(kind);
      await expect(editor.page.locator(".editor-step")).toBeVisible();
    });
  }

  test("a deterministic step exposes command, outcomes and retry controls", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();

    await expect(editor.page.getByLabel("run")).toHaveValue("cargo test --offline");
    await expect(editor.page.getByLabel("success")).toHaveValue("review");
    await expect(editor.page.getByLabel("failure")).toHaveValue("escalate");
    await expect(editor.page.getByLabel("max attempts")).toHaveValue("1");
  });

  test("renaming commits on Enter and keeps the renamed step selected", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();
    await editor.page.getByLabel("name").fill("verify-renamed");
    await editor.page.getByLabel("name").press("Enter");

    await expect(editor.page.locator('[data-ref="verify-renamed"]')).toBeVisible();
    await expect(editor.page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify-renamed");
  });

  test("a duplicate step name is explained instead of losing the inspector", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();
    await editor.page.getByLabel("name").fill("implement");

    await expect(editor.page.locator(".editor-hints")).toContainText("already exists");
    await expect(editor.page.locator('[data-ref="verify"]')).toBeVisible();
  });

  test("an agent step chooses from configured roles", async ({ editor }) => {
    await editor.page.locator('[data-ref="implement"]').click();
    const role = editor.page.getByLabel("Step role");

    await expect(role).toHaveValue("implementer");
    await expect(role.locator("option")).toHaveText(["Choose a role", "implementer", "reviewer"]);
  });

  test("edge selectors explain terminal effects instead of raw wire words", async ({ editor }) => {
    await editor.page.locator('[data-ref="implement"]').click();
    const failure = editor.page.getByLabel("failure");

    await expect(failure.locator('option[value="done"]')).toContainText("Publish");
    await expect(failure.locator('option[value="abort"]')).toContainText("Failed");
    await expect(failure.locator('option[value="escalate"]')).toContainText("Needs human");
  });

  test("a decision step exposes the observed step and all four outcomes", async ({ editor }) => {
    await editor.page.getByLabel("New step kind").selectOption("decision");
    await editor.page.getByRole("button", { name: "+ Add step" }).click();

    await expect(editor.page.getByLabel("observe step")).toBeVisible();
    for (const outcome of ["success", "failure", "blocked", "no-work"]) {
      await expect(editor.page.getByLabel(outcome)).toBeVisible();
    }
    await expect(editor.page.locator(".editor-terminal--abort")).toBeVisible();
    await expect(editor.page.locator("[data-id^='control:step-4']")).toHaveCount(4);
  });

  test("a concurrent step exposes its member list", async ({ editor }) => {
    await editor.page.getByLabel("New step kind").selectOption("concurrent");
    await editor.page.getByRole("button", { name: "+ Add step" }).click();

    await expect(editor.page.getByLabel("members (comma-separated)")).toBeVisible();
    await expect(editor.page.getByLabel("completion")).toBeVisible();
    await expect(editor.page.getByLabel("maximum concurrent members")).toBeVisible();
  });

  test("data inputs are editable and render as data edges", async ({ editor }) => {
    await editor.page.locator('[data-ref="review"]').click();
    const before = await editor.page.locator(".editor-edge--data").count();
    await editor.page.getByRole("checkbox", { name: "verify", exact: true }).uncheck();

    await expect(editor.page.locator(".editor-edge--data")).toHaveCount(before - 1);
  });

  test("invalid max attempts is visibly refused", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();
    await editor.page.getByLabel("max attempts").fill("0");

    await expect(editor.page.getByLabel("max attempts")).toHaveClass(/form-control--invalid/u);
    await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  });

  test("an issue entry selects the affected step", async ({ editor }) => {
    await editor.page.getByRole("button", { name: "+ Add step" }).click();
    await editor.page.getByRole("button", { name: "Close" }).click();
    await editor.page.locator(".editor-issues button").filter({ hasText: "step-4" }).click();

    await expect(editor.page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("step-4");
  });

  test("Escape closes the step inspector", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();
    await editor.page.keyboard.press("Escape");

    await expect(editor.page.locator(".editor-step")).toHaveCount(0);
  });

  test("clicking a terminal leaves the active step selected", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();
    await editor.page.locator(".editor-terminal--escalate").click();

    await expect(editor.page.getByLabel("name")).toHaveValue("verify");
  });

  test("dirty navigation asks before leaving the editor", async ({ editor }) => {
    await editor.page.getByRole("button", { name: "+ Add step" }).click();
    editor.page.once("dialog", (dialog) => dialog.dismiss());
    await editor.page.getByRole("button", { name: "Assignments" }).click();

    await expect(editor.page.locator(".editor-shell")).toBeVisible();
  });

  test("a successful save clears the dirty state", async ({ editor }) => {
    await editor.page.locator('[data-ref="verify"]').click();
    await editor.page.getByLabel("run").fill("cargo test --offline --quiet");
    await editor.page.getByRole("button", { name: "Save changes" }).click();

    await expect(editor.page.locator(".editor-status")).toHaveText("saved");
    await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  });

  test("deleting a step requires confirmation and can be cancelled", async ({ editor }) => {
    await editor.page.locator('[data-ref="review"]').click();
    await editor.page.getByRole("button", { name: "Delete step" }).click();

    await expect(editor.page.locator(".editor-danger-zone")).toContainText("every edge connected to it");
    await editor.page.getByRole("button", { name: "Keep step" }).click();
    await expect(editor.page.locator('[data-ref="review"]')).toBeVisible();
  });

  test("confirming deletion removes the step only from the unsaved draft", async ({ editor }) => {
    await editor.page.locator('[data-ref="review"]').click();
    await editor.page.getByRole("button", { name: "Delete step" }).click();
    await editor.page.getByRole("button", { name: "Delete step" }).click();

    await expect(editor.page.locator('[data-ref="review"]')).toHaveCount(0);
    await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeEnabled();
  });

  test("the graph navigation controls are all operable", async ({ editor }) => {
    const controls = editor.page.locator(".editor-flow .react-flow__controls-button");
    await expect(controls).toHaveCount(4);
    for (let index = 0; index < 4; index += 1) {
      await controls.nth(index).click();
    }
  });

  test("relation graph navigation controls are all operable", async ({ editor }) => {
    await editor.page.getByRole("button", { name: "Relations" }).click();
    const controls = editor.page.locator(".relation-flow .react-flow__controls-button");
    await expect(controls).toHaveCount(4);
    for (let index = 0; index < 4; index += 1) {
      await controls.nth(index).click();
    }
  });

  test("the editor raises no console or page errors", async ({ editor }) => {
    expect(editor.errors).toEqual([]);
  });
});
