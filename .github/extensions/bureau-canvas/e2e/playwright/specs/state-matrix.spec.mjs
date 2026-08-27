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
import { indexPage, rowsFor, applyMarks, SETTLED_INK, SETTLED_PHRASE, SETTLED_SLOT } from "../gallery-index.mjs";
import { notices } from "../global-teardown.mjs";
import { enterState, applyOps, expect, galleryDir, test } from "../matrix-fixtures.mjs";

const VIEWPORT_LIST = Object.values(VIEWPORTS);
const TWIN_SHOTS = twinParticipants(RENDER_TWINS);

/** What counts as drawn, spelled once, for both of the checks below. */
const DRAWN = fileURLToPath(new URL("../drawn.js", import.meta.url));

/** `#rrggbb` as the opaque `"r,g,b,a"` a screenshot of it reads back as. */
function inkOf(hex) {
  return `${[1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(",")},255`;
}

/**
 * The share of a region above which its ink is a block rather than words.
 *
 * Generous on purpose. Bold 12px type covers something like a quarter of its
 * own line box; a phrase painted over its own background covers all of it bar
 * the blended edge. Anything between those is not a case this has to judge, and
 * a bound close to either would be a number tuned to today's font.
 */
const SOLID = 0.75;

/**
 * Whether the words in `locator` are painted where a reviewer can read them.
 *
 * Five questions, and each closes what the others do not. It is in the render
 * tree at all — `hidden`, `display:none` and `opacity:0` leave a phrase nothing
 * can be read from, and this is asked first so that such a phrase names its own
 * failure rather than timing out on a screenshot of a box that is not there.
 * There is a region — `font-size:0` and a collapsed slot have none. The ink is
 * in it — `color:transparent` never appears in an opaque screenshot. It is not
 * one flat colour. The ink does not fill it, because a foreground equal to its
 * background *is* painted and is not flat either: a block of solid ink blends
 * into what is behind it at its own edges and answers with two colours, so only
 * the share separates words, which leave most of their line box unpainted, from
 * a block, which leaves none of it.
 *
 * And the ink in the region has to be *the words'*. All four of those are
 * statistics about colour, and colour statistics can be manufactured without
 * drawing a single glyph:
 *
 *   <p style="-webkit-text-fill-color:transparent;
 *             background:repeating-linear-gradient(#9a6700 0 1px,#fff8c5 1px 4px)">
 *
 * The expected ink is there, in several colours, over well under three quarters
 * of the region — and there are no words. So the words' own colour is taken
 * away and the region is read again: `color:transparent` on the very element
 * under test changes nothing about the layout, so this is not the geometry
 * comparison this suite rejects elsewhere. Real type disappears and the pixels
 * move; a decoy painted behind it is unmoved, because it never depended on the
 * colour the words are written in.
 *
 * The region is the words' own, not the element's, which is what makes the
 * middle three answerable: measured over a whole box, flatness is satisfied by
 * anything else the box happens to contain, and ink-present by a stripe of the
 * right colour painted anywhere in the padding.
 */
async function legible(page, locator, ink) {
  const seen = await locator.evaluate((node) => [
    window.bureauDrawn.rendered(node),
    window.bureauDrawn.inkRegion(node),
  ]);
  const [rendered, region] = seen;
  if (!rendered || region === null) {
    return [false, false, false, false, false];
  }
  const inked = await census(page, locator, region, ink);
  const muted = await withoutInk(page, locator, () => census(page, locator, region, ink));
  return [true, inked.share > 0, inked.distinct > 1, inked.share < SOLID, muted.signature !== inked.signature];
}

/** One region of `locator`, as Chromium painted it. */
async function census(page, locator, region, ink) {
  const png = await locator.screenshot();
  return page.evaluate(
    ([data, part, colour]) => window.bureauDrawn.paint(data, part, colour),
    [png.toString("base64"), region, ink],
  );
}

/**
 * Runs `read` with the words in `locator` written in nothing.
 *
 * The rule is `!important` and set on the element itself and its two
 * pseudo-elements, because the phrase this is asked of is CSS `content` and the
 * notices' is markup — one sheet has to reach both. It is removed afterwards
 * whatever happens, so a failure here cannot leave the page altered for the
 * next question asked of it.
 */
async function withoutInk(page, locator, read) {
  const sheet = await locator.evaluate((node) => {
    node.dataset.muted = "";
    const style = document.createElement("style");
    style.textContent = "[data-muted],[data-muted] *,[data-muted]::before,[data-muted]::after,[data-muted] *::before,[data-muted] *::after"
      + "{ color:transparent !important; -webkit-text-fill-color:transparent !important; }";
    document.head.append(style);
    return true;
  });
  try {
    return await read();
  } finally {
    await locator.evaluate((node) => {
      delete node.dataset.muted;
      document.head.querySelectorAll("style").forEach((style) => {
        if (style.textContent.includes("[data-muted]")) {
          style.remove();
        }
      });
    }, sheet);
  }
}

