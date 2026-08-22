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
 * So the path is masked. Its box still takes part in layout, and the ellipsis
 * rule (`white-space: nowrap`) keeps that box one line tall whatever the path,
 * so nothing about the header's geometry is being waived here — only the
 * characters, which no reviewer was approving in the first place.
 */
const HOST_PATH = ".config-path";

test.describe("@visual approved product screens", () => {
  for (const item of SCREENS) {
    test(item.name, async ({ watched, host }) => {
      await watched.page.setViewportSize(item.viewport);
      const result = await enterState(item.state, watched.page, host);

      expect(result.failures, `${item.name} must satisfy its structural checks`).toEqual([]);
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
