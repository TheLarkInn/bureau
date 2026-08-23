// Where a run's renders go, and what happens to the previous ones.
//
// The gallery is a browsable artefact, so a screenshot of a state the registry
// no longer holds is worse than no screenshot: it invites a reviewer to sign
// off on a surface that does not exist. But emptying the directory at the start
// of *every* Playwright invocation is the opposite failure — `test:visual` and
// `test:pr` filter every matrix render out, render nothing into the gallery,
// and used to delete it anyway, so browsing the gallery and then running a
// sibling suite left an empty directory and no error to explain it.
//
// Neither rule can be read off the command line: Playwright reports
// `FullConfig.grep` as `/.*/` no matter what `--grep` was passed, and guessing
// at `argv` would be a heuristic about flags rather than a fact about renders.
//
// So a run renders into a staging directory and publishes it only if it put
// something there. Whether a run rendered is then a fact — a directory is empty
// or it is not — rather than an inference from clocks. That distinction is not
// academic: comparing file times against the run's start time is wrong on any
// filesystem that stores mtime more coarsely than `Date.now()` reports it,
// where a shot written after the run began reads as older than it. CI is such
// a filesystem, and said so.
//
// Publishing is a rename, so the gallery is never a mixture of two runs: a
// reviewer sees the previous run's or this one's, and a crash between the
// removal and the rename leaves an empty directory, which is visibly nothing
// rather than quietly half a matrix.

import { mkdir, readdir, rename, rm } from "node:fs/promises";

/** Empties the staging directory. Nothing already published is touched. */
export async function openGallery(staging) {
  await rm(staging, { recursive: true, force: true });
  await mkdir(staging, { recursive: true });
}

/**
 * Replaces `gallery` with `staging` when this run rendered anything, and
 * discards `staging` when it did not.
 *
 * Returns how many renders were published, so the caller can say what it did
 * rather than replacing a reviewer's gallery in silence.
 */
export async function publishGallery(staging, gallery) {
  const rendered = await readdir(staging).catch(() => []);
  if (!rendered.length) {
    await rm(staging, { recursive: true, force: true });
    return 0;
  }
  await rm(gallery, { recursive: true, force: true });
  await rename(staging, gallery);
  return rendered.length;
}
