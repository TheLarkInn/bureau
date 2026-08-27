// What "drawn" means to a reviewer, decided once, inside the page.
//
// The gallery's browser checks in `specs/state-matrix.spec.mjs` ask whether a
// mark or a notice is something a human will actually meet. Presence is not
// that question, and neither is a computed value being readable: a rule can be
// present, correctly spelled, matching the right element, and paint nothing.
//
//   figure[data-settled="false"] img { border-color:transparent; }
//   figure[data-settled="false"] figcaption::after { color:transparent; }
//
// Both leave every selector matching and every `getComputedStyle` read
// answering exactly what a check comparing borders and reading `content`
// expects — while all 512 renders tell a reviewer nothing at all. That is the
// same defect as a selector that drifted, one notch further along: the mark is
// attached, the rule is found, and the page is silent. `hidden`, `display:none`
// and `opacity:0` are the same defect again on the notice a bad run depends on.
//
// This is a script injected into the page rather than two copies inside two
// `page.evaluate` bodies because a rule spelled twice is the defect this suite
// exists to remove. While each check carried its own colour parser, changing
// what counts as drawn in one left the other still asking the old question,
// green over exactly the page the change was made to catch.

window.bureauDrawn = {
  /**
   * The alpha channel of a computed colour.
   *
   * `transparent` computes to `rgba(0, 0, 0, 0)`, so a four-part colour is the
   * only one that can be invisible; `rgb(…)` is always fully opaque.
   */
  alpha(color) {
    const parts = String(color).match(/[\d.]+/gu) ?? [];
    return parts.length === 4 ? Number(parts[3]) : 1;
  },

  /** Rendered at all, and not painted in nothing. */
  visible(node) {
    return node.checkVisibility({ opacityProperty: true, visibilityProperty: true })
      && window.bureauDrawn.alpha(getComputedStyle(node).color) > 0;
  },

  /**
   * The alpha of every channel of `node`'s subtree that says `phrase`.
   *
   * Every channel, because the sentence is CSS `content` here and nothing makes
   * it so: ordinary markup inside the caption says the same words where a
   * pseudo-element read cannot see them, and a pseudo-element on any descendant
   * says them where a caption read cannot. An empty result means the words are
   * not on the page; a zero in it means they are on the page and invisible.
   */
  saying(node, phrase) {
    return [node, ...node.querySelectorAll("*")]
      .flatMap((element) => [undefined, "::before", "::after"].map((pseudo) => [element, pseudo]))
      .filter(([element, pseudo]) => String(pseudo ? getComputedStyle(element, pseudo).content : element.textContent).includes(phrase))
      .map(([element, pseudo]) => window.bureauDrawn.alpha(getComputedStyle(element, pseudo).color));
  },
};
