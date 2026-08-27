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
import { fileURLToPath } from "node:url";

import { RENDER_TWINS, STATES, TRANSITIONS } from "../../../web/statelab/registry.mjs";
import { VIEWPORTS } from "../../../web/statelab/selectors.mjs";
import { shotName, twinParticipants } from "../gallery-audit.mjs";
import { indexPage, rowsFor, applyMarks, SETTLED_PHRASE } from "../gallery-index.mjs";
import { notices } from "../global-teardown.mjs";
import { enterState, applyOps, expect, galleryDir, test } from "../matrix-fixtures.mjs";

const VIEWPORT_LIST = Object.values(VIEWPORTS);
const TWIN_SHOTS = twinParticipants(RENDER_TWINS);

/** What counts as drawn, spelled once, for both of the checks below. */
const DRAWN = fileURLToPath(new URL("../drawn.js", import.meta.url));

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
 * sheet as text can settle it. So the page is loaded and the figures are
 * compared as rendered: a stamped one must not be drawn like its neighbours,
 * and the words the mark promises must actually be in the caption.
 *
 * The stamped figure is deliberately *not* the first on the page, and every
 * unstamped figure is checked rather than one of them. While it was the first,
 * and its single neighbour the only control, a positional rule — say
 * `.shots figure:first-of-type figcaption::after` beside the real one — kept
 * this green while telling a reviewer that the first render of all 256 cards
 * was not proved settled. A mark drawn on the wrong screens is worse than one
 * drawn on none, so the question is asked of the whole page.
 *
 * It is asked of the whole *figure*, too. Reading the caption's `::after` and
 * the image's border named two channels out of many: `.shots figure
 * :first-of-type::after` draws the same amber sentence on the figure itself,
 * where neither was looking, and this stayed green while every card's first
 * render carried the warning. So both pseudo-elements of the figure and of
 * everything inside it are read. And the unstamped figures are required to be
 * drawn like *each other* rather than merely unlike the stamped one — a rule
 * that restyles some of them differs from the stamped border just as well.
 *
 * Pseudo-elements were themselves only two more channels. The sentence is CSS
 * content *here*, but nothing stops it being ordinary markup: a `<span>` added
 * to `figcaption` puts the same words on all 512 renders, in the one channel a
 * pseudo-element read cannot see, and this check stayed green while every
 * figure on the page told a reviewer it was not proved settled. What is being
 * asked is whether a figure *says* it, so the figure's own text is read
 * alongside its pseudo-elements.
 *
 * The words come from `SETTLED_PHRASE`, which is what the stylesheet draws and
 * what the notice names. Spelled separately here, this hunted for a sentence
 * the product had stopped writing, and would have gone green over a page where
 * no mark said anything at all.
 *
 * And every channel it reads was still only being read for its *value*. A rule
 * that is present, correctly spelled and matching the stamped figure can paint
 * nothing at all — `border-color:transparent` beside `border-width:2px`, and
 * `color:transparent` on the caption's `::after`, leave the border string
 * different from every neighbour's and the phrase present in `content`, and
 * this returned the expected tuple over a page where not one mark was visible.
 * So the mark is asked whether it is *painted*: the stamped figure has to be
 * visible, its border opaque, and every channel that says the words has to say
 * them in a colour with alpha. `drawn.js` decides that once, for this check and
 * for the notices below, so the two cannot drift into asking different
 * questions about the same page.
 */
test("@matrix an unsettled figure is drawn unlike a settled one", async ({ page }) => {
  const target = shot(STATES[0], VIEWPORT_LIST[VIEWPORT_LIST.length - 1]);
  const marked = applyMarks(indexPage(rowsFor(STATES, VIEWPORT_LIST, shot), STATES, VIEWPORT_LIST), "", [target]);

  await page.setContent(marked.html);
  await page.addScriptTag({ path: DRAWN });

  const drawn = await page.evaluate((phrase) => {
    const { alpha, saying, visible } = window.bureauDrawn;
    const border = (figure) => {
      const style = getComputedStyle(figure.querySelector("img"));
      return `${style.borderTopColor} ${style.borderTopWidth}`;
    };
    const stamped = document.querySelector("figure[data-settled]");
    const plain = [...document.querySelectorAll("figure:not([data-settled])")];
    const alike = new Set(plain.map(border));
    const said = saying(stamped, phrase);
    return [
      plain.length > 1,
      alike.size,
      [...alike].every((drawing) => drawing !== border(stamped)),
      said.length > 0 && said.every((value) => value > 0),
      plain.filter((figure) => saying(figure, phrase).length).length,
      visible(stamped),
      alpha(getComputedStyle(stamped.querySelector("img")).borderTopColor) > 0,
    ];
  }, SETTLED_PHRASE);

  expect(drawn, "every unstamped figure is drawn alike, unlike the stamped one, and only it visibly says so").toEqual([true, 1, true, true, 0, true, true]);
});

/**
 * The notices a run writes about itself are asked the same question.
 *
 * `notices` is pure and the offline suite holds every word of it, but it holds
 * them as a *string*. The one artefact a reviewer opens to learn a run went
 * wrong is a rendered page, and a banner can be in that page's markup and out
 * of its reader's sight: `<p hidden …>`, `display:none`, `opacity:0` and
 * `color:transparent` all leave every offline assertion — which searches the
 * serialized html for the sentence — exactly as green as a clean run, over a
 * gallery that has stopped saying anything went wrong.
 *
 * The advisory is asked *alone* as well as beside the alarm, because they are
 * produced by two independent branches of `notices` and only the second is a
 * red banner. An advisory-only run is the ordinary result of a full matrix —
 * `transport:playing` animates by design and can never settle — so the notice a
 * reviewer meets on almost every good run is the one that had no browser check
 * at all.
 *
 * The renders are marked too, so the amber note has something true to say, and
 * the phrase it must carry is `SETTLED_PHRASE` for the same reason the mark's
 * is: spelled separately, this would keep hunting for a sentence the product
 * had stopped writing.
 */
const NOTICE_CASES = [
  { id: "an advisory alone", lines: [], missing: [], drawn: 1 },
  { id: "an alarm above an advisory", lines: ["a hole"], missing: ["a state @ desktop"], drawn: 2 },
];

test("@matrix a run's notices are drawn where a reviewer will read them", async ({ page }) => {
  const target = shot(STATES[0], VIEWPORT_LIST[VIEWPORT_LIST.length - 1]);
  const page1 = indexPage(rowsFor(STATES, VIEWPORT_LIST, shot), STATES, VIEWPORT_LIST);
  const read = [];

  for (const found of NOTICE_CASES) {
    await page.setContent(applyMarks(page1, notices(found.lines, found.missing, [target], [], []), [target]).html);
    await page.addScriptTag({ path: DRAWN });
    read.push(await page.evaluate((phrase) => {
      const { visible } = window.bureauDrawn;
      const notes = [...document.querySelectorAll("body > p")];
      return [notes.length, notes.every((note) => visible(note)), notes.filter((note) => note.textContent.includes(phrase)).length];
    }, SETTLED_PHRASE));
  }

  expect(read, "every notice a run writes is visible, and says what it promises").toEqual(NOTICE_CASES.map((found) => [found.drawn, true, 1]));
});
