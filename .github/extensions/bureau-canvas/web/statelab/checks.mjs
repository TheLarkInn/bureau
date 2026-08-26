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
  const view = doc.defaultView;
  // A box is not the same as a painted pixel: `visibility: hidden` still
  // reports rects, and that is precisely how a React Flow node looks before it
  // has been measured — the blank graph `graph-measure.mjs` exists to repair.
  // Counting one would let an unpainted graph satisfy a `shows`.
  //
  // Opacity is the other way a promised control measures perfectly and paints
  // nothing, and it needs a different walk: `visibility` inherits, so the
  // node's own computed value already answers for its ancestors, but opacity
  // does not — a fully transparent parent leaves the child's computed opacity
  // at 1. Reading the node alone therefore let `opacity: 0` satisfy every
  // `shows` and every scoped `copy` in this registry, which is the one failure
  // a review surface may not have: the gallery would show the control gone and
  // the matrix would call the state correct.
  const painted = (node) => {
    for (let item = node; item; item = item.parentElement) {
      if (Number.parseFloat(view.getComputedStyle(item).opacity) === 0) {
        return false;
      }
    }
    return true;
  };
  // Having boxes is not the same as covering pixels. `getClientRects` answers
  // with a rect for a control collapsed to nothing — `width: 0`, a flex child
  // squeezed to zero, `transform: scale(0)` — so counting rects rather than
  // area let a control with no area on screen satisfy every `shows`, which is
  // the same lie `opacity: 0` told and the same lie `visibility: hidden` told
  // before it. An inline run wraps into several rects, so it is enough for one
  // of them to have area; requiring all of them would fail an ordinary
  // line-broken label.
  const boxed = (node) => [...node.getClientRects()].some((rect) => rect.width > 0 && rect.height > 0);
  const visible = (node) =>
    boxed(node)
    && view.getComputedStyle(node).visibility !== "hidden"
    && painted(node);
  const counts = {};
  const texts = {};
  for (const selector of request.selectors) {
    const found = [...doc.querySelectorAll(selector)].filter(visible);
    counts[selector] = found.length;
    texts[selector] = found.map((node) => node.innerText ?? node.textContent ?? "").join(" ");
  }
  const root = doc.documentElement;
  const boxes = [];
  // Parent identity, assigned inside the page: two boxes are siblings when
  // they answer to the same key. This is what lets the overlap check be a rule
  // ("nothing in normal flow may print over its own sibling") rather than a
  // hand-kept list of pairs someone has to remember to extend.
  const parents = new Map();
  const keyFor = (node) => {
    if (!parents.has(node)) {
      parents.set(node, `parent-${parents.size}`);
    }
    return parents.get(node);
  };
  // Element identity, for the same reason but one level down. `measure` now
  // carries the state's own `shows`, and those overlap the standing list and
  // each other — `[data-testid="limits-save"]` and the same selector with
  // `:not([disabled])` are two selectors naming one button. Without identity
  // that button is two boxes at identical coordinates, and the sibling rule
  // reports it as printing over itself.
  const nodes = new Map();
  const idFor = (node) => {
    if (!nodes.has(node)) {
      nodes.set(node, `node-${nodes.size}`);
    }
    return nodes.get(node);
  };
  // The nearest ancestor that clips, per axis. A control can report a
  // perfectly good box and still be invisible, because an `overflow: hidden`
  // parent cuts it away; `getClientRects` knows nothing about that, so a
  // `shows` would pass on a control the user cannot see.
  //
  // `auto` and `scroll` are deliberately *not* clipping. Content below the
  // fold of a scroll container is one gesture away, not lost — the editor's
  // side panel is `overflow-y: auto`, and treating that as clipping reported
  // its issue list as cut away on every editor state that had one. The axes
  // are tracked apart for the same reason: `overflow-x: hidden` with a
  // scrolling y is an ordinary vertical scroller, not a lid.
  //
  // Apart means apart all the way up, too. Stopping the search at the first
  // ancestor that clips *either* axis read that one element's overflow on both
  // axes and never looked again — so an ellipsis wrapper (`overflow-x: hidden`,
  // y visible) nested inside a genuinely lidded box answered "nothing clips y"
  // for its whole subtree, and a control cut away vertically reported a clean
  // box. Each axis now keeps looking until it finds its own clipper.
  const clipper = (node) => {
    let x = null;
    let y = null;
    let pannable = false;
    for (let item = node.parentElement; item; item = item.parentElement) {
      const style = view.getComputedStyle(item);
      if (!x && /hidden|clip/u.test(style.overflowX)) {
        x = item.getBoundingClientRect();
      }
      if (!y && /hidden|clip/u.test(style.overflowY)) {
        y = item.getBoundingClientRect();
      }
      // Graph content pans: a step card half out of frame is the surface
      // working, and the reader drags it back. Ordinary controls have no such
      // gesture, so for them any trim is text or a click target the user
      // cannot reach. The walk continues past the clipper because the pannable
      // surface is the graph root, above the pane that does the clipping.
      if (/(^|\s)react-flow(\s|$)/u.test(item.getAttribute("class") ?? "")) {
        pannable = true;
      }
    }
    return { x, y, pannable };
  };
  // Measured elements are collected before they are described, so each box can
  // name the measured boxes that *contain* it. `sameKind` compares boxes drawn
  // by the same selector, and that comparison assumed a selector never nests
  // inside itself — which is false: the repo adder's resolved preview draws a
  // `.detail-row` per field inside the `.detail-row` that holds the whole repos
  // field. Containment is not overprinting, and without this the checker
  // reported five overlaps on a screen that renders perfectly.
  const measured = [];
  for (const selector of request.measure) {
    for (const node of doc.querySelectorAll(selector)) {
      const rect = node.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        measured.push({ node, selector, rect, id: idFor(node) });
      }
    }
  }
  for (const { node, selector, rect, id } of measured) {
    const position = view.getComputedStyle(node).position;
    const clip = clipper(node);
    const outsideX = clip.x && (rect.x >= clip.x.right || rect.x + rect.width <= clip.x.left);
    const outsideY = clip.y && (rect.y >= clip.y.bottom || rect.y + rect.height <= clip.y.top);
    const trimX = clip.x ? Math.max(clip.x.left - rect.x, rect.x + rect.width - clip.x.right, 0) : 0;
    const trimY = clip.y ? Math.max(clip.y.top - rect.y, rect.y + rect.height - clip.y.bottom, 0) : 0;
    boxes.push({
      selector,
      id,
      x: rect.x, y: rect.y, width: rect.width, height: rect.height,
      parent: node.parentElement ? keyFor(node.parentElement) : null,
      // The measured boxes this one sits inside, by id.
      within: measured.filter((other) => other.node !== node && other.node.contains(node)).map((other) => other.id),
      // Absolute and fixed boxes overlap on purpose — a badge over a card,
      // a minimap over a graph — so only normal-flow boxes are held to the
      // sibling rule.
      flow: position === "static" || position === "relative",
      // Entirely outside its clipper: gone, whatever surface it is on.
      clipped: Boolean(outsideX || outsideY),
      // How far a clipping ancestor cuts into it, judged only off a
      // pannable surface, where nothing brings the rest into view.
      trimmed: clip.pannable ? 0 : Math.round(Math.max(trimX, trimY)),
    });
  }
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
  // Every `label[for]` beside the control it names, as two boxes. Gathered
  // here rather than derived from `boxes` because the pairing is the point: a
  // label and its control are one thing to a reader, and the only way to tell
  // that they have come apart is to measure them together.
  const rectOf = (node) => {
    const box = node.getBoundingClientRect();
    return { x: box.x, y: box.y, width: box.width, height: box.height };
  };
  const labels = [];
  for (const label of doc.querySelectorAll("label[for]")) {
    const named = doc.getElementById(label.getAttribute("for"));
    if (named && visible(label) && visible(named)) {
      labels.push({ text: label.textContent.trim().slice(0, 40), label: rectOf(label), control: rectOf(named) });
    }
  }
  // What this render actually put on screen, as something two states can be
  // compared by.
  //
  // Not pixels: two runs of one state differ by a handful of antialiased
  // glyphs, so byte-identity of a screenshot answers "were these written by
  // the same run" rather than "do these two states draw the same screen".
  //
  // Not geometry either, and that took measuring to learn. A signature carrying
  // rounded boxes drifted on 66 of 500 renders across two runs of one tree:
  // glyph advances and React Flow's own layout both move by a fraction, and a
  // fraction is enough to round the other way. A check built on that is a flaky
  // check, which is worse than no check. Geometry is not lost — `boxes` above
  // measures it, and the clipping and overlap rules are where a layout fault is
  // caught.
  //
  // What is left is every attribute that is not computed geometry, plus the
  // words an element carries itself and what a form control holds. Attributes
  // rather than a chosen few, because the chosen few were wrong: reading only
  // `data-testid` and `class` could not see a `<details open>`, so opening the
  // relation graph produced the identical signature to leaving it shut — on
  // eight pairs of states at once — which is why `relationOpen` reads the
  // attribute.
  //
  // `boxed` rather than `visible`: this is a description, not a promise, taken
  // over every element rather than the handful a state names. An element
  // painted or not is still a difference between two screens.
  //
  // The harness's own address is not a property of the state, and leaving it in
  // made two states permanently undiffable. `serve.mjs` binds an ephemeral port
  // per worker, and the renderer-error fallback quotes the module it could not
  // fetch — so `surface:boot+data:render-error` signed
  // `…127.0.0.1:40091/app.mjs` on one run and `…:35781/app.mjs` on the next.
  // That is not late content and no amount of settling reaches it: the state
  // could never match itself between runs, nor a twin rendered by a different
  // worker, so it was a standing finding that said nothing about the product.
  // Loopback origins are folded to one name rather than the document's own,
  // because the lab renders the page in an iframe under a second host.
  const ORIGIN = /https?:\/\/(?:127\.0\.0\.1|\[::1\]|localhost):\d+/gu;
  const stable = (value) => value.replace(ORIGIN, "http://canvas.invalid");
  const GEOMETRY = new Set(["style", "transform", "d", "points", "viewBox", "x", "y", "cx", "cy", "r", "width", "height"]);
  const signature = [];
  for (const node of doc.querySelectorAll("body *")) {
    if (!boxed(node)) {
      continue;
    }
    signature.push([
      node.tagName,
      [...node.attributes]
        .filter((attribute) => !GEOMETRY.has(attribute.name))
        .map((attribute) => `${attribute.name}=${attribute.value}`)
        .sort()
        .join(","),
      node.childElementCount ? "" : (node.textContent ?? "").trim(),
      // What a form control holds is not in the document's text, and it is
      // most of what a form state *is*. Without it a typed pipeline name and
      // an empty Name box were one screen, and so were three different runs
      // chosen in the replay picker — the registry said six states and the
      // renders said two.
      typeof node.value === "string" ? node.value : "",
      node.checked === true ? "checked" : "",
    ].join("|"));
  }
  // Every React Flow surface on screen, as the number of edges it was handed
  // beside the number it has actually drawn.
  //
  // React Flow draws edges in a pass after it has measured the nodes, and that
  // pass can land after the rest of the page has stopped changing — so a
  // signature can go still on a graph of disconnected boxes, and a settle rule
  // built on stability alone calls that finished. Measured on this tree, the
  // two states declared twins over the draft bar's refusal each rendered 89
  // elements on some runs and 111 on others, the difference being four edges
  // and their labels, and every one of those frames was filed as settled.
  //
  // The graph publishes what it was asked to draw, so the answer is a
  // comparison rather than a guess about how long that pass takes. Hidden
  // graphs are skipped: a relation graph inside a shut disclosure has drawn no
  // edges and never will, and waiting on one would be a wait with no end.
  const graphs = [];
  for (const node of doc.querySelectorAll("[data-graph-edges]")) {
    if (boxed(node)) {
      graphs.push({
        declared: Number(node.getAttribute("data-graph-edges")),
        // `boxed`, not presence. React Flow puts an edge's element into the DOM
        // before it has a path to draw, so counting elements answered "drawn: 4"
        // about a graph the signature was describing with no edges on it at all.
        drawn: [...node.querySelectorAll(".react-flow__edge")].filter(boxed).length,
      });
    }
  }
  return {
    counts,
    texts,
    boxes,
    contrast,
    labels,
    graphs,
    signature: stable(signature.join("\n")),
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
  // The relation disclosure is the tallest landing region when it is open, and
  // it is the last sibling in the column — so it is the one most able to print
  // over what sits above it, and it was the only one nothing measured.
  ".relation-section",
  // The graph's own cards, which nothing here measured at all. Every card on
  // the pipeline surface is placed by `lib/layout.mjs` at coordinates it
  // computes, so a placement rule that gets one wrong draws two cards on top of
  // each other and every check still passed: `shows` counts them, and the
  // pannable surface excuses clipping. That is how the terminal rail came to
  // stand 120px inside a concurrent group's member row — a collision the
  // offline suite could not see and the Edge harness only checked on the two
  // pipelines it happens to open.
  ".flow-card",
];

