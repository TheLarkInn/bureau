// Prunes the render gallery, after the last worker has finished.
//
// A run that rendered states owns the gallery, so anything it did not write is
// a state the registry no longer holds and is removed. A run that rendered
// nothing — `test:pr`, `test:visual` — leaves the gallery untouched.

import { fileURLToPath } from "node:url";

import { pruneGallery } from "./gallery.mjs";

const GALLERY = fileURLToPath(new URL("../gallery/", import.meta.url));

export default async function globalTeardown() {
  const startedAt = Number(process.env.BUREAU_GALLERY_RUN_AT);
  if (!Number.isFinite(startedAt)) {
    return;
  }
  const pruned = await pruneGallery(GALLERY, startedAt);
  if (pruned.length) {
    console.log(`gallery: removed ${pruned.length} render(s) of states the registry no longer holds`);
  }
}
