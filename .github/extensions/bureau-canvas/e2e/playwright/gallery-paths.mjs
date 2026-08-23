// The two directories the render gallery uses.
//
// `STAGING` is where a run's shots are written and `GALLERY` is what a reviewer
// browses; `global-teardown.mjs` moves the first onto the second, but only for
// a run that rendered. Both paths are fixed rather than passed around, so a
// worker process needs nothing from the setup process to find them.

import { fileURLToPath } from "node:url";

export const GALLERY = fileURLToPath(new URL("../gallery/", import.meta.url));
export const STAGING = fileURLToPath(new URL("../.gallery-staging/", import.meta.url));
