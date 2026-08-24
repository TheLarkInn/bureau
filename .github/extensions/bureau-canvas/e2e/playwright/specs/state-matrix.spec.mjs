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

import { STATES, TRANSITIONS } from "../../../web/statelab/registry.mjs";
import { VIEWPORTS } from "../../../web/statelab/selectors.mjs";
import { shotName } from "../gallery-audit.mjs";
import { enterState, applyOps, expect, galleryDir, test } from "../matrix-fixtures.mjs";

const VIEWPORT_LIST = Object.values(VIEWPORTS);

function shot(state, viewport) {
  return shotName(state.id, viewport.id);
}

/**
 * The render's DOM signature, filed beside its screenshot for the teardown to
 * audit. One small file per render rather than one shared file, because the
 * renders are written by several workers at once and a shared file is a race.
 * `global-teardown.mjs` collapses them into the gallery's `signatures.json`.
 */
async function fileSignature(name, signature) {
  const dir = join(galleryDir(), "signatures");
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, `${name}.sha`), createHash("sha256").update(signature ?? "").digest("hex"), "utf8");
}

for (const viewport of VIEWPORT_LIST) {
  test.describe(`@matrix ${viewport.id} (${viewport.width}×${viewport.height})`, () => {
    for (const state of STATES) {
      test(`renders ${state.id}`, async ({ watched, host }, testInfo) => {
        await watched.page.setViewportSize({ width: viewport.width, height: viewport.height });
        const result = await enterState(state, watched.page, host);

        await watched.page.screenshot({ path: join(galleryDir(), shot(state, viewport)), fullPage: true });
        await fileSignature(shot(state, viewport), result.snapshot?.signature);
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
  const rows = STATES.map((state) => `
    <article class="card" id="${escape(state.id)}">
      <h2>${escape(state.id)}</h2>
      <p class="muted">${escape(state.summary ?? "")}</p>
      <p class="meta">${escape(state.kind)}${describeProbe(state)} · fixture ${escape([].concat(state.fixture ?? []).join(" + ") || "none")}</p>
      <div class="shots">
        ${VIEWPORT_LIST.map((viewport) => `<figure><img loading="lazy" src="./${shot(state, viewport)}" alt="${escape(state.id)} at ${viewport.id}"><figcaption>${viewport.id}</figcaption></figure>`).join("")}
      </div>
    </article>`).join("");

  await writeFile(join(galleryDir(), "index.html"), page(rows), "utf8");

  const written = await readFile(join(galleryDir(), "index.html"), "utf8");
  const missing = STATES.flatMap((state) =>
    VIEWPORT_LIST
      .filter((viewport) => !written.includes(`src="./${shot(state, viewport)}"`))
      .map((viewport) => `${state.id} @ ${viewport.id}`));
  expect(missing, "states the gallery index does not link").toEqual([]);
});

function page(rows) {
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
</style></head>
<body>
<header><h1>Bureau Canvas state gallery</h1><p class="muted">${STATES.length} states × ${VIEWPORT_LIST.length} viewports, rendered by the production page.</p></header>
<main>${rows}</main>
</body></html>`;
}

function escape(value) {
  return String(value).replace(/[&<>"]/gu, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

/** A crossing names the rule it breaks; a content sample names what it covers. */
function describeProbe(state) {
  if (state.rule) {
    return ` · crossing excluded by ${escape(state.rule)}`;
  }
  return state.covers ? ` · covers ${escape(state.covers)}` : "";
}