/**
 * Regions that stack vertically and must never sit on top of one another.
 *
 * `.flow-card` is the exception that is not vertical: the graph places its
 * cards in two dimensions, and the rule there is simply that no two of them may
 * intersect. It belongs on this list rather than in `SIBLINGS` because React
 * Flow gives every node its own absolutely positioned wrapper — so the cards
 * share no DOM parent and are not in normal flow, and neither the sibling rule
 * nor the flow rule can reach them. Same-selector comparison can.
 */
const STACKED = [".assignment-card", ".detail-row", ".limit-row", ".repo-row", ".flow-card"];

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
  [".relation-section", ".assignment-card"],
  [".relation-section", ".orphan-strip"],
  [".relation-section", ".create-bar"],
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

/**
 * Which look answers for a render whose settle window ran out.
 *
 * `judge` exits early on a clean, agreeing render, so reaching the deadline
 * means the page never settled. Both obvious answers are wrong there:
 *
 * - The *last* look fails a state that was observed correct, because the
 *   deadline lands on whatever frame a contended worker happened to be drawing.
 *   That is the flake this preference was introduced to remove.
 * - The *first clean* look passes a state that was correct once and is not any
 *   more — a control that disappears after first paint, an error that arrives
 *   late. That is the one failure a review surface may not have, and it is what
 *   the preference introduced in its place: nothing could contradict a green
 *   frame, however many red ones came after it.
 *
 * A failure that comes and goes is the harness; a failure that arrives and
 * stays is the product. So the tie-break is how long the failures have lasted:
 * once as many consecutive looks have failed as agreement itself requires, the
 * render is failing rather than flickering and the last look answers. Below
 * that, an observed clean look wins.
 *
 * Pure and boolean, so the offline suite can hold the rule without a browser
 * and without a clock.
 */
