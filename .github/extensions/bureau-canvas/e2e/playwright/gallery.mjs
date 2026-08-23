// What the gallery keeps, decided by what the run actually rendered.
//
// The gallery is a browsable artefact, so a screenshot of a state the registry
// no longer holds is worse than no screenshot: it invites a reviewer to sign
// off on a surface that does not exist. But emptying the directory at the start
// of *every* Playwright invocation is the opposite failure — `test:visual` and
// `test:pr` filter every matrix render out, render nothing into the gallery,
// and used to delete it anyway, so browsing the gallery and then running a
// sibling suite left an empty directory and no error to explain it.
//
// Neither rule can be read off the command line: Playwright's `FullConfig`
// reports `grep` as `/.*/` no matter what `--grep` was passed, and guessing at
// `argv` would be a heuristic about flags rather than a fact about renders.
//
// So the decision is made from the one thing that is not a guess: whether this
// run wrote anything into the gallery. A run that wrote a shot owns the gallery
// and its leftovers are pruned; a run that wrote nothing has no claim on it and
// leaves it exactly as it found it.

import { mkdir, readdir, rm, stat } from "node:fs/promises";
import { join } from "node:path";

/** Every gallery entry with the time it was last written. */
async function entries(dir) {
  const names = await readdir(dir).catch(() => []);
  return Promise.all(names.map(async (name) => {
    const path = join(dir, name);
    return { path, at: (await stat(path)).mtimeMs };
  }));
}

/** Marks the start of a run. Never deletes: nothing has been rendered yet. */
export async function openGallery(dir, now = Date.now()) {
  await mkdir(dir, { recursive: true });
  return now;
}

/**
 * Removes what this run did not write, if this run wrote anything at all.
 *
 * Returns the paths pruned, so the caller can say what it removed rather than
 * deleting silently.
 */
export async function pruneGallery(dir, startedAt) {
  const found = await entries(dir);
  if (!found.some((entry) => entry.at >= startedAt)) {
    return [];
  }
  const stale = found.filter((entry) => entry.at < startedAt);
  await Promise.all(stale.map((entry) => rm(entry.path, { recursive: true, force: true })));
  return stale.map((entry) => entry.path);
}
