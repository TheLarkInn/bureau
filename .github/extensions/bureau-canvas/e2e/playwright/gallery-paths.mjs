// The two directories the render gallery uses.
//
// `GALLERY` is what a reviewer browses and is fixed. Staging — where a run's
// shots are written before `global-teardown.mjs` publishes them — is *not*
// fixed: it carries the setup process's id, so two Playwright invocations in
// one checkout cannot delete each other's renders. A fixed staging path made
// `test:visual` started beside a running matrix wipe the shots that run had
// already written, and the matrix would then publish whatever survived and
// report a number for it.
//
// The path is passed to the workers through the environment, which is how
// `globalSetup` hands anything to a worker. A worker that cannot see it throws
// rather than guessing at a default: writing 332 renders somewhere nothing will
// publish is the silent version of the same failure.

import { fileURLToPath } from "node:url";

export const GALLERY = fileURLToPath(new URL("../gallery/", import.meta.url));
export const STAGING_ENV = "BUREAU_GALLERY_STAGING";

/** The staging directory for this run, named by the process that opened it. */
export function stagingFor(pid) {
  return fileURLToPath(new URL(`../.gallery-staging-${pid}/`, import.meta.url));
}

/** This run's staging directory, as published by `global-setup.mjs`. */
export function staging() {
  const path = process.env[STAGING_ENV];
  if (!path) {
    throw new Error(`${STAGING_ENV} is unset: the render gallery is opened by global-setup.mjs, which did not run`);
  }
  return path;
}