export function deadlineVerdict({ lastFailed, sustained, sawClean }, repeats) {
  if (!lastFailed || sustained >= repeats || !sawClean) {
    return "last";
  }
  return "clean";
}

/**
 * Whether every visible graph on the render has drawn the edges it declared.
 *
 * The other half of settling. A signature going still says the page stopped
 * changing *for a while*, which a graph mid-draw satisfies — React Flow lays
 * its edges out in a pass of its own, and between the nodes landing and that
 * pass starting there is a genuine lull. So stability alone filed edgeless
 * graphs as settled renders, the gallery published them for review, and the
 * twin audit compared them as evidence: the two states declared twins over the
 * draft bar's refusal were reported as "no longer draw the same screen" on runs
 * where one of them had drawn its four relation edges and the other had not.
 *
 * This is a claim the surface makes about itself rather than a list of
 * selectors to wait for, so it cannot fall out of date when a graph gains an
 * edge — `data-graph-edges` is the count handed to React Flow, at the one place
 * that knows it.
 *
 * "No edge is still missing" rather than an exact count: the question being
 * asked is whether the draw pass has happened, and a surface that puts one more
 * element through that selector than it declared has plainly had it.
 *
 * Pure, so the offline suite holds the rule without a browser.
 */
export function graphsDrawn(snapshot) {
  return (snapshot?.graphs ?? []).every((graph) => graph.drawn >= graph.declared);
}

