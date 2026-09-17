import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import test from "node:test";
import { build, maximumBuildBytes, publicFiles } from "../site/build.mjs";
import { scenarios } from "../site/content.mjs";
import { validateLinks } from "../site/links.mjs";
import { normalizeBase, normalizeOrigin, siteDirectory } from "../site/paths.mjs";
import { assetNames, targets } from "./release-assets.mjs";

async function output(t) {
  const cache = join(siteDirectory, ".cache");
  await mkdir(cache, { recursive: true });
  const directory = await mkdtemp(join(cache, "build-test-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

test("production and root builds resolve every local link and anchor", async (t) => {
  for (const base of ["/bureau/", "/", "/nested/preview/"]) {
    const { manifest, files } = await build({ base, out: await output(t) });
    assert.equal(manifest.base, base);
    assert.ok(manifest.links >= 45);
    assert.equal(validateLinks(files, base), manifest.links);
    assert.match(files.get("404.html").toString(), new RegExp(`href="${base}"`, "u"));
  }
});

test("build bytes are deterministic, bounded, and explicitly whitelisted", async (t) => {
  const directory = await output(t);
  const first = await build({ out: directory });
  const second = await build({ out: directory });
  assert.deepEqual(first.manifest, second.manifest);
  assert.deepEqual(Object.keys(first.manifest.files), publicFiles);
  assert.ok(first.manifest.bytes > 10_000 && first.manifest.bytes < maximumBuildBytes);
  assert.equal(publicFiles.some((name) => /CNAME|settings|credentials|state\.db|events\.jsonl/u.test(name)), false);
});

test("canonical URLs are opt-in; assets never depend on a hardcoded deployment host", async (t) => {
  for (const origin of [undefined, "https://thelarkinn.github.io", "https://example.test"]) {
    const { files } = await build({ base: "/bureau/", origin, out: await output(t) });
    const html = files.get("index.html").toString();
    assert.equal(html.includes('rel="canonical"'), Boolean(origin));
    if (origin) assert.ok(html.includes(`href="${origin}/bureau/"`));
    assert.ok(html.includes('src="/bureau/assets/demo.mjs"'));
    assert.equal(html.includes("{{"), false);
  }
});

test("base and canonical inputs reject traversal, credentials, and ambiguous URLs", () => {
  for (const base of ["bureau", "//host/", "/../", "/./", "/%2e/", "/with space/", "/bureau", "/a//b/", "\\bureau\\"]) {
    assert.throws(() => normalizeBase(base));
  }
  for (const origin of ["http://example.test", "https://user:password@example.test", "https://example.test/path", "https://example.test/?q=x", "https://example.test/#x"]) {
    assert.throws(() => normalizeOrigin(origin));
  }
});

test("build refuses rather than removes an unrelated output file", async (t) => {
  const directory = await output(t);
  await writeFile(join(directory, "keep.txt"), "unrelated");
  await assert.rejects(build({ out: directory }), /Unexpected output entry/u);
  assert.equal(await readFile(join(directory, "keep.txt"), "utf8"), "unrelated");
});

test("link checking rejects missing files, broken anchors, and base-path escapes", () => {
  for (const href of ["/assets/base.css", "/bureau/missing.css", "#missing", "https://example.test/private"]) {
    assert.throws(() => validateLinks(new Map([["index.html", Buffer.from(`<a href="${href}">Link</a>`)]]), "/bureau/"));
  }
  assert.throws(() => validateLinks(new Map([["index.html", Buffer.from('<p id="same"></p><p id="same"></p>')]]), "/"));
});

test("public JavaScript never reads private state, credentials, or an API", async (t) => {
  const { files } = await build({ out: await output(t) });
  assert.doesNotMatch(files.get("assets/demo.mjs").toString(),
    /\b(?:fetch|WebSocket|EventSource|XMLHttpRequest|sendBeacon|localStorage|sessionStorage|cookie|credentials)\b/u);
  assert.doesNotMatch(files.get("index.html").toString(), /<(?:form|input|iframe)\b/u);
  assert.match(files.get("index.html").toString(), /connect-src 'none'/u);
  assert.match(files.get("index.html").toString(), /data-enhance hidden/u);
});

test("catalog has ten distinct agreed source anchors and explicit limited-availability entries", () => {
  assert.deepEqual(scenarios.map(({ id }) => id), [
    "design-review", "issue-intake", "issue-triage", "customer-feedback",
    "failing-test-repair", "multi-repo-fix", "azure-devops",
    "local-sdk-factory", "cloud-automation", "recurring-maintenance",
  ]);
  assert.equal(new Set(scenarios.map(({ id }) => id)).size, 10);
  assert.match(scenarios.find(({ id }) => id === "local-sdk-factory").readiness, /Qualification/u);
  assert.match(scenarios.find(({ id }) => id === "cloud-automation").readiness, /Experimental.*eligibility/u);
});

test("download guidance matches actual release packaging for both Linux architectures", async (t) => {
  const { files } = await build({ out: await output(t) });
  const html = files.get("index.html").toString();
  const script = files.get("assets/demo.mjs").toString();
  assert.equal(targets.length, 2);
  for (const target of targets) {
    assert.ok(assetNames("0.0.0").includes(`bureau-v0.0.0-${target}.tar.gz`));
    assert.ok(html.includes(`data-arch="${target.split("-")[0]}"`));
  }
  assert.ok(script.includes("bureau-v<version>-${button.dataset.arch}-unknown-linux-musl.tar.gz"));
  assert.match(html, /sha256sum --check &lt;archive&gt;\.sha256/u);
  assert.match(html, /bureau init --from init\.yaml/u);
});
