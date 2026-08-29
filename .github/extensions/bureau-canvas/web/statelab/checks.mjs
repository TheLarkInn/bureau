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
  //
  // `filter: opacity(0)` is the same erasure spelled a second way, and the walk
  // above could not see it: the `opacity` property stays "1", so a control
  // filtered to nothing satisfied every `shows`, and a promised sentence
  // filtered to nothing was still reported readable. It is folded in here
  // rather than treated as its own rule because it composes exactly like
  // opacity does — down the ancestor chain, multiplicatively — and no
  // stylesheet in this canvas declares a `filter`, so nothing legitimate is
  // caught by reading one.
  const filterAlpha = (value) => {
    let product = 1;
    for (const found of String(value ?? "").matchAll(/opacity\(\s*([\d.]+)(%?)\s*\)/giu)) {
      const amount = Number.parseFloat(found[1]);
      product *= found[2] ? amount / 100 : amount;
    }
    return product;
  };
  const painted = (node) => {
    for (let item = node; item; item = item.parentElement) {
      const style = view.getComputedStyle(item);
      if (Number.parseFloat(style.opacity) === 0 || filterAlpha(style.filter) === 0) {
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
  //
  // And keeps looking *past* it. Retaining only the nearest clipper per axis
  // assumed the innermost window is the narrowest one, which is false whenever a
  // wide inner box sits inside a narrow outer one: a control comfortably inside
  // a 300px pane, itself inside a 100px `overflow-x: hidden` shell, is entirely
  // off screen and reported `trimmed: 0`. Every ancestor gets a vote, so what is
  // kept is the intersection — the window the element is actually seen through —
  // and a check that could not see the outer lid can now fail on it.
  const clipper = (node) => {
    let x = null;
    let y = null;
    let pannable = false;
    for (let item = node.parentElement; item; item = item.parentElement) {
      const style = view.getComputedStyle(item);
      if (/hidden|clip/u.test(style.overflowX)) {
        const box = item.getBoundingClientRect();
        x = { left: Math.max(x?.left ?? box.left, box.left), right: Math.min(x?.right ?? box.right, box.right) };
      }
      if (/hidden|clip/u.test(style.overflowY)) {
        const box = item.getBoundingClientRect();
        y = { top: Math.max(y?.top ?? box.top, box.top), bottom: Math.min(y?.bottom ?? box.bottom, box.bottom) };
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
  // What the words in each requested element are actually made of.
  //
  // `texts` above reads `innerText`, and a reader does not. `innerText` reports
  // words painted in transparent ink, and it does not report the words a
  // `::before` or `::after` paints in their place — so a rule that sets an
  // element's own colour to `transparent` and spells a different sentence in
  // generated content satisfies an exact-text expectation while the screen says
  // something else entirely. That is the `opacity: 0` lie one level down: the
  // words measure perfectly, and none of them are the ones on screen.
  //
  // Gathered for every requested selector and judged only where a state
  // promises exact words, keeping the split this module is built on — `collect`
  // reports what the page is, `verdict` decides what that means.
  const generated = (node, part) => {
    const value = view.getComputedStyle(node, part).content;
    return ["none", "normal", "\"\"", "''"].includes(value) ? "" : value;
  };
  const alpha = (value) => {
    const parts = channels(value);
    return parts.length < 4 ? 1 : parts[3];
  };
  const honestInk = (node) => {
    const style = view.getComputedStyle(node);
    const rect = node.getBoundingClientRect();
    const indent = Math.abs(Number.parseFloat(style.textIndent) || 0);
    let opacity = alpha(style.color) * alpha(style.webkitTextFillColor);
    for (let item = node; item; item = item.parentElement) {
      const chain = view.getComputedStyle(item);
      opacity *= Number.parseFloat(chain.opacity) * filterAlpha(chain.filter);
    }
    return opacity === 1
      && (!style.fontSize || Number.parseFloat(style.fontSize) > 0)
      && (!style.clipPath || style.clipPath === "none")
      && indent < Math.max(rect.width, 1);
  };
  const exposed = (node) => {
    if (!doc.elementFromPoint) {
      return true;
    }
    const rect = node.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      return true;
    }
    const x = rect.x + rect.width / 2;
    const y = rect.y + rect.height / 2;
    if (x < 0 || x > doc.documentElement.clientWidth || y < 0 || y > doc.documentElement.clientHeight) {
      return true;
    }
    const front = doc.elementFromPoint(x, y);
    return front === node || node.contains(front);
  };
  const relatedTo = (node) => {
    const related = [node, ...(node.querySelectorAll?.("*") ?? [])];
    for (let item = node.parentElement; item; item = item.parentElement) {
      related.push(item);
    }
    return [...new Set(related)];
  };
  // Whether a style paints something a reader cannot see through.
  //
  // `solid` mirrors `opaque` but at the top of the range instead of the bottom,
  // and the difference is the point. Any non-zero alpha is enough to *supply* a
  // backdrop, and nowhere near enough to hide words: a 2% scrim, a
  // click-catcher, the first frame of a fade-in are all `opaque` and none of
  // them erases a sentence. `honestInk` holds the carrier's own ink to exactly
  // 1 for the same reason. It keeps `opaque`'s presence test, because a colour
  // that parses to nothing is no paint at all — reading an absent
  // `backgroundColor` as fully opaque would call every style filled.
  const solid = (value) => {
    const parts = channels(value);
    return parts.length >= 3 && (parts.length < 4 || parts[3] === 1);
  };
  const filled = (style) =>
    solid(style.backgroundColor) || Boolean(style.backgroundImage && style.backgroundImage !== "none");
  // Whether an element establishes a containing block for absolute descendants.
  const anchors = (style) =>
    style.position !== "static"
    || Boolean(style.transform && style.transform !== "none")
    || Boolean(style.filter && style.filter !== "none");
  // The region a node is actually painted in, as far as its scrolling and
  // clipping ancestors allow, or `null` when nothing bounds it.
  //
  // Wider than `clipper` on purpose. `clipper` reads only `hidden`/`clip`,
  // because a phrase scrolled out of an `auto` pane is one gesture away rather
  // than lost, and it would be a false conviction to call it missing. But it is
  // equally false to ask what stands at a point the phrase is *not drawn at*:
  // above the fold of an inspector, the answer is the toolbar above the pane,
  // which covers nothing because there is nothing of this node there to cover.
  //
  // Only ancestors that actually clip this node count. An overflow ancestor
  // clips a descendant whose containing block is inside it, so a `fixed` node is
  // bounded by nothing here and an `absolute` one by nothing until the walk
  // reaches the element it is positioned against. Bounding a node CSS does not
  // bound would excuse real occlusion, since a point outside the invented
  // region is never asked about at all.
  const scrollport = (node) => {
    const own = view.getComputedStyle(node).position;
    if (own === "fixed") {
      return null;
    }
    let escaping = own === "absolute";
    let box = null;
    for (let item = node.parentElement; item; item = item.parentElement) {
      const style = view.getComputedStyle(item);
      if (escaping && !anchors(style)) {
        continue;
      }
      escaping = false;
      if (!/auto|scroll|hidden|clip/u.test(`${style.overflowX} ${style.overflowY}`)) {
        continue;
      }
      const rect = item.getBoundingClientRect();
      box = {
        left: Math.max(box?.left ?? rect.left, rect.left),
        right: Math.min(box?.right ?? rect.right, rect.right),
        top: Math.max(box?.top ?? rect.top, rect.top),
        bottom: Math.min(box?.bottom ?? rect.bottom, rect.bottom),
      };
    }
    return box;
  };
  // Whether an element paints a positioned, filled layer of its own. Split out
  // of `coveringLayer` because two callers need it at different reaches: a
  // scoped promise asks it of a whole neighbourhood, and a carrier asks it of
  // the one element proved to be in front of the words.
  const layered = (item) =>
    ["::before", "::after"].some((part) => {
      const style = view.getComputedStyle(item, part);
      const exists = !["none", "normal"].includes(style.content);
      return exists && /absolute|fixed/u.test(style.position) && filled(style);
    });
  // Whether an element paints an opaque fill in its own right, generated or
  // not. An element beside the words needs no pseudo-element to hide them: an
  // ordinary panel with a background is the plainest way there is. Its own
  // opacity chain is folded in, because a panel faded to nothing paints nothing
  // however solid the colour it was given.
  const fills = (item) => {
    let opacity = 1;
    for (let step = item; step; step = step.parentElement) {
      const chain = view.getComputedStyle(step);
      opacity *= Number.parseFloat(chain.opacity) * filterAlpha(chain.filter);
    }
    return opacity === 1 && (layered(item) || filled(view.getComputedStyle(item)));
  };
  const coveringLayer = (node) => relatedTo(node).some(layered);
  // Where a node's words are actually painted, which is not the middle of its
  // box. A paragraph with a tall top padding, or an inline sentence that wraps,
  // has a bounding box whose centre lands on empty ground — and asking the hit
  // test there answers about whatever the page draws above the first line. A
  // range over the node's own contents reports the line boxes themselves.
  const inkRect = (node) => {
    const range = doc.createRange?.();
    if (range) {
      range.selectNodeContents(node);
      const lines = [...range.getClientRects()].filter((line) => line.width > 0 && line.height > 0);
      if (lines.length) {
        return lines[0];
      }
    }
    return node.getBoundingClientRect();
  };
  // The element actually drawn in front of a node's words, or nothing.
  //
  // `exposed` answers the same question as a boolean and treats "cannot tell"
  // as exposed; this returns the blocker itself, so a caller can ask what it is
  // rather than only that it exists.
  const inFront = (node) => {
    if (!doc.elementFromPoint) {
      return null;
    }
    const rect = inkRect(node);
    if (rect.width <= 0 || rect.height <= 0) {
      return null;
    }
    const x = rect.x + rect.width / 2;
    const y = rect.y + rect.height / 2;
    if (x < 0 || x > doc.documentElement.clientWidth || y < 0 || y > doc.documentElement.clientHeight) {
      return null;
    }
    // Only ask about a point the node is painted at. A phrase scrolled above
    // the fold of a pane still has a box, and it sits over whatever the page
    // draws there — the toolbar above the pane, another panel beside it. None
    // of those is covering the words, because none of the words are there.
    const port = scrollport(node);
    if (port && (x < port.left || x > port.right || y < port.top || y > port.bottom)) {
      return null;
    }
    const stack = doc.elementsFromPoint ? [...doc.elementsFromPoint(x, y)] : [doc.elementFromPoint(x, y)];
    const top = stack[0];
    if (!top || top === node || node.contains(top)) {
      return null;
    }
    // A carrier the hit test cannot see was skipped, not covered. With
    // `pointer-events: none` — which inherits, so this one read answers for the
    // whole chain — the browser reports what stands *behind* the node, and what
    // stands behind it is its own ancestors. Convicting on those reports every
    // decorative layer in an app shell as a substituted sentence. Something
    // beside it in the tree is still a blocker, so the stack is read past its
    // ancestors rather than abandoned.
    if (view.getComputedStyle(node).pointerEvents !== "none") {
      return top;
    }
    return stack.find((item) => item !== node && !node.contains(item) && !item.contains(node)) ?? null;
  };
  const generatedAround = (node) =>
    relatedTo(node)
      .flatMap((item) => [generated(item, "::before"), generated(item, "::after")])
      .filter(Boolean);
  const paint = {};
  for (const selector of request.selectors) {
    const found = [...doc.querySelectorAll(selector)].filter(visible);
    paint[selector] = {
      ink: found.every((node) => honestInk(node) && exposed(node) && !coveringLayer(node)),
      injected: [...new Set(found.flatMap(generatedAround))].join(" "),
    };
  }
  // The same question, asked of the promises that name no element.
  //
  // A plain phrase is settled against `doc.body.innerText`, which reports words
  // no reader receives just as surely as `texts` did — transparent ink, a
  // container at `opacity: 0`, a filter, a clip. Scoping the paint check to
  // selectors left that hole open for the great majority of this registry's
  // copy: most promises are plain, so most promised sentences were still being
  // proved against the DOM alone.
  //
  // The words are found where they actually live: the element holding them in
  // its *own* direct text, rather than any ancestor whose `innerText` merely
  // contains them. That element is the one a reader is looking at, and it is
  // also the only one whose paint answers for these words rather than for a
  // whole subtree's.
  //
  // Two deliberate narrowings, and they are narrowings rather than oversights.
  // Generated content is read from the carrier's own `::before`/`::after`
  // instead of the neighbourhood sweep a scoped promise gets, and the
  // covering-layer sweep is applied only together with a hit test. A scoped
  // selector is one the registry chose and vouches for; a carrier is whatever
  // element happened to hold the words, so sweeping its ancestors alone would
  // report every decorative overlay in the product as a substituted sentence.
  // The substitution this catches is the one painted on the words themselves —
  // either by the carrier, or by an ancestor layer proved to be in front of
  // them.
  const carriers = {};
  const phrases = request.phrases ?? [];
  if (phrases.length) {
    const flat = (value) => String(value ?? "").replace(/\s+/gu, " ").trim().toLowerCase();
    const ownText = (node) => [...(node.childNodes ?? [])]
      .filter((child) => child.nodeType === 3)
      .map((child) => child.data ?? "")
      .join("");
    // Walked once and reused, rather than re-queried per phrase: `collect` runs
    // several times per render while the settle rule waits for the page to come
    // to rest, so a per-phrase document walk is paid tens of thousands of times
    // across a matrix run.
    const owned = [...doc.querySelectorAll("*")].map((node) => ({ node, text: flat(ownText(node)) }));
    // Every text node in document order, whitespace-only ones included, built
    // at most once and only for a phrase no single element owns.
    //
    // The blank ones are the separators. The newline a template leaves between
    // two block tags is what tells `flat` that the last word of one and the
    // first word of the next are two words, so a run joined from raw `data`
    // reads the way the screen does without this having to model block layout.
    //
    // Walked over `childNodes` rather than with a `TreeWalker`, because
    // `collect` is rebuilt from its own source and run against both a browser
    // document and the offline stub, and `childNodes`/`nodeType` is the whole
    // DOM this module already asks of either — `ownText` above uses the same.
    let runs = null;
    const textRuns = () => {
      if (!runs) {
        runs = [];
        const descend = (node) => {
          for (const child of [...(node?.childNodes ?? [])]) {
            if (child.nodeType === 3) {
              runs.push(child);
            } else {
              descend(child);
            }
          }
        };
        descend(doc.body ?? doc);
      }
      return runs;
    };
    // The carriers of a phrase no single element owns.
    //
    // A promised sentence with a `<strong>` in the middle of it is held by
    // three text nodes and owned whole by none of them. Such a phrase used to
    // be *exempt*, and that exemption covered the one case this whole module
    // exists to judge: a plain phrase is settled by `absentCopy` against
    // `innerText`, which reports transparent ink, so a split sentence painted
    // in nothing satisfied the presence check, was excused by the paint check,
    // and left every gate green. It excused nothing in this registry on the day
    // it was written — measured, 0 of 1,068 promised phrases — which is exactly
    // why it survived: a hole is not visible from inside a run that never falls
    // into it, and it would have opened silently the first time a component
    // wrapped part of a promised sentence.
    //
    // Each contiguous run of text nodes whose joined text spans the phrase is
    // one candidate set, and its carriers are the elements those nodes belong
    // to. The scan starts a run at every node and stops extending it once the
    // joined text is longer than the phrase could need, so this stays linear in
    // the document rather than quadratic.
    const spans = (wanted) => {
      const sets = [];
      const nodes = textRuns();
      for (let start = 0; start < nodes.length; start += 1) {
        const bound = flat(nodes[start].data).length + wanted.length;
        let joined = "";
        for (let end = start; end < nodes.length && flat(joined).length <= bound; end += 1) {
          joined += nodes[end].data ?? "";
          if (flat(joined).includes(wanted)) {
            sets.push([...new Set(nodes.slice(start, end + 1)
              .filter((node) => (node.data ?? "").trim())
              .map((node) => node.parentElement)
              .filter(Boolean))]);
            break;
          }
        }
      }
      return sets.filter((set) => set.length);
    };
    // Readability here is about the ink, not the scroll position. A scoped
    // promise names an element the registry vouches for being in view, so that
    // one is also asked whether anything is in front of it; a plain phrase is
    // carried by whatever element happens to hold the words, and in this product
    // that is routinely a label inside a scrollable form — above the fold while
    // the reader is looking at the bottom of it, which is not a defect and is
    // not something this promise ever claimed. So `exposed` is deliberately not
    // applied *on its own*: what is asked is whether the words are painted at
    // all.
    //
    // Words inside a lid are not painted either, and nothing was asking. The
    // boxes above are judged on their clipping ancestors, but a plain phrase
    // names no selector, so it is never measured — and `visible` reads only the
    // carrier's own box, paint and visibility. A promised sentence moved
    // entirely outside an `overflow: hidden` ancestor therefore measured
    // perfectly and was reported readable, which is exactly the class of defect
    // this module exists to catch. The same `clipper` answers for a carrier as
    // for a box, so `auto` and `scroll` stay one gesture away rather than lost.
    //
    // Held to `clipped` rather than to `trimmed`, and the difference is
    // deliberate. `trimmed` is waived on a pannable surface, because a graph
    // card half out of frame is the surface working and the reader drags it
    // back. Being *entirely* outside the frame is not waived there and never
    // has been — "gone, whatever surface it is on" is the rule the measured
    // boxes are already held to, and a phrase is not more readable than a
    // control drawn in the same place.
    const framed = (node) => {
      const rect = node.getBoundingClientRect();
      const clip = clipper(node);
      const outsideX = clip.x && (rect.x >= clip.x.right || rect.x + rect.width <= clip.x.left);
      const outsideY = clip.y && (rect.y >= clip.y.bottom || rect.y + rect.height <= clip.y.top);
      return !outsideX && !outsideY;
    };
    // Ink that is opaque and still not there. `honestInk` reads alpha, so it
    // catches `transparent` and every way of multiplying opacity to nothing,
    // and it has nothing at all to say about white on white: the colour is
    // fully opaque, the contrast is 1, and the words are as gone as if they
    // had never been drawn.
    //
    // Deliberately not the WCAG grade `CONTRAST` applies. That list is two
    // selectors the registry chose because their hue carries meaning, and it
    // vouches for them at 4.5:1; a promised sentence is carried by whatever
    // element happens to hold the words, and grading all of them is a design
    // review this check has no standing to make. The threshold here is the
    // same question the rest of the module asks — were the words painted at
    // all — so it sits just above the ratio of ink that is literally its own
    // background. The grading gap is recorded as a limit, not closed here.
    const legible = (node) => {
      // What cannot be measured is not convicted. `backdrop` reads
      // `backgroundColor` and nothing else, so white text over a dark
      // background *image* would read as white on the default white and fail a
      // screen that is perfectly readable. An image's luminance is not
      // available synchronously, so a phrase drawn over one is left alone and
      // the gap is recorded as a limit.
      for (let item = node; item; item = item.parentElement) {
        const style = view.getComputedStyle(item);
        if (style.backgroundImage && style.backgroundImage !== "none") {
          return true;
        }
        if (opaque(style.backgroundColor)) {
          break;
        }
      }
      const front = luminance(view.getComputedStyle(node).color);
      const back = luminance(backdrop(node));
      return (Math.max(front, back) + 0.05) / (Math.min(front, back) + 0.05) > 1.05;
    };
    // An opaque layer painted over the words, by the element actually in front
    // of them.
    //
    // Neither instrument is usable alone, which is why the covering-layer sweep
    // was left off carriers entirely. `coveringLayer` sweeps a whole
    // neighbourhood, so on its own it reports every decoration in the product
    // as a substituted sentence — a carrier is whatever element happened to
    // hold the words, not one the registry vouches for. A bare hit test is no
    // better: it convicts any neighbour that legitimately covers a label's
    // centre, a sticky bar or an open menu included.
    //
    // Asking them together is not enough either, and that was the first shape
    // of this clause. An app shell with one decorative absolute `::before`
    // anywhere above the carrier satisfies the sweep for *every* node beneath
    // it, so the pair reduced to the bare hit test the moment such a layer
    // existed. What is asked instead is that the element proved to be in front
    // of the words is itself the one painting over them.
    //
    // What counts as painting over them depends on where that element stands.
    // An ancestor is asked only for a positioned layer of its own, and that
    // leniency is about the *instrument*, not about CSS. An ancestor almost
    // always wins this hit test because the probe missed the words rather than
    // because it is covering them: a sentence that wraps, a carrier the browser
    // skipped, a box taller than its own text. Asking such an ancestor for its
    // ordinary background would convict every card in the product. It is not
    // that an ancestor cannot hide its descendants' words — a descendant in a
    // negative-`z-index` stacking context is painted under an in-flow
    // ancestor's background, and this clause does not catch that. That is a
    // recorded limit, and the price of not convicting every card.
    //
    // Anything else in front of the words is beside them in the tree, and there
    // an ordinary opaque background hides them exactly as completely as a
    // generated layer does — a panel, a drawer, a solid overlay. Requiring a
    // pseudo-element there reported a sentence with a filled `<div>` drawn over
    // it as perfectly readable, which is the plainest way to cover words there
    // is.
    const smothered = (node) => {
      const front = inFront(node);
      if (!front) {
        return false;
      }
      return front.contains(node) ? layered(front) : fills(front);
    };
    // Memoised because `spans` can offer the same element in several candidate
    // sets, and each of these clauses walks the element's whole ancestor chain.
    const verdicts = new Map();
    const readable = (node) => {
      if (!verdicts.has(node)) {
        verdicts.set(node, visible(node) && honestInk(node) && framed(node) && legible(node));
      }
      return verdicts.get(node);
    };
    const speaks = (node) => [generated(node, "::before"), generated(node, "::after")].filter(Boolean).join(" ");
    for (const phrase of phrases) {
      const wanted = flat(phrase);
      const holders = owned.filter((entry) => entry.text.includes(wanted)).map((entry) => entry.node);
      // A plain promise is a claim about the page, not about one element: the
      // same words are drawn by every assignment card, and `absentCopy` settles
      // them against the whole body. So one element painting them honestly
      // keeps the promise, and only when *none* does is there something to
      // report — otherwise a second, collapsed copy of the same label would
      // convict a screen the reader can read perfectly well.
      //
      // A split phrase is judged the same way, one element short: a candidate
      // set keeps the promise when *every* element in it paints honestly,
      // because a sentence is only readable if all of its parts are.
      const sets = holders.length ? holders.map((node) => [node]) : spans(wanted);
      const honest = sets.find((set) => set.every(readable) && !set.some(speaks) && !set.some(smothered));
      // When no set keeps the promise, the set reported is the one that came
      // closest to keeping it, rather than whichever the scan happened to
      // find first. `sets[0]` starts at the earliest text node, so it is the
      // most over-broad candidate — and reporting from it let a phrase whose
      // readable carriers were painted over be described by a *different*,
      // unreadable set, and, where that first set carried neither fault, be
      // described as clean while no set anywhere kept the promise.
      //
      // A set whose words are all painted can name what was drawn over them;
      // only when no set is painted at all is the phrase unreadable ink. Since
      // `honest` has already failed, an all-readable `nearest` must carry a
      // `speaks` or a `smothered`, so this always reports something.
      const nearest = sets.find((set) => set.every(readable)) ?? sets[0];
      carriers[phrase] = sets.length
        ? {
          found: true,
          ink: Boolean(honest) || nearest.every(readable),
          injected: honest ? "" : nearest.map(speaks).filter(Boolean).join(" "),
          covered: honest ? false : nearest.some(smothered),
        }
        : { found: false, ink: false, injected: "", covered: false };
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
  // React Flow renders every edge caption into one portal — `EdgeLabelRenderer`
  // puts them all in `.react-flow__edgelabel-renderer` — in the order the edges
  // happened to mount, and their place on screen comes from a `transform` this
  // signature deliberately ignores. So document order among them is not a
  // property of the render, and two paths to the same screen need not agree on
  // it: the tab round trip and the direct edit are declared twins, both settled,
  // and were reported broken over `success` and `failure` appearing at swapped
  // indices when both captions were on both screens.
  //
  // Sorting drops the order and keeps the content, which makes the claim "these
  // are the captions drawn" — strictly more truthful than before, because a
  // caption that is genuinely missing or renamed still parts the two lists.
  const isCaption = (node) => (node.getAttribute?.("class") ?? "").split(/\s+/u).includes("react-flow__edge-label");
  const signature = [];
  const captions = [];
  for (const node of doc.querySelectorAll("body *")) {
    if (!boxed(node)) {
      continue;
    }
    const line = [
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
    ].join("|");
    (isCaption(node) ? captions : signature).push(line);
  }
  signature.push(...captions.sort());
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
  //
  // An edge counts as drawn when its path has length *and* that length is
  // painted. `getTotalLength` is SVG geometry, and geometry survives every way
  // there is to not be on screen: one line of `.react-flow__edge-path {
  // display: none }` removed every edge from every graph in the matrix and each
  // one still reported `drawn` equal to `declared`. Length was necessary and
  // was being taken as sufficient.
  //
  // Each clause below is a different way for a path with perfect geometry to
  // ink no pixel, and all of them are asked of the render rather than of one
  // declaration. `display` and `opacity` are walked to the root because neither
  // resolves an ancestor's value into the child's computed one, the way
  // `visibility` does. A stroke that is `none`, fully transparent, or zero-wide
  // is a line that was laid out and never drawn — and a `boxed` test still
  // cannot stand in for any of it, because an edge between two vertically
  // aligned handles is a straight vertical line with no width, which is the
  // ordinary shape of a pipeline stacked in a column.
  const shown = (node) => {
    for (let item = node; item; item = item.parentElement) {
      const style = view.getComputedStyle(item);
      if (style.display === "none" || Number.parseFloat(style.opacity) === 0) {
        return false;
      }
    }
    return true;
  };
  const inked = (style) => {
    const paint = style.stroke ?? "";
    const alpha = paint.startsWith("rgba(") ? Number.parseFloat(paint.split(",")[3]) : 1;
    return paint !== "none"
      && paint !== "transparent"
      && alpha !== 0
      && Number.parseFloat(style.strokeOpacity ?? "1") !== 0
      && Number.parseFloat(style.strokeWidth ?? "1") > 0;
  };
  const drawnPath = (path) => {
    try {
      const style = view.getComputedStyle(path);
      return path.getTotalLength() > 0
        && style.visibility !== "hidden"
        && style.visibility !== "collapse"
        && shown(path)
        && inked(style);
    } catch {
      return false;
    }
  };
  const graphs = [];
  for (const node of doc.querySelectorAll("[data-graph-edges]")) {
    if (boxed(node)) {
      graphs.push({
        // Which surface this is. Three of them publish the attribute — the
        // assignment's pipeline, the editor's canvas and the shared relation
        // graph — and the editor keeps its canvas mounted behind the Relations
        // tab, so more than one can be on a render at once. Without a name the
        // failure reads "a graph declared 4 edges and drew 0", which is two
        // numbers and no screen to go and look at: the same dead end
        // `difference()` was added to the twin audit to remove.
        name: node.getAttribute("aria-label") ?? node.getAttribute("class") ?? "a graph",
        declared: Number(node.getAttribute("data-graph-edges")),
        drawn: [...node.querySelectorAll(".react-flow__edge-path")].filter(drawnPath).length,
      });
    }
  }
  return {
    counts,
    texts,
    paint,
    carriers,
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
  // The other two graph surfaces, for exactly the same reason and by exactly
  // the same layout code. `.flow-card` is only the *viewer's* class: the editor
  // draws `.editor-card` and the shared relation renderer draws
  // `.relation-card`, and neither appeared here — so the overlap rule that
  // exists to catch a bad placement was scoped to one of the three surfaces
  // that can have one. Every editor and relation state in the matrix reported a
  // clean geometry it had never been asked about, which is a mark rather than a
  // check: React Flow gives each node its own absolutely positioned wrapper, so
  // the sibling rule cannot reach them either and nothing else was looking.
  ".editor-card",
  ".relation-card",
];

/**
 * Regions that stack vertically and must never sit on top of one another.
 *
 * The three graph card classes are the exception that is not vertical: a graph
 * places its cards in two dimensions, and the rule there is simply that no two
 * of them may intersect. They belong on this list rather than in `SIBLINGS`
 * because React Flow gives every node its own absolutely positioned wrapper —
 * so the cards share no DOM parent and are not in normal flow, and neither the
 * sibling rule nor the flow rule can reach them. Same-selector comparison can.
 */
const STACKED = [".assignment-card", ".detail-row", ".limit-row", ".repo-row", ".flow-card", ".editor-card", ".relation-card"];

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
 * edge — `data-graph-edges` is the count each surface derives from its own
 * model, at the one place that knows it. Derived from the model and not from
 * the arrays handed to React Flow, so that a projection which drops edges is
 * caught by `undrawnGraphs` rather than quietly lowering the bar to nothing;
 * `web/graph-edges.mjs` carries the reasoning.
 *
 * "No edge is still missing" rather than an exact count: the question being
 * asked is whether the draw pass has happened, and a surface that puts one more
 * element through that selector than it declared has plainly had it.
 *
 * A snapshot that files no graphs at all is *not* drawn. It used to be, on the
 * grounds that a render with no graph on it settles on stability alone — but
 * `collect` files a `graphs` array on every render, empty one included, so the
 * only way to reach this without one is an absent measurement, and this module
 * already has a rule for those: an absent measurement cannot support a pass.
 * The permissive default meant four looks at nothing could report a render
 * settled, which is the same fail-open shape the paint checks were built to
 * reject.
 *
 * Pure, so the offline suite holds the rule without a browser.
 */
export function graphsDrawn(snapshot) {
  return Array.isArray(snapshot?.graphs) && snapshot.graphs.every((graph) => graph.drawn >= graph.declared);
}

/**
 * How many consecutive looks it takes for something to have stopped being
 * flicker.
 *
 * One number, because the two places that ask it are asking one question about
 * one registry: the matrix decides when a signature has stopped changing and
 * when a graph has been undrawn long enough to be a broken screen, and the
 * State Lab decides the second of those about the same render. Two constants
 * that must be equal are a drift waiting to happen, and this module is the one
 * both surfaces can reach — the browser cannot import from `e2e/`.
 */
export const SETTLE_REPEATS = 3;

/**
 * How long the browser suite gives a render to stop changing.
 *
 * Here rather than in `matrix-fixtures.mjs` for the same reason `SETTLE_REPEATS`
 * is: the offline suite has to reach it. `transport:playing` is the one state
 * declared never to settle, and that declaration is only safe while playing the
 * committed run takes materially longer than this budget — otherwise the run
 * would sometimes reach its end, clear `playing`, come to rest, and the claim
 * would flake. `test/statelab.test.mjs` holds that margin against this number,
 * which it can only do if the number has one home.
 */
export const SETTLE_BUDGET_MS = 5000;
/**
 * One look's answer to "has this render stopped changing yet".
 *
 * Settling is two claims, and for a while only one of them was shared. Both
 * consumers imported `graphsDrawn`, so they agreed about a graph mid-draw — and
 * the *stability* half stayed written out at each call site, where the two
 * copies were not the same rule at all. `matrix-fixtures.mjs` required
 * `SETTLE_REPEATS` consecutive looks with an unchanged signature; `lab.mjs`
 * required nothing, and left on the first failure-free look. So the surface a
 * human reviews certified renders the matrix marks: `transport:playing`
 * advances on a 100ms interval and can never hold still, and the lab presented
 * its first frame as verified while the run that gates CI recorded it as not
 * proved settled. Two consumers of one registry answering the same question
 * differently is the contradiction this whole change exists to remove, and
 * sharing only the half that had already been caught left it standing in the
 * other half.
 *
 * It is also the half that catches a *late* failure. Leaving on the first clean
 * look means nothing that arrives afterwards is ever sampled — an error that
 * lands a beat after first paint, a control that disappears once its data
 * resolves — so the panel's green line described a page that no longer existed.
 * Requiring the signature to hold still keeps looking until the render is done
 * arriving, and every look re-takes the verdict.
 *
 * Carries the signature it judged rather than taking a bare previous string, so
 * a caller cannot thread the two apart and compare this look against the wrong
 * one.
 *
 * Pure, so the offline suite holds the rule without a browser and without a
 * clock.
 */
export function settleStep(previous, snapshot) {
  const agreed = previous && snapshot?.signature === previous.signature ? previous.agreed + 1 : 0;
  return { signature: snapshot?.signature, agreed, settled: agreed >= SETTLE_REPEATS && graphsDrawn(snapshot) };
}

/**
 * Why a render could not be proved settled, in words that are true of it.
 *
 * There are three reasons and they used to be reported as one. The note said "a
 * graph on this render has not drawn all of its edges" whenever `settled` was
 * false, which was accurate while `settled` meant `graphsDrawn` alone and became
 * a false statement the moment stability joined it: a replay whose scrubber is
 * advancing has drawn every edge it declared, and telling a reviewer to go and
 * look at its graph sends them after a defect that is not there.
 *
 * Pure, so the offline suite holds the rule without a browser.
 */
export function unsettledReason(snapshot, undrawn) {
  if (undrawn.length) {
    return undrawn.map((item) => item.detail).join("; ");
  }
  if (!Array.isArray(snapshot?.graphs)) {
    return "this render filed no measurement at all, so nothing about it was proved; a snapshot that was never taken is not a screen that failed";
  }
  if (!graphsDrawn(snapshot)) {
    return "a graph on this render has not drawn all of its edges, for too few looks yet to tell a late pass from a broken one";
  }
  return "the DOM never stopped changing inside the settle window, so this is a frame rather than a screen; a state that animates by design is expected here";
}

/**
 * How much of the budget each graph has spent with its edges missing.
 *
 * The observation this replaces was a latch: a graph seen complete once was
 * exempt for the rest of the budget. That was written to tolerate the one thing
 * worth tolerating — a graph caught mid-relayout on the way past — and bought
 * it by exempting the graph *permanently*, which is a much larger claim. Every
 * simpler shape tried since had the same defect somewhere else in it:
 *
 *   a flag per render — a graph that drew excused a *different* graph that
 *     never did, which is `.editor-flow` complete while `.relation-flow` mounts
 *     behind it and draws nothing;
 *   a run of consecutive looks — reset by any complete look, so a graph
 *     flashing its edges on and off all budget never reached the threshold;
 *   a bare total — never reset, and so spent by two harmless relayouts early,
 *     turning one ordinary late miss into a hard failure on exactly the
 *     animating states that must never fail for it.
 *
 * So each graph keeps three numbers and the caller asks two questions of them.
 * `run` answers "did it break and stay broken"; `missed` against `looks`
 * answers "was it chronically broken", which is the one a run cannot ask
 * because a run is reset by the very samples that make it chronic. Neither is
 * derivable from the other, and both are wrong on the case the other catches.
 *
 * Carried across a look the graph is absent from, rather than dropped. Dropping
 * was a third reset: a graph alternating between unmounted and incomplete never
 * accumulated anything. Nothing stale escapes, because `undrawnFor` only ever
 * asks about a graph that is present *and* incomplete on the final look.
 *
 * Keyed by name, which is the surface's identity and what the failure speaks
 * in.
 *
 * Pure, so the offline suite holds the rule without a browser.
 */
export function undrawnLooks(previous, snapshot) {
  const looks = new Map(previous);
  for (const graph of snapshot?.graphs ?? []) {
    const before = looks.get(graph.name) ?? { looks: 0, missed: 0, run: 0 };
    const behind = graph.drawn < graph.declared;
    looks.set(graph.name, { looks: before.looks + 1, missed: before.missed + (behind ? 1 : 0), run: behind ? before.run + 1 : 0 });
  }
  return looks;
}

/**
 * The share of its looks a graph has to have spent incomplete before that is a
 * property of the screen rather than of the machine the run is on.
 *
 * A third, which is far above what a contended worker produces — the whole
 * matrix reports two unsettled renders on a clean run — and far below the half
 * a graph alternating drawn and undrawn produces. It is only ever consulted
 * about a graph that is *also* incomplete at the deadline and has missed at
 * least `SETTLE_REPEATS` looks, so it cannot fire on a render that merely
 * blinked.
 */
const CHRONIC_SHARE = 3;

/**
 * The graphs whose edges were missing enough of the time to be a screen rather
 * than a frame, reported as failures.
 *
 * Asked only of graphs still incomplete on this look, which is what keeps a
 * late relayout from becoming an accusation: a graph that finished, however
 * raggedly, has nothing reported about it at all.
 *
 * Pure, so the offline suite holds the rule without a browser.
 */
export function undrawnFor(snapshot, looks, sustained) {
  return undrawnGraphs(snapshot).filter((finding) => chronic(looks.get(finding.name), sustained));
}

function chronic(tally, sustained) {
  if (!tally) {
    return false;
  }
  return tally.run >= sustained || (tally.missed >= sustained && tally.missed * CHRONIC_SHARE >= tally.looks);
}

/**
 * A graph that never drew its edges, reported as the defect it is.
 *
 * `graphsDrawn` above decides when a render is *finished*, and that was the
 * whole of what the count was used for: a graph still mid-draw held the settle
 * loop open, and a graph that would never finish held it open until the budget
 * ran out and then left with an amber note on its figure. The note says "this
 * frame may have raced", which is a claim about the harness — so a surface that
 * permanently draws none of the edges it declared produced no failure anywhere,
 * and the matrix stayed green while the gallery quietly published pipelines
 * whose steps joined to nothing.
 *
 * That is the same shape as every other defect this registry exists to catch,
 * and the one this harness kept making about itself: an amber mark standing in
 * for a check that found something. `settled` and "the state passed" are still
 * two questions — a page that animates by design never settles and is not
 * failing — but "a graph declared four edges and drew none, for the entire
 * budget" is not an unfinished frame, it is a broken screen.
 *
 * Asked of a graph that has spent enough of the budget incomplete to be a
 * screen rather than a frame, and never of one that finished. Mid-relayout is
 * the ordinary case on the way through, and a single late sample under a
 * contended worker is the flake the rest of this module is written to avoid —
 * but a surface that spends the budget with its edges missing is broken, and
 * `undrawnLooks` is what tells those apart.
 *
 * Carries the graph's name as well as the sentence, so `undrawnFor` can match a
 * finding to the run it belongs to without re-deriving it.
 *
 * Pure, so the offline suite holds the rule without a browser.
 */
export function undrawnGraphs(snapshot) {
  return (snapshot?.graphs ?? [])
    .filter((graph) => graph.drawn < graph.declared)
    .map((graph) => ({
      kind: "undrawn-graph",
      name: graph.name,
      detail: `${graph.name ?? "a graph"} declared ${graph.declared} edge(s) and drew ${graph.drawn}`,
    }));
}

export function verdict(state, snapshot, options = {}) {
  return [
    ...missing(state, snapshot),
    ...forbidden(state, snapshot),
    ...absentCopy(state, snapshot),
    ...paintedCopy(state, snapshot),
    ...paintedPhrases(state, snapshot),
    ...promisedCopy(state, snapshot),
    ...lowContrast(snapshot, options),
    ...strandedLabels(snapshot),
    ...permitted(state, overlaps(snapshot)),
    ...clipping(snapshot, options),
  ];
}

/**
 * Drops the overlaps a state declared, and only those.
 *
 * Bringing `.editor-card` under the overlap rule immediately found a collision,
 * and reading it is the whole of why this exists. Every editor state in the
 * matrix draws a layout `lib/layout.mjs` computed, and none of those overlap.
 * One state does: `edit:layout-moved`, whose entry path *is* a drag of 80,60 —
 * the reader picked a card up and put it down on its neighbour, which is the
 * feature working. A node a user has dragged is theirs to place, and a rule that
 * forbade it would be asserting the opposite of what the surface offers.
 *
 * The allowance names the colliding pair exactly, and is matched by equality.
 * It was a `startsWith` on the selector for a while, which read as narrow and
 * was not: `.editor-card` is the prefix of *every* editor-card collision, so
 * that one word excused the second and third as readily as the first, and a
 * layout regression that scattered every card in this state could not fail it.
 * The comment above already claimed the narrow reading — "*these* cards may sit
 * on one another" — so the words were right and the code was not.
 *
 * The claim runs both ways. An allowance that matched nothing is reported as
 * stale rather than passing quietly, because a licence for a collision that no
 * longer happens is how the excuse outlives the reason and is waiting, already
 * granted, for an unrelated defect to walk into it.
 */
export function permitted(state, found) {
  const allowed = state.expect?.allowOverlap ?? [];
  return [
    ...found.filter((problem) => !allowed.includes(problem.detail)),
    ...allowed
      .filter((detail) => !found.some((problem) => problem.detail === detail))
      .map((detail) => ({ kind: "stale-overlap-allowance", detail: `${detail} is excused here but did not happen` })),
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

/**
 * The promised words are the words on screen.
 *
 * `absentCopy` compares an element's `innerText` against the sentence its state
 * promises, and `innerText` answers for the DOM rather than for a reader: it
 * reports words drawn in transparent ink, and it does not report a `::before`
 * or `::after` at all. So a stylesheet that hides an element's own sentence and
 * spells a different one in generated content satisfies the exact-text
 * expectation with every gate green — 681 browser checks and 452 offline ones —
 * while the panel tells a reader the opposite of what the validator said, a
 * rejected config reported as `clean — bureau validate would pass`.
 *
 * That is the family `visible()` already defends against, one level down. The
 * walk above asks whether a promised element paints *anything*; this asks
 * whether what it paints is its *own words*, which is the question an
 * exact-text promise is actually making.
 *
 * Both halves are asserted, because either alone is half a check. Ink without
 * generated content passes a false sentence layered over the true one; generated
 * content without ink passes a true sentence painted in nothing. Judged only
 * where a state names an element and the words it must read, since that is the
 * only place the registry claims to know what a reader sees.
 */
function paintedCopy(state, snapshot) {
  return scopedCopy(state).flatMap((phrase) => {
    const paint = snapshot.paint?.[phrase.selector];
    const problems = [];
    if (!paint) {
      problems.push({ kind: "unreadable-copy", detail: `${phrase.selector} has no paint sample, so its words were not proved readable` });
    } else if (paint.ink === false) {
      problems.push({ kind: "unreadable-copy", detail: `${phrase.selector} draws its own words in ink a reader cannot see` });
    }
    if (paint?.injected) {
      problems.push({ kind: "substituted-copy", detail: `${phrase.selector} paints ${paint.injected} in place of its own words` });
    }
    return problems;
  });
}

/**
 * The same judgement, for the promises that name no element.
 *
 * A plain phrase is settled against the body's `innerText`, so every way of
 * showing a reader something other than the promised words was open to it —
 * and most of this registry's copy is plain, which made the scoped check a
 * guard over the minority of its own subject.
 *
 * A missing sample is still a failure in its own right, for the reason the
 * scoped branch learned it: an absent measurement cannot support a pass.
 *
 * So is a phrase no element carries, and that is the correction the previous
 * round made. `found: false` used to be *exempt*, on the reasoning that it
 * meant the words were split across elements and `absentCopy` had already
 * proved them on the page. Both halves of that were wrong. `absentCopy` reads
 * `innerText`, which is the DOM-not-reader standard this module exists to
 * reject — it reports words drawn in transparent ink — so the exemption excused
 * precisely the defect the check was written for: a split sentence painted in
 * nothing passed both. And split words are no longer routed here at all,
 * because `collect` now finds the *set* of elements that carries them and
 * judges every one. What reaches `found: false` now is a phrase no run of text
 * nodes spans, which is a promise nothing on the page was measured against.
 *
 * `covered` is the third verdict, and it is a substitution rather than an
 * absence: the words are painted, honestly, and an ancestor's own generated
 * layer is painted in front of them. `injected` cannot report it, because that
 * one reads the carrier's own `::before`/`::after` and a wrapper's belongs to
 * the wrapper.
 */
function paintedPhrases(state, snapshot) {
  return phrasesFor(state).flatMap((phrase) => {
    const paint = snapshot.carriers?.[phrase];
    if (!paint) {
      return [{ kind: "unreadable-copy", detail: `“${phrase}” has no paint sample, so its words were not proved readable` }];
    }
    if (!paint.found) {
      return [{ kind: "unreadable-copy", detail: `“${phrase}” is carried by no element, so its words were not proved readable` }];
    }
    return [
      ...(paint.ink === false ? [{ kind: "unreadable-copy", detail: `“${phrase}” is drawn in ink a reader cannot see` }] : []),
      ...(paint.injected ? [{ kind: "substituted-copy", detail: `“${phrase}” has ${paint.injected} painted over it` }] : []),
      ...(paint.covered ? [{ kind: "substituted-copy", detail: `“${phrase}” has a layer of its own page painted over it` }] : []),
    ];
  });
}

/** One stable string per expectation, so a failure names what was promised. */
export function copyLabel(phrase) {
  return typeof phrase === "object" && phrase !== null ? `${phrase.selector} reads exactly “${phrase.text}”` : phrase;
}

/**
 * Whether a failure is *about this promise*, in the terms a reviewer reads.
 *
 * The State Lab prints one row per promised sentence, and that row asked only
 * whether the sentence was `missing-copy`. So the two verdicts this module
 * gained for the screen rather than the DOM — words drawn in ink nobody can
 * see, and words a generated layer paints in their place — arrived at the panel
 * as a note *beneath* the list while the row naming the sentence stayed ticked.
 * That is the wrong way round for a review surface: a reviewer looking at the
 * line for that sentence was told it was fine.
 *
 * It was also the shape a check cannot keep. The note was the only consumer of
 * either kind, so both could be dropped from its filter with every test in the
 * repository still green and the panel merely more reassuring.
 *
 * Judged here rather than in `lab.mjs` because this module owns what a failure
 * kind means; the lab owns how a row looks. The paint verdicts name their
 * selector at the head of `detail`, which is the only handle they carry back to
 * the promise that asked for them, so an unscoped phrase can never claim one.
 */
export function copyFailure(item, phrase, label) {
  if (item.kind === "missing-copy") {
    return item.detail === label;
  }
  if (!["unreadable-copy", "substituted-copy"].includes(item.kind)) {
    return false;
  }
  return typeof phrase === "object" && phrase !== null
    ? item.detail.startsWith(`${phrase.selector} `)
    : item.detail.startsWith(`“${phrase}” `);
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

/** The copy expectations that name only words, and so must be found by them. */
export function phrasesFor(state) {
  return [...new Set((state.expect?.copy ?? []).filter((phrase) => typeof phrase === "string"))];
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
