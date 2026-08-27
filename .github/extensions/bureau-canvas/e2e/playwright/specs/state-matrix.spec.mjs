// The state matrix, generated from the registry.
//
// Every reachable state is rendered at both recorded viewports by the real
// production page, checked against the controls and copy the registry promises
// for it, and captured into a browsable gallery. Nothing here names a state:
// the list comes from `web/statelab/registry.mjs`, so a state added to the
// registry is rendered and asserted the moment it exists.

import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { RENDER_TWINS, STATES, TRANSITIONS } from "../../../web/statelab/registry.mjs";
import { VIEWPORTS } from "../../../web/statelab/selectors.mjs";
import { shotName, twinParticipants } from "../gallery-audit.mjs";
import { indexPage, rowsFor, applyMarks } from "../gallery-index.mjs";
import { enterState, applyOps, expect, galleryDir, test } from "../matrix-fixtures.mjs";

const VIEWPORT_LIST = Object.values(VIEWPORTS);
const TWIN_SHOTS = twinParticipants(RENDER_TWINS);

function shot(state, viewport) {
  return shotName(state.id, viewport.id);
}

/**
 * The render's DOM signature and whether it was ever proved settled, filed
 * beside its screenshot for the teardown to audit. One small file per render
 * rather than one shared file, because the renders are written by several
 * workers at once and a shared file is a race. `global-teardown.mjs` collapses
 * them into the gallery's `signatures.json` and `settled.json`.
 *
 * Settle-proof is filed rather than inferred because it cannot be recovered
 * afterwards: a raced frame and a settled one are the same PNG and the same
 * digest to anything reading the published directory. Only the loop that took
 * the sample knows, and it now says so.
 *
 * Renders that a declared twin names carry the signature itself as well as its
 * digest. A digest answers "do these two draw the same screen" and nothing
 * else, so a broken twin was a dead end: the audit could say two states had
 * stopped matching and could not say in what, and a reviewer had two
 * screenshots and a hash. It is kept to the renders a twin actually names —
 * a couple of dozen rather than five hundred — because that is the only place
 * the difference is ever asked for.
 */
async function fileSignature(name, result) {
  const dir = join(galleryDir(), "signatures");
  await mkdir(dir, { recursive: true });
  const signature = result?.snapshot?.signature ?? "";
  const record = {
    signature: createHash("sha256").update(signature).digest("hex"),
    settled: Boolean(result?.settled),
    ...(TWIN_SHOTS.has(name) ? { detail: signature } : {}),
  };
  await writeFile(join(dir, `${name}.json`), JSON.stringify(record), "utf8");
}

for (const viewport of VIEWPORT_LIST) {
  test.describe(`@matrix ${viewport.id} (${viewport.width}×${viewport.height})`, () => {
    for (const state of STATES) {
      test(`renders ${state.id}`, async ({ watched, host }, testInfo) => {
        await watched.page.setViewportSize({ width: viewport.width, height: viewport.height });
        const result = await enterState(state, watched.page, host);

        await watched.page.screenshot({ path: join(galleryDir(), shot(state, viewport)), fullPage: true });
        await fileSignature(shot(state, viewport), result);
        await testInfo.attach(`${viewport.id} ${state.id}`, {
          path: join(galleryDir(), shot(state, viewport)),
          contentType: "image/png",
        });

        expect(describe(result.failures), `${state.id} @ ${viewport.id}`).toEqual([]);
        expect(unexpected(watched.errors, state), `${state.id} @ ${viewport.id} console`).toEqual([]);
      });
    }
  });
}

function describe(failures) {
  return failures.map((failure) => `${failure.kind}: ${failure.detail}`);
}

/** Errors the state did not declare. A declared one is the state, not a bug. */
function unexpected(errors, state) {
  const allowed = state.expect.allowErrors ?? [];
  return errors.filter((error) => !allowed.some((pattern) => error.includes(pattern)));
}

/**
 * Every edge of the transition DAG, walked as an edge.
 *
 * The claim the DAG makes is that the child is the parent plus one operation,
 * so the test enters the *parent*, asserts the parent is what the registry
 * says it is, then applies only the delta and asserts the child. Re-entering
 * the child from scratch would prove nothing the render tests do not already
 * prove, and would leave the graph the lab draws for a human unverified.
 */
test.describe("@matrix transitions", () => {
  for (const edge of TRANSITIONS) {
    test(`${edge.from} → ${edge.to}`, async ({ watched, host }) => {
      const from = STATES.find((state) => state.id === edge.from);
      const to = STATES.find((state) => state.id === edge.to);

      const parent = await enterState(from, watched.page, host);
      expect(describe(parent.failures), `parent ${edge.from}`).toEqual([]);

      const child = await applyOps(edge.delta, to, watched.page, host);
      expect(describe(child.failures), `${edge.from} → ${edge.to} via ${edge.via}`).toEqual([]);
      expect(unexpected(watched.errors, to)).toEqual([]);
    });
  }
});

/**
 * Writes the gallery index last, from the same registry the shots came from.
 *
 * The assertion reads the file back rather than checking the value it just
 * built: a gallery is only browsable if every state's shots are actually
 * reachable from the index, and a truncated write, a template that dropped a
 * row, or an escape that broke a `src` all produce an index that renders and
 * silently omits states. Missing image *files* are not asserted here — the
 * shots are written by other workers and this test may run before them.
 */
test("@matrix gallery index", async () => {
  const rows = rowsFor(STATES, VIEWPORT_LIST, shot);

  await writeFile(join(galleryDir(), "index.html"), indexPage(rows, STATES, VIEWPORT_LIST), "utf8");

  const written = await readFile(join(galleryDir(), "index.html"), "utf8");
  const missing = STATES.flatMap((state) =>
    VIEWPORT_LIST
      .filter((viewport) => !written.includes(`src="./${shot(state, viewport)}"`))
      .map((viewport) => `${state.id} @ ${viewport.id}`));
  expect(missing, "states the gallery index does not link").toEqual([]);
});

/**
 * The amber mark is asked whether it is *drawn*, by a browser.
 *
 * The offline suite can hold the marker and the stylesheet to one attribute,
 * but a selector is only half a mark: emptying both declaration blocks leaves
 * every rule present, spelled correctly, matching the stamped figure — and
 * changing nothing a reviewer can see. That is the same defect as an anchor
 * that drifted, asserted one notch further down, and no amount of reading the
 * sheet as text can settle it. So the page is loaded and the two figures are
 * compared as rendered: a stamped one must not be drawn like its neighbour, and
 * the words the mark promises must actually be in the caption.
 */
test("@matrix an unsettled figure is drawn unlike a settled one", async ({ page }) => {
  const target = shot(STATES[0], VIEWPORT_LIST[0]);
  const marked = applyMarks(indexPage(rowsFor(STATES, VIEWPORT_LIST, shot), STATES, VIEWPORT_LIST), "", [target]);

  await page.setContent(marked.html);

  const drawn = await page.evaluate(() => {
    const border = (figure) => {
      const style = getComputedStyle(figure.querySelector("img"));
      return `${style.borderTopColor} ${style.borderTopWidth}`;
    };
    const after = (figure) => getComputedStyle(figure.querySelector("figcaption"), "::after").content;
    const stamped = document.querySelector("figure[data-settled]");
    const plain = document.querySelector("figure:not([data-settled])");
    return [border(stamped) !== border(plain), after(stamped).includes("not proved settled"), after(plain).includes("not proved settled")];
  });

  expect(drawn, "a stamped figure is drawn unlike an unstamped one, and says so").toEqual([true, true, false]);
});
