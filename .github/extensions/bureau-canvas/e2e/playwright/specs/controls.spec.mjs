// Cross-surface control inventory. Every visible control needs a name a user
// can understand; progressive forms should not occupy the page before asked.

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "../fixtures.mjs";

async function unnamedControls(page) {
  return page.locator("button, input, select, textarea, a[href], summary").evaluateAll((controls) =>
    controls
      .filter((control) => {
        const style = getComputedStyle(control);
        const visible = style.display !== "none" && style.visibility !== "hidden"
          && control.getBoundingClientRect().width > 0 && control.getBoundingClientRect().height > 0;
        if (!visible) return false;
        const labelled = control.getAttribute("aria-label")
          || control.getAttribute("title")
          || control.textContent?.trim()
          || (control.id && document.querySelector(`label[for="${CSS.escape(control.id)}"]`)?.textContent?.trim())
          || control.closest("label")?.textContent?.trim();
        return !labelled;
      })
      .map((control) => `${control.tagName.toLowerCase()}.${control.className}`),
  );
}

test.describe("control system", () => {
  test("every visible config control has a human-readable name", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    expect(await unnamedControls(page)).toEqual([]);
  });

  test("every visible editor control has a human-readable name", async ({ editor }) => {
    expect(await unnamedControls(editor.page)).toEqual([]);
  });

  test("the expanded global create form has no unnamed controls", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();

    expect(await unnamedControls(page)).toEqual([]);
  });

  test("Escape closes global creation and returns focus to its trigger", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Name").press("Escape");

    await expect(page.locator("[data-testid='create-bar']")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "+ New pipeline or role" })).toBeFocused();
  });

  test("the expanded work-source form has no unnamed controls", async ({ card }) => {
    await card.page.locator(".ws-value").click();

    expect(await unnamedControls(card.page)).toEqual([]);
  });

  test("the ranked repos editor and resolver have no unnamed controls", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    expect(await unnamedControls(card.page)).toEqual([]);

    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await card.page.getByLabel("Repository URL").fill("https://github.com/microsoft/rushstack");
    expect(await unnamedControls(card.page)).toEqual([]);
  });

  test("the limits editor has no unnamed controls", async ({ card }) => {
    await card.page.locator(".limits-value").click();

    expect(await unnamedControls(card.page)).toEqual([]);
  });

  test("every step-inspector variant has no unnamed controls", async ({ editor }) => {
    for (const reference of ["implement", "verify"]) {
      await editor.page.locator(`[data-ref="${reference}"]`).click();
      expect(await unnamedControls(editor.page)).toEqual([]);
    }
    for (const kind of ["decision", "concurrent"]) {
      await editor.page.getByLabel("New step kind").selectOption(kind);
      await editor.page.getByRole("button", { name: "+ Add step" }).click();
      expect(await unnamedControls(editor.page)).toEqual([]);
    }
  });

  test("global creation is progressive rather than an unexplained open form", async ({ page, canvas }) => {
    await page.goto(canvas.url);

    await expect(page.getByRole("button", { name: "+ New pipeline or role" })).toBeVisible();
    await expect(page.getByLabel("Kind")).toHaveCount(0);
    await expect(page.getByLabel("Name")).toHaveCount(0);
  });

  test("the global creator only exposes config kinds it can scaffold safely", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();

    await expect(page.getByLabel("Kind").locator("option"))
      .toHaveText(["pipeline", "role"]);
  });

  test("the create button names the selected kind", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("pipeline");
    await page.getByLabel("Name").fill("browser-pipeline");

    await expect(page.getByRole("button", { name: "Create pipeline" })).toBeEnabled();
  });

  test("creating through the UI produces a reviewable draft, not an immediate write", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("role");
    await page.getByLabel("Name").fill("browser-role");
    await page.getByRole("button", { name: "Create role" }).click();

    await expect(page.locator("[data-testid='draft-bar']")).toBeVisible();
    await expect(readFile(join(canvas.dir, "roles", "browser-role.yaml"), "utf8")).rejects.toThrow();
  });

  test("discard removes a pending create without touching disk", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("role");
    await page.getByLabel("Name").fill("discarded-role");
    await page.getByRole("button", { name: "Create role" }).click();
    await page.getByRole("button", { name: "Discard" }).click();

    await expect(page.locator("[data-testid='draft-bar']")).toHaveCount(0);
    await expect(readFile(join(canvas.dir, "roles", "discarded-role.yaml"), "utf8")).rejects.toThrow();
  });

  test("save applies the planned file to the scratch config", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Kind").selectOption("role");
    await page.getByLabel("Name").fill("saved-role");
    await page.getByRole("button", { name: "Create role" }).click();
    await page.getByRole("button", { name: "Save" }).click();

    await expect(page.locator("[data-testid='draft-bar']")).toHaveCount(0);
    await expect.poll(async () => readFile(join(canvas.dir, "roles", "saved-role.yaml"), "utf8"))
      .toContain("name: saved-role");
  });

  test("duplicate creation stays open and explains why it was refused", async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.getByRole("button", { name: "+ New pipeline or role" }).click();
    await page.getByLabel("Name").fill("agent-eligible-pipeline");
    await page.getByRole("button", { name: "Create pipeline" }).click();

    await expect(page.locator("[data-testid='create-bar']")).toBeVisible();
    await expect(page.getByRole("alert")).toContainText("already exists");
    await expect(page.locator("[data-testid='draft-bar']")).toHaveCount(0);
  });
});

