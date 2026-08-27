// The gallery index's template, owned in one place.
//
// It lives here rather than in `specs/state-matrix.spec.mjs` because two
// modules have to agree on it and only one of them writes it. The spec builds
// the index; `global-teardown.mjs` marks findings into the index afterwards,
// by replacing `NOTICE_ANCHOR` and each render's `figurePrefix(...)`. While the
// template was private to the spec, that agreement was a coincidence of two
// string literals in different files: an attribute added to `<figure>`, or a
// `<main>` that gained one, turned both replacements into silent no-ops. The
// red "not the whole matrix" banner and every amber "not proved settled" mark
// would simply stop appearing, and nothing would fail — an unmarked gallery is
// indistinguishable from a clean one.
//
// That is the exact defect this suite exists to remove, so the anchors are
// exported, both sides import them, and `applyMarks` reports what it did not
// find rather than returning the html unchanged. `test/gallery-audit.test.mjs`
// asserts the round trip against this module's own output rather than against a
// hand-written literal, so the two can no longer drift apart unnoticed.

/** Where the notices are inserted. Both the writer and the marker use this. */
export const NOTICE_ANCHOR = "<main>";

/**
 * The attribute an unsettled figure is stamped with, and the one the page's
 * styling selects. One string, for the same reason the anchors are exported.
 *
 * Co-location in this file is not a binding: while the attribute was spelled out
 * once in `applyMarks` and twice in the stylesheet, renaming it in the sheet
 * alone left every mark landing correctly, `unmarked` empty, the gate green —
 * and not one of the marks visible to the reviewer they are drawn for. The mark
 * was anchored; its *rendering* was not. `gallery-audit.test.mjs` reads the
 * attribute back out of a stamped figure and requires the stylesheet to select
 * that, so the two ends cannot be renamed apart.
 */
export const SETTLED_MARK = 'data-settled="false"';

/**
 * The words a mark promises. One string, for the same reason the attribute is.
 *
 * It is drawn by the stylesheet below, named in the amber notice, and looked for
 * by `@matrix an unsettled figure is drawn unlike a settled one`. While it was
 * spelled separately in all three, the browser check was hunting for a sentence
 * the sheet no longer wrote: renaming the mark's wording in one place left the
 * check green about a phrase that appears nowhere, which is a check that has
 * stopped reading the product.
 */
export const SETTLED_PHRASE = "not proved settled";

/**
 * The ink a mark is drawn in. One string, for the same reason the phrase is —
 * and here the second spelling would have been in the check rather than in the
 * product.
 *
 * `@matrix an unsettled figure is drawn unlike a settled one` requires this
 * exact colour to appear in the pixels of the stamped figure's border, which is
 * the only question a rule that paints nothing cannot pass. Spelled separately
 * there, restyling the mark would have left the check hunting a colour the sheet
 * had stopped drawing — green about ink that is on no page.
 */
export const SETTLED_INK = "#9a6700";

