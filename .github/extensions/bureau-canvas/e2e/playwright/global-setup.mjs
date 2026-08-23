// Opens the render gallery, before any worker starts.
//
// It creates the directory and records when this run began; the pruning of
// anything older happens in `global-teardown.mjs`, once the run has shown
// whether it rendered any state at all. See `gallery.mjs` for why the decision
// is made that way round.

import { fileURLToPath } from "node:url";

import { openGallery } from "./gallery.mjs";

const GALLERY = fileURLToPath(new URL("../gallery/", import.meta.url));

export default async function globalSetup() {
  process.env.BUREAU_GALLERY_RUN_AT = String(await openGallery(GALLERY));
}
