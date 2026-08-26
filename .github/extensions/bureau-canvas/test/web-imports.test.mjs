// Every module the browser loads has to be reachable by the browser.
//
// The dashboard serves `web/` and nothing above it — `staticPath` resolves
// under that directory and refuses anything that escapes it — so a module in
// `web/` that imports `../../lib/…` is a 404 at runtime, not a build error. The
// page then fails in the least legible way available: the import throws, the
// surface never mounts, and every browser spec that needed it times out waiting
// for an element that was never going to appear. That cost a five-minute
// Playwright run to diagnose, thirty-nine red specs pointing at a shell that
// simply never rendered.
//
// So the boundary is checked here instead, in milliseconds and offline: every
// relative specifier under `web/` resolves to a file inside `web/`, and every
// bare specifier is one the pages actually declare an import map entry for.
// `vendor/` is excluded — those are third-party bundles, shipped as fetched.
//
// The host side needs no such rule: it reads files off disk, which is why the
// one rule both trees share lives on the reachable side, in `web/step-refs.mjs`,
// and `lib/edit.mjs` imports it rather than the other way round.

import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const WEB = fileURLToPath(new URL("../web/", import.meta.url));

/** Every specifier a module names, however it names it. */
const SPECIFIERS = [
  /\bfrom\s*["']([^"'\n]+)["']/gu,
  /\bimport\s*\(\s*["']([^"'\n]+)["']/gu,
  /\bimport\s*["']([^"'\n]+)["']/gu,
];

/**
 * The source with its commentary removed.
 *
 * These modules explain themselves at length, and that prose quotes module
 * names and uses the word `from`, so scanning the raw text reported a
 * paragraph in `registry.mjs` as an unmapped import. Line comments are cut only
 * where `//` does not follow a colon, so the `https://` inside a real string
 * survives intact.
 */
function withoutComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//gu, "").replace(/(^|[^:])\/\/[^\n]*/gu, "$1");
}

async function scriptsUnder(directory) {
  const found = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory() && entry.name !== "vendor") {
      found.push(...await scriptsUnder(path));
    } else if (entry.isFile() && /\.m?js$/u.test(entry.name)) {
      found.push(path);
    }
  }
  return found;
}

function specifiersIn(source) {
  const found = new Set();
  for (const pattern of SPECIFIERS) {
    for (const [, specifier] of withoutComments(source).matchAll(pattern)) {
      found.add(specifier);
    }
  }
  return [...found].sort();
}

/** The bare specifiers the pages map, which are the only ones a module may use. */
async function mappedSpecifiers() {
  const pages = (await readdir(WEB, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".html"));
  const mapped = new Set();
  for (const page of pages) {
    const source = await readFile(join(WEB, page.name), "utf8");
    const map = source.match(/<script type="importmap">([\s\S]*?)<\/script>/u);
    for (const name of Object.keys(JSON.parse(map?.[1] ?? "{}").imports ?? {})) {
      mapped.add(name);
    }
  }
  return mapped;
}

/**
 * Why a module's specifier is unreachable from the browser, or nothing.
 *
 * The two reasons read differently to whoever has to fix them: a relative path
 * that climbs out of the served root is a module in the wrong tree, and a bare
 * name with no import map entry is a dependency the pages never declared.
 */
function unreachableReason(file, specifier, mapped) {
  if (!specifier.startsWith(".")) {
    return mapped.has(specifier) ? undefined : "no import map entry";
  }
  const root = WEB.endsWith(sep) ? WEB : `${WEB}${sep}`;
  return resolve(dirname(file), specifier).startsWith(root) ? undefined : "outside the served root";
}

async function unreachable() {
  const mapped = await mappedSpecifiers();
  const escaped = [];
  for (const file of await scriptsUnder(WEB)) {
    for (const specifier of specifiersIn(await readFile(file, "utf8"))) {
      const reason = unreachableReason(file, specifier, mapped);
      if (reason) {
        escaped.push(`${relative(WEB, file)} → ${specifier} (${reason})`);
      }
    }
  }
  return escaped;
}

/**
 * The rule, stated as the empty list it has to produce.
 *
 * Reported as names rather than a count, because "one module escapes" is not
 * something a reader can act on and "editor/editor.mjs → ../../lib/edit.mjs" is
 * the whole diagnosis.
 */
test("no module under web/ imports anything the browser cannot fetch", async () => {
  assert.deepStrictEqual(await unreachable(), []);
});

/**
 * The verdict itself, on the exact import that broke the editor and on the
 * neighbours it must not be confused with. A guard that cannot fail is a guard
 * that has stopped being one, and an empty list from a tree walk looks the same
 * whether the rule held or nothing was read.
 */
test("the served-root rule names an escape, a stray bare specifier, and neither of the reachable ones", () => {
  const editor = join(WEB, "editor", "editor.mjs");
  const mapped = new Set(["react"]);
  const cases = [
    ["../../lib/edit.mjs", "outside the served root"],
    ["../step-refs.mjs", undefined],
    ["./relation.mjs", undefined],
    ["react", undefined],
    ["nowhere", "no import map entry"],
  ];

  assert.deepStrictEqual(
    cases.map(([specifier]) => unreachableReason(editor, specifier, mapped)),
    cases.map(([, expected]) => expected),
  );
});

/**
 * The scanner reads imports and not the prose around them. These modules
 * explain themselves at length, quoting module names and using the word
 * `from`, and a URL in a real string is not a comment.
 */
test("the scanner reads imports and not the commentary around them", () => {
  const source = [
    "// A comment that mentions importing from \"../../lib/edit.mjs\" is not an import.",
    'import { a } from "./real.mjs";',
    'import "./local.mjs";',
    'const b = await import("nowhere");',
    'const url = "https://example.invalid/not-a-comment";',
  ].join("\n");

  assert.deepStrictEqual(specifiersIn(source), ["./local.mjs", "./real.mjs", "nowhere"]);
});

/** Every page that mounts a module declares the vendored specifiers it uses. */
test("the pages map every bare specifier the modules import", async () => {
  const mapped = await mappedSpecifiers();

  assert.deepStrictEqual(
    ["react", "react-dom/client", "@xyflow/react"].filter((name) => !mapped.has(name)),
    [],
  );
});
