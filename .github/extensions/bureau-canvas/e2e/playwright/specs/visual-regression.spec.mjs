import { STATES } from "../../../web/statelab/registry.mjs";
import { enterState, expect, test } from "../matrix-fixtures.mjs";

const DESKTOP = { width: 1280, height: 900 };
const COMPACT = { width: 760, height: 900 };

const SCREENS = [
  screen("empty-configuration", "surface:config+data:validated+section:empty", DESKTOP),
  screen("assignment-overview", "surface:config+data:validated+section:stack+card:collapsed", DESKTOP),
  screen("validation-and-advisory", "surface:config+data:invalid-advisory+section:stack+card:collapsed", DESKTOP),
  screen("pipeline-design", "surface:pipeline+data:validated+mode:design", DESKTOP),
  screen("live-run", "surface:pipeline+data:validated+mode:live+run:running", DESKTOP),
  screen("finished-run-replay", "surface:pipeline+data:validated+mode:replay+run:finished", DESKTOP),
  screen("pipeline-editor", "surface:editor+tab:pipeline+pick:agent", DESKTOP),
  screen("unsaved-pipeline-edit", "surface:editor+tab:pipeline+pick:agent+edit:renamed", DESKTOP),
  screen("assignment-overview-compact", "surface:config+data:validated+section:stack+card:collapsed", COMPACT),
  screen("unsaved-pipeline-edit-compact", "surface:editor+tab:pipeline+pick:agent+edit:renamed", COMPACT),
];

/**
 * The header prints the absolute directory the config was loaded from, which is
 * the one thing on these screens that is not a property of the UI: it is
 * `/home/runner/work/...` on CI and someone's checkout everywhere else. Baking
 * it into a baseline made the approved screens pass on exactly one machine and
 * fail on every other, which is the opposite of a regression gate — a reviewer
 * learns to expect a red run and stops reading it.
 *
 * Masking the box is necessary and was not sufficient. `.status` is an
 * auto-sized grid and `.config-path` is its widest item, so the column — and
 * therefore the masked rectangle — is as wide as whatever path this machine
 * happens to have. Two magenta boxes of different widths differ by more pixels
 * than the text did. So the width is pinned first and then the box is masked:
 * the rectangle is identical everywhere, and it stays a visible rectangle in
 * the approved image rather than a blank, so a reviewer browsing the gallery
 * can see that this region is excluded instead of wondering where it went.
 *
 * What that gives up is the path box's own intrinsic width, which is a
 * property of the host and not of the canvas. Everything the header does
 * around it — the right edge every line aligns to, the one-line height the
 * `white-space: nowrap` rule guarantees, and the rest of the screen below —
 * is still compared exactly.
 */
const HOST_PATH = ".config-path";
const PIN_HOST_PATH = `${HOST_PATH} { width: 12rem; }`;

test.describe("@visual approved product screens", () => {
  for (const item of SCREENS) {
    test(item.name, async ({ watched, host }) => {
      await watched.page.setViewportSize(item.viewport);
      const result = await enterState(item.state, watched.page, host);

      expect(result.failures, `${item.name} must satisfy its structural checks`).toEqual([]);
      await watched.page.addStyleTag({ content: PIN_HOST_PATH });
      await expect(watched.page).toHaveScreenshot(`${item.name}.png`, {
        animations: "disabled",
        caret: "hide",
        fullPage: true,
        mask: [watched.page.locator(HOST_PATH)],
      });
    });
  }
});

function screen(name, stateId, viewport) {
  const state = STATES.find((candidate) => candidate.id === stateId);
  if (!state) {
    throw new Error(`Canonical visual state is missing: ${stateId}`);
  }
  return { name, state, viewport };
}

/**
 * The claim the pin-and-mask makes is that the approved image does not depend
 * on this machine's checkout path. That claim is worth exactly as much as a
 * test of it, so here it is: the same state is rendered with a path far longer
 * than any real one, and it has to match the *approved* image — not a second
 * capture of itself, which would pass no matter what.
 *
 * This is the check that would have caught the original defect, and it is also
 * the one that caught the first attempt at fixing it: masking alone left the
 * rectangle as wide as the path, so a longer path drew a wider box and the
 * baseline moved anyway.
 */
test("@visual an approved screen does not depend on the host's config path", async ({ watched, host }) => {
  const item = SCREENS[0];
  await watched.page.setViewportSize(item.viewport);
  await enterState(item.state, watched.page, host);
  await watched.page.addStyleTag({ content: PIN_HOST_PATH });
  await watched.page.locator(HOST_PATH).evaluate((node) => {
    node.textContent = "/var/lib/some-other-runner/deeply/nested/checkout/bureau/.bureau";
  });

  await expect(watched.page).toHaveScreenshot(`${item.name}.png`, {
    animations: "disabled",
    caret: "hide",
    fullPage: true,
    mask: [watched.page.locator(HOST_PATH)],
  });
});
