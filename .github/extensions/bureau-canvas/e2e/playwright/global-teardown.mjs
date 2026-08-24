// Publishes the render gallery, after the last worker has finished, and makes
// it say what it actually holds.
//
// A run that rendered states replaces the gallery wholesale, so a render of a
// state the registry no longer holds cannot survive into it. A run that
// rendered nothing — `test:pr`, `test:visual` — leaves the published gallery
// exactly as it found it.
//
// The audit is here rather than in a spec because this is the only place with
// the whole picture: the renders are written by several workers at once, and no
// test can see another worker's files. `@matrix gallery index` therefore cannot
// assert that the figures it links exist — it runs while other workers are
// still rendering, and says so — and that is exactly the gap this closes.

import { readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { RENDER_TWINS, STATES } from "../../web/statelab/registry.mjs";
import { VIEWPORTS } from "../../web/statelab/selectors.mjs";
import { auditNames, auditTwins, expectedShots } from "./gallery-audit.mjs";
import { publishGallery } from "./gallery.mjs";
import { GALLERY, staging } from "./gallery-paths.mjs";

const SIGNATURES = "signatures";

export default async function globalTeardown() {
  const signatures = await collapseSignatures(staging());
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
    console.log(`gallery: this run rendered no index; browse the files directly under ${GALLERY}`);
    return;
  }
  await report(published, signatures);
}

/**
 * Folds the per-render signature files into one `signatures.json` inside the
 * staging directory, so the published gallery carries a diffable record of what
 * each state actually drew rather than only a picture of it.
 */
async function collapseSignatures(dir) {
  const names = await readdir(join(dir, SIGNATURES)).catch(() => []);
  const signatures = {};
  for (const name of names) {
    signatures[name.replace(/\.sha$/u, "")] = await readFile(join(dir, SIGNATURES, name), "utf8");
  }
  await rm(join(dir, SIGNATURES), { recursive: true, force: true });
  if (names.length) {
    await writeFile(join(dir, "signatures.json"), `${JSON.stringify(signatures, null, 1)}\n`, "utf8");
  }
  return signatures;
}

/**
 * States the gallery links but does not hold, states it holds that the registry
 * does not, and states that draw one another's screen without saying so.
 *
 * Reported into the artefact rather than thrown, and that is a measured
 * decision rather than a soft one. The signature still drifts on about an
 * eighth of the renders between two runs of one tree — some content arrives
 * after the surface has stopped changing for a poll interval, so a render is
 * occasionally captured a beat early — and a gate on a drifting signal fails
 * runs at random. This repository's own rule is that a flaky gate is worse than
 * no gate, so the audit publishes what it found where the reviewer is already
 * looking, and the drift is filed rather than papered over.
 *
 * What it publishes is still a contradiction the registry could not previously
 * face: `signatures.json` is a diffable record of what each state actually
 * drew, the index says out loud when it is not the whole matrix, and every pair
 * of states drawing one screen is either declared in `RENDER_TWINS` with a
 * reason or named here as news.
 */
async function report(published, signatures) {
  const names = auditNames(expectedShots(STATES, Object.values(VIEWPORTS)), published);
  const twins = auditTwins(signatures, RENDER_TWINS);
  const lines = [
    ...(names.missing.length ? [`${names.missing.length} render(s) were never written by this run`] : []),
    ...(names.stray.length ? [`${names.stray.length} render(s) belong to no state in the registry`] : []),
    ...twins.map((finding) => `${finding.kind}: ${finding.detail}`),
  ];
  console.log(`gallery: ${Object.keys(signatures).length} render(s) audited`);
  if (!lines.length) {
    console.log("gallery: complete, and every state draws its own screen or a declared twin's");
    return;
  }
  for (const line of lines) {
    console.log(`gallery: ${line}`);
  }
  await stamp(lines, names.missing);
}

/** Marks the findings in the artefact itself, not only on a console. */
async function stamp(lines, missing) {
  const banner = '<p style="margin:0;padding:.75rem 1.5rem;background:#ffebe9;color:#cf222e;font-weight:700">'
    + `This gallery is not the whole matrix, or not every state in it draws its own screen: ${lines.join("; ")}. `
    + `${missing.length ? `Missing: ${missing.slice(0, 20).join(", ")}${missing.length > 20 ? ", …" : ""}` : ""}</p>`;
  const index = join(GALLERY, "index.html");
  const written = await readFile(index, "utf8").catch(() => null);
  if (written) {
    await writeFile(index, written.replace("<main>", `${banner}<main>`), "utf8");
  }
}
