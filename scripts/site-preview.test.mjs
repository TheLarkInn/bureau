import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { request } from "node:http";
import { join } from "node:path";
import test from "node:test";
import { build } from "../site/build.mjs";
import { siteDirectory } from "../site/paths.mjs";
import { temporaryPreview } from "../site/preview.mjs";
import { serve } from "../site/serve.mjs";

test("preview serves only the built base path with a correct 404 and redirect", async (t) => {
  for (const base of ["/", "/bureau/"]) {
    const preview = await temporaryPreview(base);
    t.after(() => preview.close());
    assert.equal((await fetch(preview.url)).status, 200);
    const asset = await fetch(`${preview.url}assets/demo.mjs`);
    assert.equal(asset.status, 200);
    assert.match(asset.headers.get("content-type"), /javascript/u);
    const missing = await fetch(`${preview.url}not-found`);
    assert.equal(missing.status, 404);
    assert.match(await missing.text(), /Page not found/u);
    if (base !== "/") {
      assert.equal((await fetch(new URL("/", preview.url))).status, 404);
      const redirect = await fetch(preview.url.slice(0, -1), { redirect: "manual" });
      assert.equal(redirect.status, 308);
      assert.equal(redirect.headers.get("location"), base);
    }
  }
});

test("preview has no mutation, credential, source-tree, or traversal route", async (t) => {
  const preview = await temporaryPreview("/bureau/");
  t.after(() => preview.close());
  for (const path of ["../AGENTS.md", "site-manifest.json", "settings.yaml", "state.db", "assets/../../AGENTS.md", "assets/%2e%2e%2fsettings.yaml"]) {
    assert.equal((await fetch(new URL(path, preview.url))).status, 404);
  }
  const write = await fetch(preview.url, { method: "POST", body: "{}" });
  assert.equal(write.status, 405);
  assert.equal(write.headers.get("allow"), "GET, HEAD");
  const head = await fetch(preview.url, { method: "HEAD" });
  assert.equal(head.status, 200);
  assert.equal(await head.text(), "");
  assert.equal(head.headers.get("x-content-type-options"), "nosniff");
});

test("preview rejects a non-loopback host header", async (t) => {
  const preview = await temporaryPreview("/");
  t.after(() => preview.close());
  const status = await new Promise((accept, reject) => {
    const call = request(preview.url, { headers: { host: "public.example" } }, (response) => {
      response.resume();
      accept(response.statusCode);
    });
    call.on("error", reject);
    call.end();
  });
  assert.equal(status, 403);
});

test("preview detects changed artifacts instead of serving unverified bytes", async (t) => {
  const cache = join(siteDirectory, ".cache");
  await mkdir(cache, { recursive: true });
  const directory = await mkdtemp(join(cache, "preview-test-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await build({ out: directory });
  await writeFile(join(directory, "index.html"), "changed");
  await assert.rejects(serve({ directory }), /Build changed after validation/u);
});
