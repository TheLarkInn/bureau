// What "drawn" means to a reviewer, decided once, inside the page.
//
// The gallery's browser checks in `specs/state-matrix.spec.mjs` ask whether a
// mark or a notice is something a human will actually meet. Presence is not
// that question, and neither is a computed value being readable: a rule can be
// present, correctly spelled, matching the right element, and paint nothing.
//
// This file used to answer it by reading properties — `checkVisibility`, plus
// the alpha channel of a computed colour — and that was the same defect one
// notch along, because the list of ways to be invisible has no end:
//
//   figure[data-settled="false"] img { border-color:#9a6700; border-style:none }
//   figure[data-settled="false"] figcaption::after { color:#9a6700; display:none }
//
// Both leave every selector matching, the border colour differing from every
// neighbour's and the phrase present in `content` at full alpha — and neither
// paints a single pixel. `font-size:0`, `clip-path:inset(100%)`,
// `text-indent:-9999px`, and a foreground that equals its own background are
// the same defect again, and no list of properties closes them all.
//
// So the question is asked of the pixels. `colours` reads back a screenshot of
// the page and reports the distinct colours in it, which is the one answer none
// of those mutations survives: a mark that paints nothing puts none of its own
// ink on the page, and a notice whose words are unreadable leaves a region of
// one flat colour. Everything above is a property; this is the render.
//
// And *which* region is read is half of that question. Asking it of an
// element's whole box let two more mutations through, both of which leave a
// reviewer a blank bar:
//
//   <p style="font-size:0;background:linear-gradient(currentColor 2px,#fff8c5 2px)">
//   figure[data-settled="false"] figcaption .mark::after { background:currentColor }
//
// The first paints its own ink in a two-pixel stripe of padding the words are
// not in, so ink-present and not-flat both pass over a paragraph with no
// legible glyph in it. The second makes the phrase its own background, which is
// flat — but the caption's box also holds the viewport name in another colour,
// so flatness measured over the box is satisfied by a neighbour of the thing
// under test. Both are answered by reading the region the *words* occupy:
// `inkRegion` returns the union of a node's own text rectangles, or — for a
// phrase written as pseudo-content, which has no text node to select — the box
// of the element that exists to hold it, and nothing when that comes to no
// area at all. A region a reviewer's eye cannot land on is not a region to ask
// about paint.
//
// Flatness alone did not close the second of those even then, and that is worth
// recording because it looks as though it should. A block of solid ink is one
// colour in the middle and a fringe of blends at its edges, so a phrase painted
// over its own background answers with *two* colours and passes a test for
// "more than one". What separates words from a block is not how many colours
// are in the region but how much of it the ink covers: glyphs are ink on a
// ground and leave most of their line box unpainted, and a block leaves none of
// it. So `paint` reports the ink's share of the region as well as the count,
// and both ends of that share are the question — nothing painted, and
// everything painted, are two ways to say the same unreadable thing.
//
// It is a script injected into the page rather than a copy inside each
// `page.evaluate` body because a rule spelled twice is the defect this suite
// exists to remove. While each check carried its own reading of "drawn",
// changing what counts in one left the other still asking the old question,
// green over exactly the page the change was made to catch.

/**
 * Every pixel of one region of a screenshot, counted by colour.
 *
 * The region is given in fractions of the shot rather than in pixels so that
 * neither this nor its caller has to know the device pixel ratio it was taken
 * at — a scale mismatch would silently read the wrong rectangle, which is the
 * same defect as reading the wrong element.
 */
async function census(base64, region) {
  const blob = await (await fetch(`data:image/png;base64,${base64}`)).blob();
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.drawImage(bitmap, 0, 0);
  const box = region
    ? [
      Math.floor(region.x * bitmap.width),
      Math.floor(region.y * bitmap.height),
      Math.max(1, Math.round(region.width * bitmap.width)),
      Math.max(1, Math.round(region.height * bitmap.height)),
    ]
    : [0, 0, bitmap.width, bitmap.height];
  const { data } = context.getImageData(...box);
  const counted = new Map();
  for (let at = 0; at < data.length; at += 4) {
    const colour = `${data[at]},${data[at + 1]},${data[at + 2]},${data[at + 3]}`;
    counted.set(colour, (counted.get(colour) ?? 0) + 1);
  }
  return { counted, total: data.length / 4 };
}

