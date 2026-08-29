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
// relative specifier under `web/` resolves to a file that exists inside `web/`,
// and every bare specifier is one the pages actually declare an import map
// entry for. `vendor/` is excluded — those are third-party bundles, shipped as
// fetched.
//
// "Exists", and not merely "stays under the root", because a typo is the same
// 404 as a module in the wrong tree and reaches the reader the same way. And
// the pages' own inline `<script type="module">` blocks are read alongside the
// `.mjs` files, because that is where both surfaces start: `index.html` reaches
// `app.mjs` and `editor.html` reaches `editor/index.mjs` through an
// `await import(…)`, so the two specifiers whose failure this rule exists to
// prevent were the two it did not look at.
//
// The host side needs no such rule: it reads files off disk, which is why the
// one rule both trees share lives on the reachable side, in `web/step-refs.mjs`,
// and `lib/edit.mjs` imports it rather than the other way round.

import assert from "node:assert/strict";
import { access, readdir, readFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const WEB = fileURLToPath(new URL("../web/", import.meta.url));

/**
 * Every specifier a module names, however it names it.
 *
 * Backticks included, and not for completeness: a no-substitution template is a
 * legal specifier for `import()`, so ``import(`../../lib/edit.mjs`)`` reaches a
 * module outside this tree — the one thing this file exists to forbid — and read
 * only for quotes the rule saw nothing at all. A comment claiming "however it
 * names it" over three patterns that cover two of the three ways to write a
 * string is the shape of check this suite keeps finding: exhaustive in its
 * wording and partial in its reach.
 *
 * An *interpolated* template is caught by the same patterns and deliberately not
 * resolved. `${…}` means the specifier is not knowable from the source, so no
 * rule reading source can say whether it stays in this tree; it is reported as a
 * finding of its own rather than silently skipped, which is how the quoted
 * patterns would have treated it.
 */
const SPECIFIERS = [
  /\bfrom\s*["'`]([^"'`\n]+)["'`]/gu,
  /\bimport\s*\(\s*["'`]([^"'`\n]+)["'`]/gu,
  /\bimport\s*["'`]([^"'`\n]+)["'`]/gu,
];

/**
 * A dynamic import whose argument is not a literal at all.
 *
 * Each pattern above requires the argument to *open* with a quote or a
 * backtick, so `import(target)` — an identifier, a call, a ternary — matched
 * nothing, and a specifier this scanner never captures is one no later rule is
 * ever asked about. That is the backtick hole one step further out, and the
 * more useful one to a module reaching past this boundary, because it needs no
 * unusual syntax: two ordinary lines put `../../lib/edit.mjs` outside
 * everything here enforces.
 *
 * Reported rather than resolved, exactly as an interpolated template is. What
 * the argument evaluates to is not knowable from source, so the honest verdict
 * is that no source-reading rule can say where it points — and the alternative
 * to saying that is a silence indistinguishable from approval.
 */
const COMPUTED_IMPORT = /\bimport\s*\(\s*(?!["'`])([^)]+)\)/gu;

/** Whether a specifier read from source can be followed at all. */
function unknowable(specifier) {
  return specifier.includes("${") || specifier.startsWith("import(");
}

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

/**
 * The module scripts the pages carry inline, which are where this tree's two
 * surfaces actually start.
 *
 * The walk above reads `.mjs` files, and both entry points are not in one:
 * `index.html` reaches `app.mjs` and `editor.html` reaches `editor/index.mjs`
 * through an `await import(…)` inside a `<script type="module">`. So the two
 * specifiers whose failure is the whole reason this file exists — the surface
 * never mounts, and every browser spec times out on an element that was never
 * coming — were the only two the rule did not read.
 *
 * `src` counts as a specifier too: a module script fetched by attribute is
 * unreachable in exactly the same way, and `statelab.html` mounts the lab that
 * way.
 */
const INLINE_MODULE = /<script\b(?<attributes>[^>]*\btype="module"[^>]*)>(?<body>[\s\S]*?)<\/script>/gu;
const SRC_ATTRIBUTE = /\bsrc="([^"]+)"/u;

async function pagesUnder(directory) {
  return (await readdir(directory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".html"))
    .map((entry) => join(directory, entry.name));
}

