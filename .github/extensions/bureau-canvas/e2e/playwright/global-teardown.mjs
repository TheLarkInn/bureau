// Publishes the render gallery, after the last worker has finished.
//
// A run that rendered states replaces the gallery wholesale, so a render of a
// state the registry no longer holds cannot survive into it. A run that
// rendered nothing — `test:pr`, `test:visual` — leaves the published gallery
// exactly as it found it.

import { join } from "node:path";

import { publishGallery } from "./gallery.mjs";
import { GALLERY, staging } from "./gallery-paths.mjs";

export default async function globalTeardown() {
  const published = await publishGallery(staging(), GALLERY);
  if (!published.length) {
    return;
  }
  console.log(`gallery: published ${published.length} file(s) to ${GALLERY}`);
  // A full matrix run always writes the index, including a failing one — it is
  // an ordinary test and nothing short-circuits the run. A narrower `--grep`
  // can leave it out, and the renders are still worth keeping, so this says so
  // rather than withholding them.
  if (!published.includes("index.html")) {
    console.log(`gallery: this run rendered no index; browse the files directly under ${join(GALLERY)}`);
  }
}
