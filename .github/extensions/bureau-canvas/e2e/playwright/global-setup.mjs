// Opens this run's render staging directory, before any worker starts, and
// tells the workers where it is.
//
// Publishing it over the gallery happens in `global-teardown.mjs`, once the run
// has shown whether it rendered any state at all. See `gallery.mjs` for why the
// decision is made that way round, and `gallery-paths.mjs` for why staging is
// named after this process.

import { openGallery } from "./gallery.mjs";
import { stagingFor, STAGING_ENV } from "./gallery-paths.mjs";

export default async function globalSetup() {
  const staging = stagingFor(process.pid);
  await openGallery(staging);
  process.env[STAGING_ENV] = staging;
}