function inlineSpecifiers(source) {
  const found = [];
  for (const { groups } of source.matchAll(INLINE_MODULE)) {
    found.push(...specifiersIn(groups.body), ...(groups.attributes.match(SRC_ATTRIBUTE)?.slice(1) ?? []));
  }
  return found;
}

function specifiersIn(source) {
  const found = new Set();
  const stripped = withoutComments(source);
  for (const pattern of SPECIFIERS) {
    for (const [, specifier] of stripped.matchAll(pattern)) {
      found.add(specifier);
    }
  }
  for (const [, argument] of stripped.matchAll(COMPUTED_IMPORT)) {
    found.add(`import(${argument.trim()})`);
  }
  return [...found].sort();
}

/**
 * Each page's own import map, which is the only one its modules may draw on.
 *
 * Unioning every page's map and checking every module against the union was the
 * weaker rule it looks like: a dependency mapped in `editor.html` answered for a
 * module only `index.html` loads, so dropping `react` from one page's map left
 * this rule green and that page 404-ing on the very import whose failure means
 * nothing mounts at all. A map belongs to a page, so the question does too.
 */
async function pageMaps() {
  const maps = new Map();
  for (const page of await pagesUnder(WEB)) {
    const source = await readFile(page, "utf8");
    const map = source.match(/<script type="importmap">([\s\S]*?)<\/script>/u);
    maps.set(page, new Set(Object.keys(JSON.parse(map?.[1] ?? "{}").imports ?? {})));
  }
  return maps;
}

/** The bare specifiers some page maps, for modules no page reaches. */
async function mappedSpecifiers() {
  return new Set([...await pageMaps()].flatMap(([, mapped]) => [...mapped]));
}

/**
 * Every module a page can actually reach, and the specifiers each one names.
 *
 * Walked from the page's own inline scripts through its relative imports, so a
 * module is judged against the map of the page that loads it. A specifier that
 * names nothing is simply not followed — `missingReason` is what reports that,
 * and reporting it twice in two voices helps nobody.
 */
async function graphOf(page) {
  const modules = new Map();
  const queued = new Set([page]);
  const pending = [[page, inlineSpecifiers(await readFile(page, "utf8"))]];
  while (pending.length) {
    const [file, specifiers] = pending.pop();
    modules.set(file, specifiers);
    for (const specifier of specifiers.filter((name) => name.startsWith("."))) {
      const target = resolve(dirname(file), specifier);
      if (!queued.has(target) && await readable(target)) {
        queued.add(target);
        pending.push([target, specifiersIn(await readFile(target, "utf8"))]);
      }
    }
  }
  return modules;
}

async function readable(path) {
  return access(path).then(() => true, () => false);
}

/**
 * Bare specifiers no page that loads them has an import map entry for.
 *
 * Reported with the page as well as the module, because the same module can be
 * reachable from two pages and the fix — which map gains the entry — depends on
 * which of them is missing it.
 */
async function unmapped() {
  const escaped = [];
  for (const [page, mapped] of await pageMaps()) {
    for (const [file, specifiers] of await graphOf(page)) {
      const strays = specifiers.filter((name) => !name.startsWith(".") && !mapped.has(name));
      escaped.push(...strays.map((name) => `${relative(WEB, page)} → ${relative(WEB, file)} → ${name} (no import map entry)`));
    }
  }
  return escaped.sort();
}

/**
 * Why a module's specifier is unreachable from the browser, or nothing.
 *
 * The two reasons read differently to whoever has to fix them: a relative path
 * that climbs out of the served root is a module in the wrong tree, and a bare
 * name with no import map entry is a dependency the pages never declared. A
 * third reads differently again: a specifier assembled at runtime is one this
 * rule cannot judge either way, which is a finding rather than a pass, because
 * the alternative is a module in this tree reaching wherever it likes behind a
 * `${…}` no source-reading rule can follow.
 */
function unreachableReason(file, specifier, mapped) {
  if (unknowable(specifier)) {
    return "built at runtime, so no rule reading source can say where it points";
  }
  if (!specifier.startsWith(".")) {
    return mapped.has(specifier) ? undefined : "no import map entry";
  }
  const root = WEB.endsWith(sep) ? WEB : `${WEB}${sep}`;
  return resolve(dirname(file), specifier).startsWith(root) ? undefined : "outside the served root";
}