window.bureauDrawn = {
  /**
   * The distinct colours of a PNG, as `"r,g,b,a"` strings.
   *
   * The image is a screenshot Playwright took of this page and handed straight
   * back, so what is measured is what Chromium painted rather than what the
   * cascade says it should have. A mark that paints nothing leaves its own ink
   * out of the answer entirely.
   */
  async colours(base64, region) {
    return [...(await census(base64, region)).counted.keys()];
  },

  /**
   * How varied one region is, and how much of it `ink` covers.
   *
   * The share is what tells words from a block. A region of solid ink is not
   * one colour — its edges blend into whatever is behind it — so counting
   * colours calls it varied and passes it. Glyphs leave most of their own line
   * box unpainted; a block leaves none of it.
   */
  async paint(base64, region, ink) {
    const { counted, total } = await census(base64, region);
    return { distinct: counted.size, share: (counted.get(ink) ?? 0) / total };
  },

  /**
   * Where `node`'s words are, as fractions of `node`'s own box — or `null` when
   * they are nowhere.
   *
   * A node that holds text is measured by that text's own rectangles, so the
   * padding around it is not part of the answer: a stripe painted in the margin
   * of a paragraph whose type is zero-sized is ink on the page and no ink on
   * the words, and only this distinction separates them. A node that holds no
   * text is measured by its own box, which is the case the mark's phrase is in
   * — it is CSS `content`, and there is no text node to select — and is why the
   * caption carries an otherwise empty element for the phrase to be written
   * into: pseudo-content has no box of its own to ask about, and the box of the
   * caption is shared with the viewport name.
   *
   * `null` where there is no area is the finding, not an error: `font-size:0`,
   * `display:none` on the pseudo-element and an empty slot all arrive here, and
   * each of them means a reviewer meets nothing.
   */
  inkRegion(node) {
    const own = node.getBoundingClientRect();
    if (!own.width || !own.height) {
      return null;
    }
    const range = document.createRange();
    range.selectNodeContents(node);
    const holdsText = document.createTreeWalker(node, NodeFilter.SHOW_TEXT, {
      acceptNode: (text) => (text.data.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
    }).nextNode() !== null;
    const rects = [...range.getClientRects()].filter((rect) => rect.width > 0 && rect.height > 0);
    if (holdsText && !rects.length) {
      return null;
    }
    const words = holdsText ? rects : [own];
    const left = Math.min(...words.map((rect) => rect.left));
    const top = Math.min(...words.map((rect) => rect.top));
    const right = Math.max(...words.map((rect) => rect.right));
    const bottom = Math.max(...words.map((rect) => rect.bottom));
    return {
      x: (left - own.left) / own.width,
      y: (top - own.top) / own.height,
      width: (right - left) / own.width,
      height: (bottom - top) / own.height,
    };
  },

  /**
   * Whether `node` is in the render tree at all.
   *
   * Kept beside the pixel reads because it names its own failure: an element
   * that is `hidden` or `display:none` has no box, and a screenshot of it is an
   * error about geometry rather than the finding, which is that a reviewer
   * cannot see the thing. This says so first.
   */
  rendered(node) {
    return node.checkVisibility({ opacityProperty: true, visibilityProperty: true });
  },

  /**
   * How many channels of `node`'s subtree say `phrase`.
   *
   * Structural on purpose: this answers *where the words are* and nothing about
   * whether they are legible — that is the pixels' question, asked separately.
   * Every channel is counted, because the sentence is CSS `content` here and
   * nothing makes it so: ordinary markup inside the caption says the same words
   * where a pseudo-element read cannot see them, and a pseudo-element on any
   * descendant says them where a caption read cannot.
   */
  saying(node, phrase) {
    return [node, ...node.querySelectorAll("*")]
      .flatMap((element) => [undefined, "::before", "::after"].map((pseudo) => [element, pseudo]))
      .filter(([element, pseudo]) => String(pseudo ? getComputedStyle(element, pseudo).content : element.textContent).includes(phrase))
      .length;
  },
};