export function verdict(state, snapshot, options = {}) {
  return [
    ...missing(state, snapshot),
    ...forbidden(state, snapshot),
    ...absentCopy(state, snapshot),
    ...promisedCopy(state, snapshot),
    ...lowContrast(snapshot, options),
    ...strandedLabels(snapshot),
    ...overlaps(snapshot),
    ...clipping(snapshot, options),
  ];
}

/**
 * A label has to sit beside or above the control it names.
 *
 * The create bar drew its refusal as an extra child of a four-column field
 * grid, so "could not create pipeline" took the cell the Name *label* was meant
 * for and pushed the label and its input into different rows and columns. Every
 * other check passed: nothing was clipped, nothing overlapped, both controls
 * were present and the copy was right. The form was simply telling the reader
 * that the box below "Kind" was called "Name", at the moment it was asking them
 * to try again.
 *
 * Adjacency is deliberately generous — sharing a horizontal band (beside) or a
 * vertical one (above) both count — because the two viewports lay these pairs
 * out differently and only one arrangement is wrong: the one where the label
 * shares neither, and so points at nothing.
 */
function strandedLabels(snapshot) {
  return (snapshot.labels ?? [])
    .filter((pair) => !adjacent(pair.label, pair.control))
    .map((pair) => ({
      kind: "stranded-label",
      detail: `"${pair.text}" sits neither beside nor above the control it names`,
    }));
}