/** The element a figure's mark is written into, empty until one is. */
function markSlot(page, name) {
  return page.locator(`figure[data-shot="${name}"] figcaption .${SETTLED_SLOT}`);
}

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
 * nothing at all, and an alpha channel is only the first of the ways: beside
 * `border-color:transparent` sit `border-style:none`, which computes the width
 * to zero, and `display:none`, `font-size:0` and `clip-path:inset(100%)` on the
 * caption's `::after`, each of which leaves the phrase present in `content` at
 * full alpha and draws not one pixel. The list of ways to be invisible has no
 * end, so no list of properties closes it.
 *
 * So the mark is asked of the render instead. Each of the two channels the
 * stylesheet paints is screenshotted and read back for the colours Chromium
 * actually put there, and `SETTLED_INK` — the one the sheet interpolates — has
 * to be among them: on the stamped figure's border, on its caption, and on
 * neither channel of any figure that was not stamped. That is a question a rule
 * painting nothing cannot pass, whichever property it was silenced with, and it
 * is per-channel because a mark half drawn is a mark that lies.
 *
 * Comparing the stamped render against an unstamped one is deliberately *not*
 * the question. The mark changes the border's width, so the geometry moves and
 * the pixels differ whether or not anything was painted — `font-size:0` on the
 * `::after` still nudges the caption's line box, and a difference check reads
 * that as a mark. Ink present is about paint alone.
 *
 * `drawn.js` decides what reading the render means, once, for this check and for
 * the notices below, so the two cannot drift into asking different questions
 * about the same page.
 */
/*
 * And which figures wear it was read from one of them.
 *
 * `document.querySelector("figure[data-settled]")` is the first stamped figure,
 * and the control was every figure *without* the attribute — so a mark landing
 * on figures the run never asked to mark simply moved them out of the control
 * group. Stamping every compact render, one line in `figureTag`, left the
 * stamped figure saying it, no unstamped figure saying it, and half the gallery
 * telling a reviewer it was not proved settled. A set is the claim: exactly the
 * shots handed to `applyMarks`, and no others.
 *
 * The attribute being exact is still not the mark being exact, because the mark
 * is what a reviewer *sees*. Two rules keyed on position rather than on the
 * attribute leave the set perfectly correct:
 *
 *   .card::after { content:"not proved settled"; color:#9a6700 }
 *   .shots figure:last-of-type:not([data-settled]) img { border-color:#9a6700 }
 *
 * The first says it outside every figure, where a figure-scoped read cannot
 * see it; the second draws the amber border on a figure no sample happened to
 * take. So the phrase is counted over the whole document — every element and
 * both pseudo-elements of each — and the ink is swept across *every* figure's
 * two channels from a single full-page shot, rather than sampled from two.
 *
 * Both channels are asked separately, because a mark half drawn is a mark that
 * lies: collapsing them to "either one carries ink" approves
 *
 *   figure[data-settled="false"] img { border-color:#9a6700; border-style:none }
 *
 * on the strength of the caption alone, with no amber anywhere near the render
 * it judges. And a box is not the only place ink can land, so the sweep also
 * reports the ink it finds *outside* every channel: an `outline` at an offset,
 * or a generated block below the caption, paints the mark's own colour on
 * unstamped figures without touching a single sampled rectangle.
 *
 * The page is built from a slice of the registry rather than all of it, which
 * is what makes "every figure" affordable: what is under test is the marker and
 * the stylesheet, and neither has any idea which states it was handed.
 */
const MARKED_PAGE = STATES.slice(0, 8);