/**
 * Leaving a card with a field editor open.
 *
 * The guard exists so an unsaved edit is never thrown away silently, and that
 * is worth a prompt. What it may not do is demand one when nothing was
 * changed: an editor opened to be read is the common case, and a confirmation
 * that fires on it teaches the reader to dismiss every confirmation, including
 * the one that was protecting real work.
 *
 * The two halves are asserted on the same control so the difference is
 * genuinely the draft and not the route out.
 */
test.describe("leaving an open field editor", () => {
  const prompts = (page) => {
    const seen = [];
    page.on("dialog", (dialog) => {
      seen.push(dialog.message());
      return dialog.accept();
    });
    return seen;
  };

  test("collapsing a card that was only being read asks nothing", async ({ card }) => {
    const seen = prompts(card.page);
    await card.page.locator(".limits-value").click();
    await expect(card.page.locator(".limits-editor")).toHaveAttribute("data-dirty", "false");

    await card.page.locator(".assignment-head").click();

    await expect(card.page.locator(".assignment-detail")).toHaveCount(0);
    expect(seen).toEqual([]);
  });

  test("collapsing a card holding an unsaved edit asks first, and honours the answer", async ({ card }) => {
    const seen = prompts(card.page);
    await card.page.locator(".limits-value").click();
    await card.page.getByRole("button", { name: "runs per day limit" }).click();
    await expect(card.page.locator(".limits-editor")).toHaveAttribute("data-dirty", "true");

    await card.page.locator(".assignment-head").click();

    await expect(card.page.locator(".assignment-detail")).toHaveCount(0);
    expect(seen).toEqual(["Discard the unsaved field changes?"]);
  });

  test("every field editor reports its own draft the same way", async ({ card }) => {
    const fields = [
      [".ws-value", ".ws-open", () => card.page.getByLabel("Board, query, or issues URL").fill("https://example.com/x")],
      [".runtime-value", ".assignment-runtime-editor", () => card.page.locator("[data-testid='wr-branch']").fill("other/")],
      [".terminal-label-value", ".terminal-label-editor", () => card.page.locator("[data-testid='sig-abort']").fill("bureau:other")],
    ];

    const seen = [];
    for (const [value, editor, dirty] of fields) {
      await card.page.locator(value).click();
      const before = await card.page.locator(editor).getAttribute("data-dirty");
      await dirty();
      await expect(card.page.locator(`${editor} .draft-mark`)).toHaveText("unsaved changes");
      seen.push([before, await card.page.locator(editor).getAttribute("data-dirty")]);
      await card.page.locator(value).click();
    }

    expect(seen).toEqual(fields.map(() => ["false", "true"]));
  });

  /**
   * The repo adder sits on top of the list it was opened from, so it is the one
   * editor whose unsaved work is not all its own. Judging it on its pasted URL
   * alone would report a clean editor over a list that has already been edited,
   * and the guard would then throw that edit away without asking.
   *
   * The edit here is a removal, which is deliberately one the editor will not
   * let you save — an empty list has no repo for the branch to land in. Work
   * that cannot yet be saved is exactly the work a reader most needs the guard
   * to notice.
   */
  test("the repo adder still reports the unsaved edit in the list behind it", async ({ card }) => {
    const seen = prompts(card.page);
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "Remove bureau" }).click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();

    await expect(card.page.locator(".repos-editor")).toHaveAttribute("data-dirty", "true");
    await card.page.locator(".assignment-head").click();

    expect(seen).toEqual(["Discard the unsaved field changes?"]);
  });

  /** The other half of the adder's draft: a URL pasted over a list nobody touched. */
  test("a URL pasted into the adder is unsaved work on its own", async ({ card }) => {
    await card.page.locator(".repos-value").click();
    await card.page.getByRole("button", { name: "+ Add repo" }).click();
    await expect(card.page.locator(".repos-editor")).toHaveAttribute("data-dirty", "false");

    await card.page.getByLabel("Repository URL").fill("https://github.com/microsoft/rushstack");

    await expect(card.page.locator(".repos-editor")).toHaveAttribute("data-dirty", "true");
    await expect(card.page.locator(".repos-editor .draft-mark")).toHaveText("unsaved changes");
  });

  /**
   * Clearing the box has to clear the draft with it, including the reply still
   * in flight for what the box used to say. A derivation that lands after the
   * URL has gone leaves "Use this" live over an empty field — a save the guard
   * would then let the reader walk away from without a word, because the box it
   * reads is empty.
   *
   * The reply is held open until after the box is cleared, so the response
   * really is stale when it arrives. Waiting for the preview first and only
   * then clearing would leave nothing in flight, and the test would pass
   * against the bug.
   */
  test("a derivation that arrives after the URL is cleared leaves nothing behind", async ({ card }) => {
    let release = () => {};
    const held = new Promise((resolve) => { release = resolve; });
    let seen = 0;
    await card.page.route("**/intent", async (route) => {
      const body = route.request().postDataJSON();
      if (body?.kind === "derive-work-source" && seen++ === 0) {
        await held;
      }
      await route.continue();
    });

    await card.page.locator(".ws-value").click();
    const url = card.page.getByLabel("Board, query, or issues URL");
    await url.fill("https://github.com/TheLarkInn/bureau/issues?q=is%3Aopen");
    await url.fill("");
    release();

    await expect(card.page.locator(".ws-open")).toHaveAttribute("data-dirty", "false");
    expect(await settled(card.page)).toEqual({ preview: 0, save: true, mark: 0 });
  });
  /**
   * Cancel during a save, and the reply that lands after it.
   *
   * Cancel stays live for the whole round trip on purpose — a host that never
   * answers would otherwise leave the reader inside a form with no way out.
   * That is the decision; this is its consequence. Every editor closes itself
   * when the host answers, so a reply arriving after Cancel used to run the
   * success path anyway: it published the edit the reader had just cancelled,
   * and it re-focused the field's trigger, pulling the caret out of whatever
   * was opened next.
   *
   * The reply is held until after Cancel *and* after the next editor has been
   * opened, so both halves are genuinely in the past when it arrives. Waiting
   * for the save to finish first would leave nothing in flight and the test
   * would pass against the bug.
   */
  test("a save reply that lands after Cancel changes nothing and takes no focus", async ({ card }) => {
    let release = () => {};
    const held = new Promise((resolve) => { release = resolve; });
    await card.page.route("**/intent", async (route) => {
      if (route.request().postDataJSON()?.kind === "set-limits") {
        await held;
      }
      await route.continue();
    });

    await card.page.locator(".limits-value").click();
    await card.page.getByRole("button", { name: "runs per day limit" }).click();
    await card.page.getByRole("button", { name: "Save limits" }).click();
    await expect(card.page.getByRole("button", { name: "Saving…" })).toBeDisabled();
    await card.page.locator(".limits-editor").getByRole("button", { name: "Cancel" }).click();
    await expect(card.page.locator(".limits-editor")).toHaveCount(0);

    // Where the reader went next, and the counter the stale reply would move.
    await card.page.locator(".ws-value").click();
    const url = card.page.getByLabel("Board, query, or issues URL");
    await expect(url).toBeFocused();
    await card.page.evaluate(() => {
      window.__published = 0;
      window.addEventListener("bureau-state", () => { window.__published += 1; });
    });

    release();
    await landed(card.page, "set-limits");

    expect(await card.page.evaluate(() => window.__published)).toBe(0);
    await expect(url).toBeFocused();
    await expect(card.page.locator(".limits-editor")).toHaveCount(0);
  });

  /**
   * The same race on the one write that had no guard at all.
   *
   * `CreateBar` posted with Create still live and applied whatever came back.
   * Two presses sent two creates, and a Cancel pressed while the first was out
   * closed the form and then had the answer installed over whatever the reader
   * opened next — `publishLocalState` on a create they had abandoned, and a
   * `close()` that pulls focus back to the create trigger.
   *
   * The editors solve their half with `useLive()`, but that cannot reach this
   * one: cancelling a create closes the *form* and leaves `CreateBar` itself
   * mounted, so the component is still live and the reply is still its own. The
   * generation ticket is what makes the answer nobody's.
   *
   * Held until after Cancel and after the next editor is open, so both are
   * genuinely in the past when the reply lands.
   */
  test("a create reply that lands after Cancel installs nothing and takes no focus", async ({ card }) => {
    let release = () => {};
    const held = new Promise((resolve) => { release = resolve; });
    await card.page.route("**/intent", async (route) => {
      if (route.request().postDataJSON()?.kind === "create") {
        await held;
      }
      await route.continue();
    });

    await card.page.getByTestId("create-open").click();
    await card.page.locator("#create-name").fill("abandoned-pipeline");
    await card.page.getByTestId("create-submit").click();
    await expect(card.page.getByTestId("create-submit")).toBeDisabled();
    await card.page.getByTestId("create-cancel").click();
    await expect(card.page.getByTestId("create-bar")).toHaveCount(0);

    // Where the reader went next, and the counter the stale reply would move.
    await card.page.locator(".ws-value").click();
    const url = card.page.getByLabel("Board, query, or issues URL");
    await expect(url).toBeFocused();
    await card.page.evaluate(() => {
      window.__published = 0;
      window.addEventListener("bureau-state", () => { window.__published += 1; });
    });

    release();
    await landed(card.page, "create");

    expect(await card.page.evaluate(() => window.__published)).toBe(0);
    await expect(url).toBeFocused();
    await expect(card.page.getByTestId("create-bar")).toHaveCount(0);
  });
});

/**
 * The settle window both race tests take before reading their counter.
 *
 * Both prove a negative — a reply that lands after Cancel moves nothing — and
 * a negative is worth exactly as much as the certainty that the thing it
 * denies actually happened. A bare `waitForTimeout` after `release()` does not
 * carry that: the window is spent waiting for the round trip rather than
 * watching what the reply did, so on a loaded machine it can close while the
 * reply is still in flight, and the test passes for the one reason it must
 * never pass — the guarded path never ran at all.
 *
 * Anchoring on the response starts the window where the risk starts. The wait
 * that follows is what lets the continuation — `publishLocalState`, the focus
 * move — run and be caught, rather than what hopes the reply arrives.
 */
async function landed(page, kind) {
  await page.waitForResponse((response) => response.url().includes("/intent")
    && response.request().postDataJSON()?.kind === kind);
  await page.waitForTimeout(250);
}

/** What the work-source editor looks like once nothing is in flight. */
async function settled(page) {
  await page.waitForTimeout(150);
  return {
    preview: await page.locator(".derived").count(),
    save: await page.getByRole("button", { name: "Use this" }).isDisabled(),
    mark: await page.locator(".ws-open .draft-mark").count(),
  };
}