function adjacent(label, control) {
  return span(label.y, label.height, control.y, control.height)
    || span(label.x, label.width, control.x, control.width);
}

/** Whether two one-dimensional extents share any of the same line. */
function span(start, size, otherStart, otherSize) {
  return Math.min(start + size, otherStart + otherSize) - Math.max(start, otherStart) > 0;
}

/**
 * Copy that reserves room for something the surface does not draw.
 *
 * The pipeline side panel carried a "Trust flow — Reserved for trust analysis."
 * section whose body was a constant. Trust results arrive as advisories and are
 * already drawn in the findings directly above it, so the section told a reader
 * that the one thing it names is missing while the panel was showing it. A
 * region that never varies has no state to assert, so nothing else here could
 * fail for it — only the gallery reads it as a defect rather than a stub.
 *
 * These are phrases rather than selectors because the defect is the sentence:
 * whatever markup carries it, a surface that promises instead of drawing is
 * one a reviewer has to be told about. The text searched is the whole body,
 * which includes config the canvas is quoting rather than authoring — a `run:`
 * command may legitimately say any of these — so a state can declare the
 * phrase its own, the way `allowErrors` declares a failed request its own.
 */
const PLACEHOLDER_PROMISES = [/reserved for\b/iu, /coming soon\b/iu, /not implemented\b/iu, /to be (?:added|built|done)\b/iu];

