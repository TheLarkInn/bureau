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
