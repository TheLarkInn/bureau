// The gallery index's template, owned in one place.
//
// It lives here rather than in `specs/state-matrix.spec.mjs` because two
// modules have to agree on it and only one of them writes it. The spec builds
// the index; `global-teardown.mjs` marks findings into the index afterwards,
// by replacing `NOTICE_ANCHOR` and each render's `figureTag(...)`. While the
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

export function escape(value) {
  return String(value).replace(/[&<>"]/gu, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

/** The opening tag a mark is attached to. The single source of that shape. */
export function figureTag(shot) {
  return `<figure data-shot="${escape(shot)}">`;
}

/** A crossing names the rule it breaks; a content sample names what it covers. */
function describeProbe(state) {
  if (state.rule) {
    return ` · crossing excluded by ${escape(state.rule)}`;
  }
  return state.covers ? ` · covers ${escape(state.covers)}` : "";
}

function figureFor(state, viewport, shot) {
  return `${figureTag(shot)}<img loading="lazy" src="./${shot}" alt="${escape(state.id)} at ${viewport.id}"><figcaption>${escape(viewport.id)}</figcaption></figure>`;
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
 * The shell is here with `NOTICE_ANCHOR` and `figureTag` because it carries
 * both of their counterparts: the `<main>` a banner is inserted before, and the
 * `figure[data-settled="false"]` rules that draw a mark once one is attached. A
 * mark whose CSS lived in another file could land and still be invisible.
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
     is about that screen. */
  figure[data-settled="false"] img { border-color:#9a6700; border-width:2px; }
  figure[data-settled="false"] figcaption::after { content:" · not proved settled"; color:#9a6700; font-weight:700; }
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
    const tag = figureTag(shot);
    const marked = replaceOnce(page, tag, `<figure data-shot="${escape(shot)}" data-settled="false">`);
    if (marked === null) {
      unmarked.push(shot);
    } else {
      page = marked;
    }
  }
  return { html: page, unmarked };
}
