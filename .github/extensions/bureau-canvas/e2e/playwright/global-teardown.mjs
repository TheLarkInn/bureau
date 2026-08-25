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
//
// It is a gate: a finding here fails the run. `report` below records why that
// is now safe and what had to be fixed first.

import { readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { RENDER_TWINS, STATES } from "../../web/statelab/registry.mjs";
import { VIEWPORTS } from "../../web/statelab/selectors.mjs";
import { auditNames, auditTwins, expectedShots } from "./gallery-audit.mjs";
import { publishGallery } from "./gallery.mjs";
import { GALLERY, staging } from "./gallery-paths.mjs";

const SIGNATURES = "signatures";

export default async function globalTeardown(config) {
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
  await report(published, signatures, config);
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
 * This throws, and until this change it did not. The reason it did not was
 * measured and recorded: the signature drifted on about an eighth of the
 * renders between two runs of one tree, and a gate on a drifting signal fails
 * runs at random, which is worse than no gate at all.
 *
 * The drift is gone, and it was never the late content everyone took it for.
 * The config surface mounts its relation graph inside a `<details>` that is
 * shut by default, and a shut disclosure is a subtree the browser stops
 * rendering but keeps answering `getClientRects` for. So every collapsed config
 * state was signing a description of a graph no reader can see, and React
 * Flow's measurement race went on racing inside it — 58 of 502 renders differed
 * across two runs, 54 of them compact config states with the relation section
 * shut. `checks.mjs` now asks `checkVisibility()` before it describes an
 * element, the shut graph left the signature, and two runs of this matrix agree
 * on all 502.
 *
 * So the audit is a gate. `signatures.json` is still published beside the
 * renders as the diffable record of what each state drew, the index still says
 * out loud when it is not the whole matrix, and now a gallery that is not the
 * whole matrix, or in which two states draw one screen without declaring it,
 * fails the run that produced it.
 *
 * Except under `--shard`, where an incomplete gallery is the point. A shard
 * runs its fraction of the renders and one of them also runs the index test, so
 * gating there would fail every shard for hundreds of renders another shard
 * wrote, and would report every twin split across two shards as unchecked.
 * Nothing here can see the other shards, so a sharded run reports what it found
 * and leaves the verdict to whatever aggregates them. Sharding is not used by
 * this repository today; the guard is here so that turning it on is a change to
 * the workflow rather than a morning spent on a red gate nobody can reproduce.
 */
async function report(published, signatures, config) {
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
  // Stamped before the throw, not instead of it: the reviewer who opens the
  // artefact to find out what failed must see the finding on the page as well
  // as in the log.
  await stamp(lines, names.missing);
  if (config?.shard) {
    console.log(`gallery: reported rather than failed — this run is shard ${config.shard.current}/${config.shard.total} of the matrix`);
    return;
  }
  throw new Error(
    `the render gallery is not the whole matrix, or not every state in it draws its own screen:\n  ${lines.join("\n  ")}`,
  );
}

/**
 * Marks the findings in the artefact itself, not only on a console.
 *
 * Both lists are capped. A run in which every state drew one screen produces a
 * finding per group and a missing entry per render, and a banner carrying all
 * of them unabridged is megabytes of text at the top of the page a reviewer
 * came to read — the run where the gallery most needs to stay legible is
 * exactly the run that would make it unreadable.
 */
async function stamp(lines, missing) {
  const shown = lines.slice(0, 20);
  const banner = '<p style="margin:0;padding:.75rem 1.5rem;background:#ffebe9;color:#cf222e;font-weight:700">'
    + `This gallery is not the whole matrix, or not every state in it draws its own screen: ${shown.join("; ")}`
    + `${lines.length > shown.length ? `; and ${lines.length - shown.length} more` : ""}. `
    + `${missing.length ? `Missing: ${missing.slice(0, 20).join(", ")}${missing.length > 20 ? ", …" : ""}` : ""}</p>`;
  const index = join(GALLERY, "index.html");
  const written = await readFile(index, "utf8").catch(() => null);
  if (written) {
    await writeFile(index, written.replace("<main>", `${banner}<main>`), "utf8");
  }
}