function promisedCopy(state, snapshot) {
  if (state.expect?.allowPlaceholder?.length) {
    return [];
  }
  const text = normalise(snapshot.text);
  return PLACEHOLDER_PROMISES
    .filter((pattern) => pattern.test(text))
    .map((pattern) => ({ kind: "placeholder-copy", detail: `the render says "${text.match(pattern)[0]}" instead of drawing it` }));
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

/**
 * Copy, in the two shapes a promise about words can take.
 *
 * A plain string is a substring of the whole body, which is right for a
 * sentence that may be drawn anywhere — a refusal, an advisory, a summary.
 *
 * It is wrong for a *status*, and dangerously so: a substring search cannot
 * distinguish a word from its own negation. `edit: rest` promised the editor
 * says "saved", and "unsaved edits" contains "saved", so the one state whose
 * whole subject is that nothing is pending was satisfied by the screen saying
 * something is. The assertion could not fail in the direction it existed to
 * catch.
 *
 * So a copy expectation may instead name the element that carries it, and then
 * the element's own text must be *exactly* the phrase. "unsaved edits" is not
 * "saved", and the state fails by name.
 */
function absentCopy(state, snapshot) {
  return (state.expect.copy ?? [])
    .filter((phrase) => !satisfied(phrase, snapshot))
    .map((phrase) => ({ kind: "missing-copy", detail: copyLabel(phrase) }));
}

function satisfied(phrase, snapshot) {
  if (typeof phrase !== "object" || phrase === null) {
    return normalise(snapshot.text).includes(normalise(phrase));
  }
  return normalise(snapshot.texts?.[phrase.selector]) === normalise(phrase.text);
}

/** One stable string per expectation, so a failure names what was promised. */
export function copyLabel(phrase) {
  return typeof phrase === "object" && phrase !== null ? `${phrase.selector} reads exactly “${phrase.text}”` : phrase;
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
 * Two boxes drawn by the same kind of region must not overlap, plus the named
 * sibling pairs and anything in normal flow that prints over its own sibling.
 */
function overlaps(snapshot) {
  return dedupe([...sameKind(snapshot), ...siblingKinds(snapshot), ...siblingBoxes(snapshot)]);
}

/**
 * Anything in normal flow that prints over its own DOM sibling.
 *
 * `SIBLINGS` above is a list someone has to remember to extend, and it only
 * ever covered the landing column. This is the same claim as a rule: boxes
 * that share a parent and are both in normal flow are laid out *beside* or
 * *below* one another by definition, so an intersection between them is the
 * layout failing, whatever the two regions happen to be. Nesting cannot reach
 * here — a child does not share its parent's parent — and absolutely
 * positioned boxes are excluded, because overlapping is what they are for.
 */
function siblingBoxes(snapshot) {
  const found = [];
  const boxes = snapshot.boxes.filter((box) => box.flow && box.parent);
  for (let left = 0; left < boxes.length; left += 1) {
    for (let right = left + 1; right < boxes.length; right += 1) {
      const pair = [boxes[left], boxes[right]];
      const sameElement = pair[0].id !== undefined && pair[0].id === pair[1].id;
      if (!sameElement && pair[0].parent === pair[1].parent && intersects(...pair)) {
        found.push({ kind: "overlap", detail: `${pair[0].selector} overlaps ${pair[1].selector}` });
      }
    }
  }
  return found;
}

function dedupe(problems) {
  const seen = new Set();
  return problems.filter((problem) => {
    const key = `${problem.kind}:${problem.detail}`;
    return seen.has(key) ? false : Boolean(seen.add(key));
  });
}

/**
 * Two boxes drawn by the same kind of region must not overlap. Nesting a
 * *different* kind is fine — a detail row lives inside a card — and so is
 * nesting the same kind inside itself, which the repo adder does: its resolved
 * preview draws a `.detail-row` per field inside the `.detail-row` that holds
 * the repos field. Containment is checked and skipped rather than measured,
 * because a parent always intersects its child and that is not a defect.
 *
 * What is left is exactly the "cards overprinting each other" defect the
 * browser suite has caught before, plus the sibling pairs above, which is the
 * defect a crossing probe is rendered to find.
 */
function sameKind(snapshot) {
  const found = [];
  for (const selector of STACKED) {
    const boxes = snapshot.boxes.filter((box) => box.selector === selector);
    for (let left = 0; left < boxes.length; left += 1) {
      for (let right = left + 1; right < boxes.length; right += 1) {
        if (!nested(boxes[left], boxes[right]) && intersects(boxes[left], boxes[right])) {
          found.push({ kind: "overlap", detail: `${selector} #${left} overlaps #${right}` });
        }
      }
    }
  }
  return found;
}

/** Whether either box is drawn inside the other. */
function nested(left, right) {
  return Boolean(left.within?.includes(right.id) || right.within?.includes(left.id));
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
 * an edge or be cut away by a clipping ancestor.
 *
 * The right edge alone used to be the whole check, which is how a compact
 * viewport breaks most often but not the only way. A box pushed off the *left*
 * is just as unreachable, and a control cut away entirely by an
 * `overflow: hidden` parent is worse than either, because it still reports a
 * box — so `shows` passes while the user sees nothing.
 *
 * Total loss was too generous a bar on its own, though: a control whose label
 * or click target is half eaten by its own container is a defect a reader can
 * see and this could not. So a trim is judged too, everywhere except the one
 * surface that pans.
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
    if (box.x < -slack) {
      problems.push({ kind: "clipped", detail: `${box.selector} starts ${Math.round(-box.x)}px left of the viewport` });
    }
    if (box.clipped) {
      problems.push({ kind: "clipped", detail: `${box.selector} is cut away entirely by a clipping ancestor` });
    } else if ((box.trimmed ?? 0) > slack) {
      problems.push({ kind: "clipped", detail: `${box.selector} is cut ${box.trimmed}px into by a clipping ancestor` });
    }
  }
  return dedupe(problems);
}

/**
 * Every selector a state mentions, so one collect pass covers the verdict.
 *
 * Scoped copy is included: an expectation that names an element has to have
 * that element's own text gathered, and `collect` gathers text for exactly the
 * selectors it is given.
 */
export function selectorsFor(state) {
  return [...new Set([...(state.expect.shows ?? []), ...(state.expect.hides ?? []), ...scopedCopy(state).map((item) => item.selector)])];
}

/** The copy expectations that name an element rather than the whole page. */
function scopedCopy(state) {
  return (state.expect?.copy ?? []).filter((phrase) => typeof phrase === "object" && phrase !== null);
}

/**
 * What to measure for a state: the standing regions, plus the controls this
 * state promises are on screen.
 *
 * Measuring only the standing list meant the geometry checks never looked at
 * the thing the state is actually about. A Save button could be shoved off the
 * viewport or cut away by its own editor and every check still passed, because
 * `shows` asks whether it has a box and nothing asked where that box was.
 */
export function measureFor(state) {
  return [...new Set([...MEASURED, ...(state.expect?.shows ?? [])])];
}
