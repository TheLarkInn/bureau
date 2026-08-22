// Clears the render gallery once, before any worker starts.
//
// The gallery is a browsable artefact, so a screenshot of a state the registry
// no longer holds is worse than no screenshot: it invites a reviewer to sign
// off on a surface that does not exist. Emptying the directory once per run
// keeps the gallery exactly the current matrix and nothing else.

import { mkdir, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const GALLERY = fileURLToPath(new URL("../gallery/", import.meta.url));

export default async function globalSetup() {
  await rm(GALLERY, { recursive: true, force: true });
  await mkdir(GALLERY, { recursive: true });
}
