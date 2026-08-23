// Publishes the render gallery, after the last worker has finished.
//
// A run that rendered states replaces the gallery wholesale, so a render of a
// state the registry no longer holds cannot survive into it. A run that
// rendered nothing — `test:pr`, `test:visual` — leaves the published gallery
// exactly as it found it.

import { publishGallery } from "./gallery.mjs";
import { GALLERY, STAGING } from "./gallery-paths.mjs";

export default async function globalTeardown() {
  const published = await publishGallery(STAGING, GALLERY);
  if (published) {
    console.log(`gallery: published ${published} file(s) to ${GALLERY}`);
  }
}