test("@matrix an unsettled figure is drawn unlike a settled one", async ({ page }) => {
  const target = shot(MARKED_PAGE[0], VIEWPORT_LIST[VIEWPORT_LIST.length - 1]);
  const marked = applyMarks(indexPage(rowsFor(MARKED_PAGE, VIEWPORT_LIST, shot), MARKED_PAGE, VIEWPORT_LIST), "", [target]);

  await page.setContent(marked.html);
  await page.addScriptTag({ path: DRAWN });

  const said = await page.evaluate((phrase) => [
    [...document.querySelectorAll("figure[data-settled]")].map((figure) => figure.dataset.shot),
    window.bureauDrawn.saying(document.body, phrase),
    document.querySelectorAll("figure").length,
  ], SETTLED_PHRASE);
  const figures = await page.evaluate(() => window.bureauDrawn.channels("figure"));
  const full = await page.screenshot({ fullPage: true });
  const regions = figures.flatMap((figure) => figure.regions);
  const swept = await page.evaluate(
    ([data, boxes, colour]) => window.bureauDrawn.sweep(data, boxes, colour),
    [full.toString("base64"), regions, inkOf(SETTLED_INK)],
  );
  // Absent evidence is not clean evidence. `wrong` reads each figure's verdicts
  // by position, so a `channels` that returns one figure fewer, or a `sweep`
  // that returns fewer verdicts than it was given regions, leaves the tail
  // comparing an empty slice — and `.some` of nothing is `false`, which is
  // indistinguishable here from a figure that was checked and found right.
  // Both are instruments this very test exists to hold, so what they owe is
  // counted before any of it is believed.
  const drawn = MARKED_PAGE.length * VIEWPORT_LIST.length;
  const counted = [figures.length, regions.length, swept.found.length];
  const wrong = figures.filter((figure, at) => swept.found.slice(at * 2, at * 2 + 2).some((seen) => seen !== figure.marked));
  const read = [];
  for (const name of [target, shot(MARKED_PAGE[1], VIEWPORT_LIST[0])]) {
    read.push(await legible(page, markSlot(page, name), inkOf(SETTLED_INK)));
  }

  expect([said, counted, wrong.map((figure) => figure.shot), swept.stray, read], "exactly the stamped figure wears the mark, in ink a reviewer can read").toEqual([
    [[target], 1, drawn],
    [drawn, drawn * 2, drawn * 2],
    [],
    0,
    [[true, true, true, true, true], [false, false, false, false, false]],
  ]);
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
 *
 * And, like the mark, being *present* was still all that was asked of the words
 * themselves. A notice at `color:transparent`, or one whose foreground is set to
 * its own background — `background:#ffebe9;color:#ffebe9` — is in the render
 * tree, passes `checkVisibility`, serializes identically, and cannot be read.
 * So each notice is screenshotted and its pixels are asked four things: it is
 * rendered at all, its own computed ink is among the colours actually painted,
 * the region is not one flat colour, and the ink does not fill it.
 *
 * The four are needed together and each closes what the others do not.
 * `opacity:0` is caught only by the first, `color:transparent` (which never
 * appears in an opaque screenshot) only by the second, and a foreground equal
 * to its background only by the last — that ink genuinely *is* painted, and a
 * block of it is not flat either, because its edges blend into what is behind
 * them.
 *
 * Those three were asked of the banner's whole box, and a box is bigger than
 * its words. `font-size:0` leaves a `<p>` its padding, so
 * `background:linear-gradient(currentColor 2px,#fff8c5 2px)` puts the notice's
 * own ink on the page, in two colours, over a bar with no legible glyph in it —
 * every one of the three green, and a reviewer told nothing. `legible` reads
 * the rectangles the *text* occupies instead, so a stripe in the padding is
 * outside the question and type with no size has no region to ask about.
 */
const NOTICE_CASES = [
  { id: "an advisory alone", lines: [], missing: [], drawn: 1 },
  { id: "an alarm above an advisory", lines: ["a hole"], missing: ["a state @ desktop"], drawn: 2 },
];

test("@matrix a run's notices are drawn where a reviewer will read them", async ({ page }) => {
  const target = shot(MARKED_PAGE[0], VIEWPORT_LIST[VIEWPORT_LIST.length - 1]);
  const page1 = indexPage(rowsFor(MARKED_PAGE, VIEWPORT_LIST, shot), MARKED_PAGE, VIEWPORT_LIST);
  const read = [];

  for (const found of NOTICE_CASES) {
    await page.setContent(applyMarks(page1, notices(found.lines, found.missing, [target], [], []), [target]).html);
    await page.addScriptTag({ path: DRAWN });
    const said = await page.evaluate((phrase) => {
      const notes = [...document.querySelectorAll("body > p")];
      return [
        notes.length,
        notes.every((note) => window.bureauDrawn.rendered(note)),
        notes.filter((note) => note.textContent.includes(phrase)).length,
        notes.map((note) => `${getComputedStyle(note).color.match(/[\d.]+/gu).slice(0, 3).join(",")},255`),
      ];
    }, SETTLED_PHRASE);
    const seen = [];
    for (const [at, ink] of said[3].entries()) {
      // Serial, not `Promise.all`: `legible` takes the words' colour away
      // while it reads, and two of these overlapping would each be reading the
      // page the other had muted.
      seen.push(await legible(page, page.locator("body > p").nth(at), ink));
    }
    const promise = await legible(page, page.getByText(SETTLED_PHRASE, { exact: true }), inkOf(SETTLED_INK));
    read.push([...said.slice(0, 3), seen.every((note) => note.every(Boolean)), promise.every(Boolean)]);
  }

  expect(read, "every notice a run writes is painted in ink a reviewer can read, and says what it promises").toEqual(NOTICE_CASES.map((found) => [found.drawn, true, 1, true, true]));
});