export function escape(value) {
  return String(value).replace(/[&<>"]/gu, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

/**
 * The part of a figure's opening tag that identifies it, up to and including the
 * closing quote of `data-shot`. This is the anchor a mark is found by.
 *
 * It stops before the tag's `>` so the marker can find a figure whose tag
 * carries attributes beyond the one it is named by. Searching for the whole tag
 * meant the marker could only ever be handed tags of exactly the shape
 * `figureTag` writes today, so "does the marker preserve what it did not write"
 * was not a question this suite could ask at all — every fixture that could have
 * asked it was unfindable by construction.
 *
 * The quote is part of the anchor deliberately: without it, a shot name that is
 * a prefix of another would mark the wrong figure.
 */
export function figurePrefix(shot) {
  return `<figure data-shot="${escape(shot)}"`;
}

/** The opening tag a mark is attached to. The single source of that shape. */
export function figureTag(shot) {
  return `${figurePrefix(shot)}>`;
}

/**
 * An opening tag, stamped with the mark.
 *
 * The mark is *inserted* before the tag's `>`, so whatever attributes the tag
 * already carried survive it. It is a function of the tag rather than of the
 * shot because `applyMarks` hands it the tag as it stands on the page, which
 * may carry attributes this module never wrote — a rebuilt tag would drop
 * exactly those, silently, on the marked figures alone.
 */
export function markTag(tag) {
  return `${tag.slice(0, -1)} ${SETTLED_MARK}>`;
}

/** A crossing names the rule it breaks; a content sample names what it covers. */
function describeProbe(state) {
  if (state.rule) {
    return ` · crossing excluded by ${escape(state.rule)}`;
  }
  return state.covers ? ` · covers ${escape(state.covers)}` : "";
}

function figureFor(state, viewport, shot) {
  return `${figureTag(shot)}<img loading="lazy" src="./${escape(shot)}" alt="${escape(state.id)} at ${viewport.id}"><figcaption>${escape(viewport.id)}</figcaption></figure>`;
}

/** One state's card, linking its render at every viewport. */
export function rowsFor(states, viewports, shot) {
  return states.map((state) => `
    <article class="card" id="${escape(state.id)}">
      <h2>${escape(state.id)}</h2>
      <p class="muted">${escape(state.summary ?? "")}</p>
      <p class="meta">${escape(state.kind)}${describeProbe(state)} · fixture ${escape([].concat(state.fixture ?? []).join(" + ") || "none")}</p>
      <div class="shots">
        ${viewports.map((viewport) => figureFor(state, viewport, shot(state, viewport))).join("")}
      </div>
    </article>`).join("");
}

/**
 * The whole index page.
 *
 * The shell is here with `NOTICE_ANCHOR` and `SETTLED_MARK` because it carries
 * both of their counterparts: the `<main>` a banner is inserted before, and the
 * rules that draw a mark once one is attached. Being in one file is not what
 * holds them together — the stylesheet interpolates the same constant the
 * marker writes, so there is one attribute rather than three spellings of it.
 * A mark whose selector had drifted would land and still be invisible.
 */
export function indexPage(rows, states, viewports) {
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Bureau Canvas state gallery</title>
<style>
  :root { color-scheme: light dark; --border:#d0d7de; --muted:#656d76; }
  body { margin:0; font:14px/20px -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; }
  header { padding:1rem 1.5rem; border-bottom:1px solid var(--border); }
  h1 { font-size:1.25rem; margin:0; }
  main { display:grid; gap:1.5rem; padding:1.5rem; }
  .card { border:1px solid var(--border); border-radius:.625rem; padding:1rem; }
  .card h2 { font-size:1rem; margin:0 0 .25rem; font-family:"SFMono-Regular",Consolas,monospace; }
  .muted,.meta { color:var(--muted); margin:.25rem 0; }
  .meta { font-size:12px; }
  .shots { display:grid; grid-template-columns:repeat(auto-fit,minmax(20rem,1fr)); gap:1rem; margin-top:.75rem; }
  figure { margin:0; }
  img { width:100%; border:1px solid var(--border); border-radius:6px; display:block; }
  figcaption { color:var(--muted); font-size:12px; padding-top:.25rem; }
  /* Stamped by global-teardown.mjs on renders whose DOM never stopped changing
     inside the settle budget. The marker sits on the figure a reviewer is
     looking at rather than only in the banner, because the judgement being made
     is about that screen. The selector is interpolated from the same constant
     applyMarks writes, so a mark can never land under an attribute this sheet
     does not draw. */
  figure[${SETTLED_MARK}] img { border-color:${SETTLED_INK}; border-width:2px; }
  figure[${SETTLED_MARK}] figcaption::after { content:" · ${SETTLED_PHRASE}"; color:${SETTLED_INK}; font-weight:700; }
</style></head>
<body>
<header><h1>Bureau Canvas state gallery</h1><p class="muted">${states.length} states × ${viewports.length} viewports, rendered by the production page.</p></header>
${NOTICE_ANCHOR}${rows}</main>
</body></html>`;
}

/**
 * Replaces `find` once, with `$` in the replacement taken literally.
 *
 * A string pattern makes `String.prototype.replace` honour `$&`, `` $` ``, `$'`
 * and `$n` *in the replacement*, and the notices embed product-derived text —
 * a signature line quoted back from a rendered page. A `$&` reaching the banner
 * would corrupt it, so the replacement is supplied as a function, for which no
 * such expansion happens.
 */
function replaceOnce(html, find, replacement) {
  const at = html.indexOf(find);
  return at === -1 ? null : `${html.slice(0, at)}${replacement}${html.slice(at + find.length)}`;
}

/**
 * Applies the gallery's marks and says which ones found nothing to attach to.
 *
 * The return is `{ html, unmarked }` rather than just html so a mark that did
 * not land is a finding, not a silence. `unmarked` names the anchor that was
 * missing — `notices` when the banner had nowhere to go, or the shot whose
 * figure was not found — which is what a reviewer needs to know, because the
 * page still renders perfectly well without them and looks clean while doing it.
 *
 * A figure is found by `figurePrefix` and stamped by `markTag` over the tag as
 * it was actually read off the page — never a second spelling of that tag.
 * While the search was for the whole of `figureTag(shot)`, the two ends were
 * bound in the *search* direction only: an attribute added to `figureTag` was
 * still found, and then dropped from every figure the marker rewrote, so the
 * marked page silently lost it while the unmarked figures kept it, `unmarked`
 * stayed empty and the gate stayed green. Rebuilding was undetectable even
 * after `markTag` existed, because a tag carrying an unknown attribute could
 * not be handed to `applyMarks` at all: the whole-tag search could never find
 * one. Searching by prefix is what makes the question askable, and insertion
 * over the read tag is what answers it — insertion cannot drop what it never
 * re-spells.
 *
 * Scanning to the tag's first `>` is exact because every attribute a figure
 * carries is written through `escape`, which leaves no raw `>` in a value. Any
 * attribute added to `figureTag` must keep that true.
 */
export function applyMarks(html, notice, unsettled) {
  const unmarked = [];
  let page = html;
  if (notice) {
    const withNotice = replaceOnce(page, NOTICE_ANCHOR, `${notice}${NOTICE_ANCHOR}`);
    if (withNotice === null) {
      unmarked.push("notices");
    } else {
      page = withNotice;
    }
  }
  for (const shot of unsettled) {
    const marked = markFigure(page, figurePrefix(shot));
    if (marked === null) {
      unmarked.push(shot);
    } else {
      page = marked;
    }
  }
  return { html: page, unmarked };
}

/** Stamps the first figure whose tag opens with `prefix`, as that tag stands. */
function markFigure(html, prefix) {
  const at = html.indexOf(prefix);
  if (at === -1) {
    return null;
  }
  const end = html.indexOf(">", at + prefix.length);
  if (end === -1) {
    return null;
  }
  return `${html.slice(0, at)}${markTag(html.slice(at, end + 1))}${html.slice(end + 1)}`;
}
