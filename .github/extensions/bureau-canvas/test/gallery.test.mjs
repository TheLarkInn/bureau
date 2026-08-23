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
import { stagingFor, staging, STAGING_ENV } from "../e2e/playwright/gallery-paths.mjs";

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
  const { staging: stage, gallery } = await pair(t, ["desktop--old.png", "index.html"]);

  await openGallery(stage);
  const published = await publishGallery(stage, gallery);

  assert.deepEqual(
    [published, (await readdir(gallery)).sort(), await readdir(stage).catch(() => "gone")],
    [[], ["desktop--old.png", "index.html"], "gone"],
  );
});

test("a run that rendered replaces the gallery with exactly what it wrote", async (t) => {
  const { staging: stage, gallery } = await pair(t, ["desktop--dropped.png", "desktop--kept.png"]);

  await openGallery(stage);
  await writeFile(join(stage, "desktop--kept.png"), "fresh", "utf8");
  const published = await publishGallery(stage, gallery);

  assert.deepEqual([published, await readdir(gallery)], [["desktop--kept.png"], ["desktop--kept.png"]]);
});

test("opening staging discards whatever a crashed run left in it", async (t) => {
  const { staging: stage, gallery } = await pair(t, []);
  await mkdir(stage, { recursive: true });
  await writeFile(join(stage, "desktop--abandoned.png"), "stale", "utf8");

  await openGallery(stage);
  const published = await publishGallery(stage, gallery);

  assert.deepEqual([published, await readdir(gallery)], [[], []]);
});

/**
 * Two runs in one checkout must not share a staging directory: `openGallery`
 * empties what it is given, so a fixed path let a `test:visual` started beside
 * a running matrix delete the shots that run had already written, and the
 * matrix would publish the remainder and print a count for it.
 */
test("each run stages under its own process, and a worker without one refuses", () => {
  const before = process.env[STAGING_ENV];
  delete process.env[STAGING_ENV];
  const refused = (() => {
    try {
      staging();
      return null;
    } catch (error) {
      return error.message;
    }
  })();
  process.env[STAGING_ENV] = before ?? "";
  assert.deepEqual(
    [stagingFor(1) === stagingFor(2), refused?.includes(STAGING_ENV) === true],
    [false, true],
  );
});