async function unreachable() {
  const mapped = await mappedSpecifiers();
  const escaped = [];
  for (const [file, specifiers] of await sources()) {
    for (const specifier of specifiers) {
      const reason = unreachableReason(file, specifier, mapped) ?? await missingReason(file, specifier);
      if (reason) {
        escaped.push(`${relative(WEB, file)} → ${specifier} (${reason})`);
      }
    }
  }
  return escaped;
}

/** Every file the browser parses as a module, and the specifiers it names. */
async function sources() {
  const found = [];
  for (const file of await scriptsUnder(WEB)) {
    found.push([file, specifiersIn(await readFile(file, "utf8"))]);
  }
  for (const page of await pagesUnder(WEB)) {
    found.push([page, inlineSpecifiers(await readFile(page, "utf8"))]);
  }
  return found;
}

/**
 * The other way a relative specifier is a 404: it stays inside the served root
 * and names nothing.
 *
 * Staying under `web/` was the whole of the rule, which catches a module in the
 * wrong tree and misses a typo — and both arrive at the reader identically,
 * as a surface that never mounts. `./relaton.mjs` resolves neatly under the
 * root, and the only thing that would have noticed is a five-minute Playwright
 * run reporting that every spec timed out.
 *
 * Only for relative specifiers: a bare one is answered by the import map, and
 * its target is a vendored file this rule deliberately does not walk.
 */
