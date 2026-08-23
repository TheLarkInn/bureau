// The gallery is pruned by a run that rendered it, and only by such a run.
//
// This is the guard on a silent artefact loss in both directions: a run that
// renders nothing (`test:pr`, `test:visual`) must leave a reviewer's gallery
// alone, and a run that does render must not leave behind the shot of a state
// the registry has since dropped.

import assert from "node:assert/strict";
import { mkdtemp, readdir, rm, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { openGallery, pruneGallery } from "../e2e/playwright/gallery.mjs";

async function galleryWith(names, at) {
  const dir = await mkdtemp(join(tmpdir(), "bureau-gallery-"));
  for (const name of names) {
    const path = join(dir, name);
    await writeFile(path, name, "utf8");
    await utimes(path, new Date(at), new Date(at));
  }
  return dir;
}

const PREVIOUS = Date.now() - 60_000;

test("a run that rendered nothing leaves every earlier render in place", async (t) => {
  const dir = await galleryWith(["desktop--old.png", "index.html"], PREVIOUS);
  t.after(() => rm(dir, { recursive: true, force: true }));

  const startedAt = await openGallery(dir);
  const pruned = await pruneGallery(dir, startedAt);

  assert.deepEqual([pruned, (await readdir(dir)).sort()], [[], ["desktop--old.png", "index.html"]]);
});

test("a run that rendered prunes the states it did not render", async (t) => {
  const dir = await galleryWith(["desktop--dropped.png", "desktop--kept.png"], PREVIOUS);
  t.after(() => rm(dir, { recursive: true, force: true }));

  const startedAt = await openGallery(dir);
  await writeFile(join(dir, "desktop--kept.png"), "fresh", "utf8");
  const pruned = await pruneGallery(dir, startedAt);

  assert.deepEqual([pruned.map((path) => path.endsWith("desktop--dropped.png")), (await readdir(dir)).sort()], [[true], ["desktop--kept.png"]]);
});

test("opening an absent gallery creates it and prunes nothing", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "bureau-gallery-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const dir = join(parent, "gallery");

  const startedAt = await openGallery(dir);
  const pruned = await pruneGallery(dir, startedAt);

  assert.deepEqual([pruned, await readdir(dir)], [[], []]);
});
