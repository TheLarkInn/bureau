// One headless suite over the shared bundle (design Q12): host-independence
// by construction. Both hosts — the Copilot canvas host (extension.mjs) and
// the standalone host (serve.mjs) — serve and load exactly the same web
// modules, so these tests pin the sharing instead of re-testing the renderer.
// No DOM, no browser: the DOM-facing entry modules declare bare imports
// ("react", "@xyflow/react") that only the import map satisfies, so the
// checks work on source text rather than importing them.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

const EXTENSION = new URL("../", import.meta.url);
const VENDORED_IMPORTS = {
  "react": "./vendor/react.mjs",
  "react/jsx-runtime": "./vendor/react-jsx-runtime.mjs",
  "react-dom": "./vendor/react-dom.mjs",
  "react-dom/client": "./vendor/react-dom-client.mjs",
  "@xyflow/react": "./vendor/xyflow-react.mjs",
};

async function source(path) {
  return readFile(new URL(path, EXTENSION), "utf8");
}

/** Relative and bare module specifiers one web module asks the runtime for. */
function specifiers(text) {
  return [...text.matchAll(/(?:from\s+|import\s*\(\s*)["']([^"']+)["']/gu)].map((match) => match[1]);
}

/** Both pages plus the standalone launcher and the canvas host's entry. */
async function sharedBundleFacts() {
  const [index, editor, serve, app] = await Promise.all([
    source("web/index.html"),
    source("web/editor.html"),
    source("serve.mjs"),
    source("web/app.mjs"),
  ]);
  return { index, editor, serve, app };
}

test("both hosts open the same canvas over the same endpoint code", async () => {
  const { serve } = await sharedBundleFacts();
  const extension = await source("extension.mjs");

  assert.deepStrictEqual(
    {
      launcherSetsNoSdk: serve.includes('process.env.BUREAU_CANVAS_NO_SDK = "1"'),
      launcherImports: serve.includes('await import("./extension.mjs")'),
      launcherCallsOpen: serve.includes("openBureauCanvas("),
      sdkGated: extension.includes('process.env.BUREAU_CANVAS_NO_SDK !== "1"'),
      sdkIsolated: (extension.match(/copilot-sdk/gu) ?? []).length,
      sdkImportIsDynamic: extension.includes('await import("@github/copilot-sdk/extension")'),
    },
    {
      launcherSetsNoSdk: true,
      launcherImports: true,
      launcherCallsOpen: true,
      sdkGated: true,
      sdkIsolated: 1,
      sdkImportIsDynamic: true,
    },
  );
});

test("both pages pin identical import maps to the vendored modules", async () => {
  const { index, editor } = await sharedBundleFacts();
  const mapOf = (html) => JSON.parse(html.match(/<script type="importmap">\s*(\{[\s\S]*?\})\s*<\/script>/u)[1]).imports;

  assert.deepStrictEqual(
    { index: mapOf(index), editor: mapOf(editor), bare: Object.keys(mapOf(index)).sort() },
    { index: VENDORED_IMPORTS, editor: VENDORED_IMPORTS, bare: Object.keys(VENDORED_IMPORTS).sort() },
  );
});

test("the standalone page has no fork of the canvas host's module graph", async () => {
  const { index, editor, app } = await sharedBundleFacts();
  const editorEntry = await source("web/editor/index.mjs");
  // The standalone host serves both pages from the same web/ tree; the app
  // host loads index.html. Forks would show up as html committed module
  // bodies or entry imports outside the shared entry points.
  const entries = {
    indexEntry: /await import\("\.\/app\.mjs"\)/u.test(index),
    editorEntry: /await import\("\.\/editor\/index\.mjs"\)/u.test(editor),
    appImportsShared: ["/modes.js", "/live/live.js", "/replay/replay.js", "/live/overlay.js"].every((part) =>
      specifiers(app).includes(`.${part}`),
    ),
    editorImportsShared: ["./editor.mjs", "./relation.mjs"].every((part) => specifiers(editorEntry).includes(part)),
  };

  assert.deepStrictEqual(entries, { indexEntry: true, editorEntry: true, appImportsShared: true, editorImportsShared: true });
});

test("web modules import only shared siblings and the pinned vendor aliases", async () => {
  const files = [
    "web/app.mjs",
    "web/modes.js",
    "web/live/live.js",
    "web/live/overlay.js",
    "web/replay/replay.js",
    "web/editor/index.mjs",
    "web/editor/editor.mjs",
    "web/editor/relation.mjs",
    // The state lab is served from the same tree by the same host, so it is
    // held to the same rule: siblings and the pinned aliases, nothing else.
    "web/statelab/registry.mjs",
    "web/statelab/dimensions.mjs",
    "web/statelab/constraints.mjs",
    "web/statelab/enumerate.mjs",
    "web/statelab/paths.mjs",
    "web/statelab/probes.mjs",
    "web/statelab/fixtures.mjs",
    "web/statelab/selectors.mjs",
    "web/statelab/driver.mjs",
    "web/statelab/checks.mjs",
    "web/statelab/dom-adapter.mjs",
    "web/statelab/lab.mjs",
  ];
  const offenders = [];
  for (const file of files) {
    for (const specifier of specifiers(await source(file))) {
      if (specifier.startsWith(".") || specifier.startsWith("@xyflow/") || specifier.startsWith("react")) {
        continue;
      }
      offenders.push(`${file} -> ${specifier}`);
    }
  }

  assert.deepStrictEqual(offenders, []);
});

test("the state lab reads the production page rather than forking it", async () => {
  const lab = await source("web/statelab.html");
  const labModule = await source("web/statelab/lab.mjs");
  const adapter = await source("web/statelab/dom-adapter.mjs");
  // The lab renders states by loading index.html/editor.html into a frame. If
  // it ever grew its own copy of a canvas component, the matrix would be
  // reviewing something the user never sees.
  assert.deepStrictEqual(
    {
      framesTheRealPage: /<iframe[^>]*id="stage-frame"/u.test(lab),
      loadsProductionPages: ["./editor.html", "./index.html"].every((page) => adapter.includes(page)),
      importsNoCanvasComponent: !specifiers(labModule).some((item) => /app\.mjs|editor\/editor\.mjs|editor\/relation\.mjs/u.test(item)),
      drivenByTheRegistry: specifiers(labModule).includes("./registry.mjs"),
    },
    { framesTheRealPage: true, loadsProductionPages: true, importsNoCanvasComponent: true, drivenByTheRegistry: true },
  );
});

test("the fallback state loader uses the same endpoints the app does", async () => {
  const { index } = await sharedBundleFacts();
  // index.html's no-renderer fallback and app.mjs both read /state; if one
  // moved without the other, one host would silently render stale chrome.
  const fetchEndpoints = [...index.matchAll(/fetch\("(\.\/[^"]+)"/gu)].map((match) => match[1]);
  const appEndpoints = [...(await source("web/app.mjs")).matchAll(/fetch\("(\.\/[^"]+)"/gu)].map((match) => match[1]);
  const eventSources = [...index.matchAll(/import\("\.(\/[^"]+)"\)/gu)].map((match) => match[1]);

  assert.deepStrictEqual(
    { fallbackState: fetchEndpoints.includes("./state"), appState: appEndpoints.includes("./state"), entries: eventSources },
    { fallbackState: true, appState: true, entries: ["/app.mjs"] },
  );
});
