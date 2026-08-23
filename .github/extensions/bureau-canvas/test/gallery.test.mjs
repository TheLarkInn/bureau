// The gallery is replaced by the run that filled it, and by no other.
//
// This is the guard on a silent artefact loss in both directions: a run that
// renders nothing (`test:pr`, `test:visual`) must leave a reviewer's gallery
// alone, and a run that does render must not leave behind the shot of a state
// the registry has since dropped.
//
// Deliberately no clock anywhere. An earlier version decided this by comparing
// file times against the run's start, and CI — whose filesystem records mtime
// more coarsely than `Date.now()` reports it — recorded a shot written *after*
// the run began as older than it.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { openGallery, publishGallery } from "../e2e/playwright/gallery.mjs";

/** A staging and a gallery directory, with the gallery already populated. */
async function pair(t, published) {
  const root = await mkdtemp(join(tmpdir(), "bureau-gallery-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const gallery = join(root, "gallery");
  await mkdir(gallery, { recursive: true });
  for (const name of published) {
    await writeFile(join(gallery, name), name, "utf8");
  }
  return { staging: join(root, "staging"), gallery };
}

test("a run that rendered nothing leaves the published gallery in place", async (t) => {
  const { staging, gallery } = await pair(t, ["desktop--old.png", "index.html"]);

  await openGallery(staging);
  const published = await publishGallery(staging, gallery);

  assert.deepEqual(
    [published, (await readdir(gallery)).sort(), await readdir(staging).catch(() => "gone")],
    [0, ["desktop--old.png", "index.html"], "gone"],
  );
});

test("a run that rendered replaces the gallery with exactly what it wrote", async (t) => {
  const { staging, gallery } = await pair(t, ["desktop--dropped.png", "desktop--kept.png"]);

  await openGallery(staging);
  await writeFile(join(staging, "desktop--kept.png"), "fresh", "utf8");
  const published = await publishGallery(staging, gallery);

  assert.deepEqual([published, await readdir(gallery)], [1, ["desktop--kept.png"]]);
});

test("opening staging discards whatever a crashed run left in it", async (t) => {
  const { staging, gallery } = await pair(t, []);
  await mkdir(staging, { recursive: true });
  await writeFile(join(staging, "desktop--abandoned.png"), "stale", "utf8");

  await openGallery(staging);
  const published = await publishGallery(staging, gallery);

  assert.deepEqual([published, await readdir(gallery)], [0, []]);
});
