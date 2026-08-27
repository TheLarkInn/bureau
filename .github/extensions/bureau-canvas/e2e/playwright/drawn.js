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
// It is a script injected into the page rather than a copy inside each
// `page.evaluate` body because a rule spelled twice is the defect this suite
// exists to remove. While each check carried its own reading of "drawn",
// changing what counts in one left the other still asking the old question,
// green over exactly the page the change was made to catch.

window.bureauDrawn = {
  /**
   * The distinct colours of a PNG, as `"r,g,b,a"` strings.
   *
   * The image is a screenshot Playwright took of this page and handed straight
   * back, so what is measured is what Chromium painted rather than what the
   * cascade says it should have. A region drawn in one flat colour answers with
   * a single entry, which is what an unreadable notice is; a mark that paints
   * nothing leaves its own ink out of the answer entirely.
   */
  async colours(base64) {
    const blob = await (await fetch(`data:image/png;base64,${base64}`)).blob();
    const bitmap = await createImageBitmap(blob);
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.drawImage(bitmap, 0, 0);
    const { data } = context.getImageData(0, 0, bitmap.width, bitmap.height);
    const seen = new Set();
    for (let at = 0; at < data.length; at += 4) {
      seen.add(`${data[at]},${data[at + 1]},${data[at + 2]},${data[at + 3]}`);
    }
    return [...seen];
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
