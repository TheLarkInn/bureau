// Opens the render staging directory, before any worker starts.
//
// Publishing it over the gallery happens in `global-teardown.mjs`, once the run
// has shown whether it rendered any state at all. See `gallery.mjs` for why the
// decision is made that way round.

import { openGallery } from "./gallery.mjs";
import { STAGING } from "./gallery-paths.mjs";

export default async function globalSetup() {
  await openGallery(STAGING);
}