async function missingReason(file, specifier) {
  if (!specifier.startsWith(".")) {
    return undefined;
  }
  return access(resolve(dirname(file), specifier)).then(() => undefined, () => "no such file under web/");
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

/**
 * And it reads the third way to write a string.
 *
 * The patterns took `"` and `'` under a comment that said "however it names
 * it". A no-substitution template is a legal specifier for `import()`, so
 * ``import(`../../lib/edit.mjs`)`` reached out of this tree past a rule whose
 * only job is to stop exactly that — and it did so silently, because a
 * specifier the scanner never captures is one no later rule is ever asked
 * about.
 *
 * An interpolated specifier is captured too and answered differently on
 * purpose. `${…}` means the target is not knowable from source at all, so
 * "stays under the served root" has no answer rather than a good one; reported
 * as its own reason, it is a finding instead of a gap.
 */
test("the scanner reads a template specifier, and refuses to guess an interpolated one", () => {
  const editor = join(WEB, "editor", "editor.mjs");
  const source = [
    "const a = await import(`../../lib/edit.mjs`);",
    "const b = await import(`./built-${kind}.mjs`);",
    "import { c } from `./templated.mjs`;",
  ].join("\n");
  const found = specifiersIn(source);

  assert.deepStrictEqual(
    [found, found.map((specifier) => unreachableReason(editor, specifier, new Set()))],
    [
      ["../../lib/edit.mjs", "./built-${kind}.mjs", "./templated.mjs"],
      ["outside the served root", "built at runtime, so no rule reading source can say where it points", undefined],
    ],
  );
});

/**
 * And a specifier that is no literal at all.
 *
 * The three patterns each require the argument to *open* with a quote or a
 * backtick, so `import(target)` matched nothing and reached the served-root
 * rule as silence. That silence is indistinguishable from approval, and it is
 * the cheapest way past this boundary of the three: `const target = "…"` and
 * `await import(target)` are two ordinary lines needing no unusual syntax.
 *
 * Answered the way an interpolated template is, and for the same reason — an
 * argument computed at runtime has no source-readable target — with the call
 * itself carried as the finding so a reader is told which call to go and look
 * at, not merely that one exists.
 */
test("the scanner reports a dynamic import whose target is computed", () => {
  const editor = join(WEB, "editor", "editor.mjs");
  const source = [
    "const target = \"../../lib/edit.mjs\";",
    "const a = await import(target);",
    "const b = await import(pick ? \"./one.mjs\" : \"./two.mjs\");",
    "const c = await import(\"./plain.mjs\");",
  ].join("\n");
  const found = specifiersIn(source);

  assert.deepStrictEqual(
    [found, found.map((specifier) => unreachableReason(editor, specifier, new Set()))],
    [
      ["./plain.mjs", "import(pick ? \"./one.mjs\" : \"./two.mjs\")", "import(target)"],
      [
        undefined,
        "built at runtime, so no rule reading source can say where it points",
        "built at runtime, so no rule reading source can say where it points",
      ],
    ],
  );
});

/** Every page that mounts a module declares the vendored specifiers it uses. */
test("the pages map every bare specifier the modules import", async () => {
  const mapped = await mappedSpecifiers();

  assert.deepStrictEqual(
    ["react", "react-dom/client", "@xyflow/react"].filter((name) => !mapped.has(name)),
    [],
  );
});

/**
 * The same rule asked of one page at a time, which is how the browser asks it.
 *
 * An import map belongs to the page that carries it. Unioning all three and
 * checking every module against the union answered for a page with what a
 * different page had declared, so the rule was strongest exactly where it
 * needed no strength and silent where a page maps nothing at all.
 */
test("every page maps the bare specifiers the modules it loads name", async () => {
  assert.deepStrictEqual(await unmapped(), []);
});

/**
 * The union really was a weaker question, and this is the pair that proves it.
 *
 * `statelab.html` carries no import map: it mounts the lab, which drives the
 * product pages inside a frame rather than importing their dependencies. So a
 * bare specifier reachable from the lab was answered by `index.html`'s map and
 * would have 404'd at runtime. A rule that cannot tell these two maps apart is
 * not asking about a page.
 */
test("a page is judged by its own import map and not by another page's", async () => {
  const maps = await pageMaps();

  assert.deepStrictEqual(
    [maps.get(join(WEB, "statelab.html")).has("react"), maps.get(join(WEB, "index.html")).has("react")],
    [false, true],
  );
});

/**
 * The per-page rule is only worth anything if the walk actually reaches past
 * the page's own inline script. `index.html` names `app.mjs` and nothing else;
 * every dependency it has to answer for is somewhere below that.
 */
test("a page's graph is walked through its relative imports, not just its inline scripts", async () => {
  const held = new Set([...await graphOf(join(WEB, "index.html"))].map(([file]) => relative(WEB, file)));

  assert.deepStrictEqual(
    ["index.html", "app.mjs", "graph-edges.mjs"].filter((name) => !held.has(name)),
    [],
  );
});

/**
 * The two specifiers this whole rule is about live in HTML, not in a module.
 *
 * `index.html` reaches `app.mjs` and `editor.html` reaches `editor/index.mjs`
 * through an `await import(…)` inside a `<script type="module">`, and
 * `statelab.html` mounts the lab through a `src`. Those are the entry points:
 * if one of them is wrong, nothing mounts at all. The file walk reads `.mjs`
 * and so read none of them, which left the rule strongest everywhere except
 * the three places where it decides whether a page exists.
 */
test("the scanner reads the module scripts the pages carry inline", () => {
  const page = [
    '<script type="importmap">{"imports":{"react":"./vendor/react.mjs"}}</script>',
    '<script type="module" src="./statelab/lab.mjs"></script>',
    '<script type="module">',
    '  const boot = async () => { await import("./app.mjs"); };',
    "</script>",
    '<script>const legacy = "./not-a-module.mjs";</script>',
  ].join("\n");

  assert.deepStrictEqual(inlineSpecifiers(page).sort(), ["./app.mjs", "./statelab/lab.mjs"]);
});

/**
 * A typo is the same 404 as a module in the wrong tree.
 *
 * `./relaton.mjs` resolves under the served root and names nothing, so the
 * root rule passed it. The reader gets the identical failure either way — the
 * import throws, the surface never mounts — so the guard owes both answers.
 */
test("a relative specifier that names nothing is reported as the 404 it is", async () => {
  const editor = join(WEB, "editor", "editor.mjs");
  const cases = [
    ["./relation.mjs", undefined],
    ["./relaton.mjs", "no such file under web/"],
    ["../step-refs.mjs", undefined],
    ["react", undefined],
  ];

  assert.deepStrictEqual(
    await Promise.all(cases.map(([specifier]) => missingReason(editor, specifier))),
    cases.map(([, expected]) => expected),
  );
});
