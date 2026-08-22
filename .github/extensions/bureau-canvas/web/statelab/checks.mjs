// What "the render matched the registry" means, in one place.
//
// `collect` runs inside the page. It is deliberately self-contained — no
// imports, no closure over module scope — because Playwright serialises it
// into the browser while the lab calls it directly against an iframe
// document. `verdict` is pure and runs outside the page, so the offline suite
// can test the judgement without a browser at all.

/**
 * Everything a verdict needs, gathered in one pass so the page is measured at
 * a single moment rather than across a dozen round trips.
 */
export function collect(doc, request) {
  const visible = (node) => node.getClientRects().length > 0;
  const counts = {};
  for (const selector of request.selectors) {
    counts[selector] = [...doc.querySelectorAll(selector)].filter(visible).length;
  }
  const root = doc.documentElement;
  const boxes = [];
  for (const selector of request.measure) {
    for (const node of doc.querySelectorAll(selector)) {
      const rect = node.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        boxes.push({ selector, x: rect.x, y: rect.y, width: rect.width, height: rect.height });
      }
    }
  }
  const view = doc.defaultView;
  const channels = (value) => (String(value).match(/[\d.]+/gu) ?? []).map(Number);
  const opaque = (value) => {
    const parts = channels(value);
    return parts.length >= 3 && (parts.length < 4 || parts[3] > 0);
  };
  const luminance = (value) => {
    const [red, green, blue] = channels(value);
    const linear = [red, green, blue].map((part) => {
      const ratio = part / 255;
      return ratio <= 0.03928 ? ratio / 12.92 : ((ratio + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
  };
  const backdrop = (node) => {
    for (let item = node; item; item = item.parentElement) {
      const fill = view.getComputedStyle(item).backgroundColor;
      if (opaque(fill)) {
        return fill;
      }
    }
    return "rgb(255, 255, 255)";
  };
  const contrast = [];
  for (const selector of request.contrast ?? []) {
    for (const node of doc.querySelectorAll(selector)) {
      if (!visible(node) || !node.textContent.trim()) {
        continue;
      }
      const front = luminance(view.getComputedStyle(node).color);
      const back = luminance(backdrop(node));
      const ratio = (Math.max(front, back) + 0.05) / (Math.min(front, back) + 0.05);
      contrast.push({ selector, text: node.textContent.trim().slice(0, 40), ratio: Number(ratio.toFixed(2)) });
    }
  }
  return {
    counts,
    boxes,
    contrast,
    text: doc.body ? doc.body.innerText : "",
    overflowX: root.scrollWidth - root.clientWidth,
    viewport: { width: root.clientWidth, height: root.clientHeight },
  };
}

/** Selectors whose geometry the overlap and clipping checks care about. */
export const MEASURED = [
  ".assignment-card",
  ".draft-bar",
  ".app-header",
  ".create-bar",
  ".general-findings",
  ".orphan-strip",
  ".detail-row",
  ".limit-row",
  ".repo-row",
  ".editor-toolbar",
  ".editor-panel",
  ".pipeline-toolbar",
];

/** Regions that stack vertically and must never sit on top of one another. */
const STACKED = [".assignment-card", ".detail-row", ".limit-row", ".repo-row"];

/**
 * Regions that are siblings in the landing's single column. None of these
 * nests inside another — the draft bar, the findings strip and the create form
 * are drawn above the stack, and the orphan strip below it — so any
 * intersection between a pair is one region printing over another.
 *
 * This is what the scoping probes are for. Each `scoping` rule claims two axes
 * share no React state, which is true and says nothing about layout: the
 * crossings still land in one column. Comparing only same-selector pairs would
 * have let a draft bar cover an assignment card without a word.
 */
const SIBLINGS = [
  [".draft-bar", ".assignment-card"],
  [".general-findings", ".assignment-card"],
  [".create-bar", ".assignment-card"],
  [".orphan-strip", ".assignment-card"],
  [".draft-bar", ".general-findings"],
  [".draft-bar", ".create-bar"],
  [".general-findings", ".create-bar"],
];

const OVERLAP_TOLERANCE = 1;

/**
 * Text whose colour must stand off what it sits on.
 *
 * These are the labels that take their colour from a kind hue rather than from
 * the ink token: a badge filled from `--accent`, and the access tags that
 * paint a repo's grant in its own hue. Both are small text, so the hue has to
 * carry contrast as well as meaning — a hue retune that reads as decoration on
 * white is a defect the screenshots would not show.
 */
export const CONTRAST = [".kind-label", ".access"];

/**
 * WCAG AA for normal text. Neither selector is "large text" — that needs
 * 18.66px bold or 24px, and these are 12px — so the 3:1 large-text allowance
 * does not apply to them.
 */
const MIN_CONTRAST = 4.5;

export function verdict(state, snapshot, options = {}) {
  return [
    ...missing(state, snapshot),
    ...forbidden(state, snapshot),
    ...absentCopy(state, snapshot),
    ...lowContrast(snapshot, options),
    ...overlaps(snapshot),
    ...clipping(snapshot, options),
  ];
}

function lowContrast(snapshot, options) {
  const floor = options.minContrast ?? MIN_CONTRAST;
  return (snapshot.contrast ?? [])
    .filter((item) => item.ratio < floor)
    .map((item) => ({
      kind: "low-contrast",
      detail: `${item.selector} draws "${item.text}" at ${item.ratio}:1 against its background`,
    }));
}

function missing(state, snapshot) {
  return (state.expect.shows ?? [])
    .filter((selector) => !snapshot.counts[selector])
    .map((selector) => ({ kind: "missing-control", detail: selector }));
}

function forbidden(state, snapshot) {
  return (state.expect.hides ?? [])
    .filter((selector) => snapshot.counts[selector] > 0)
    .map((selector) => ({ kind: "unexpected-control", detail: selector }));
}

function absentCopy(state, snapshot) {
  const text = normalise(snapshot.text);
  return (state.expect.copy ?? [])
    .filter((phrase) => !text.includes(normalise(phrase)))
    .map((phrase) => ({ kind: "missing-copy", detail: phrase }));
}

/**
 * Copy is compared case-insensitively: the design system's Label rule
 * uppercases group and rail headings in CSS, and `innerText` reports text as
 * rendered. The registry records the copy as it is authored.
 */
function normalise(value) {
  return String(value ?? "")
    .replace(/\s+/gu, " ")
    .replace(/[\u2010-\u2015]/gu, "-")
    .trim()
    .toLowerCase();
}

/**
 * Two boxes drawn by the same kind of region must not overlap. Nesting is
 * fine — a detail row lives inside a card — so same-selector pairs are
 * compared, which is exactly the "cards overprinting each other" defect the
 * browser suite has caught before, plus the sibling pairs above, which is the
 * defect a crossing probe is rendered to find.
 */
function overlaps(snapshot) {
  return [...sameKind(snapshot), ...siblingKinds(snapshot)];
}

function sameKind(snapshot) {
  const found = [];
  for (const selector of STACKED) {
    const boxes = snapshot.boxes.filter((box) => box.selector === selector);
    for (let left = 0; left < boxes.length; left += 1) {
      for (let right = left + 1; right < boxes.length; right += 1) {
        if (intersects(boxes[left], boxes[right])) {
          found.push({ kind: "overlap", detail: `${selector} #${left} overlaps #${right}` });
        }
      }
    }
  }
  return found;
}

function siblingKinds(snapshot) {
  const found = [];
  for (const [one, other] of SIBLINGS) {
    for (const left of snapshot.boxes.filter((box) => box.selector === one)) {
      for (const right of snapshot.boxes.filter((box) => box.selector === other)) {
        if (intersects(left, right)) {
          found.push({ kind: "overlap", detail: `${one} overlaps ${other}` });
        }
      }
    }
  }
  return found;
}

function intersects(left, right) {
  const overlapX = Math.min(left.x + left.width, right.x + right.width) - Math.max(left.x, right.x);
  const overlapY = Math.min(left.y + left.height, right.y + right.height) - Math.max(left.y, right.y);
  return overlapX > OVERLAP_TOLERANCE && overlapY > OVERLAP_TOLERANCE;
}

/**
 * Nothing may need a horizontal scrollbar, and no measured region may hang off
 * the right edge. Both are how a compact viewport breaks in practice.
 */
function clipping(snapshot, options) {
  const slack = options.slack ?? 2;
  const problems = [];
  if (snapshot.overflowX > slack) {
    problems.push({ kind: "horizontal-overflow", detail: `${snapshot.overflowX}px wider than the viewport` });
  }
  for (const box of snapshot.boxes) {
    if (box.x + box.width > snapshot.viewport.width + slack) {
      problems.push({ kind: "clipped", detail: `${box.selector} extends ${Math.round(box.x + box.width - snapshot.viewport.width)}px past the viewport` });
    }
  }
  return problems;
}

/** Every selector a state mentions, so one collect pass covers the verdict. */
export function selectorsFor(state) {
  return [...new Set([...(state.expect.shows ?? []), ...(state.expect.hides ?? [])])];
}
