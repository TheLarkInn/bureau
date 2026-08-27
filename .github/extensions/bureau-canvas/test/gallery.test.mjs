// The gallery is replaced by the run that filled it, and by no other.
//
// This is the guard on a silent artefact loss in both directions: a run that
// renders nothing (`test:pr`, `test:visual`) must leave a reviewer's gallery
// alone, and a run that does render must not leave behind the shot of a state
// the registry has since dropped.
//
// Deliberately no clock anywhere. An earlier version decided this by comparing
// file times against the run's start, and CI — whose filesystem records mtime
// more coarsely than `Date.now()` reports it — recorded a shot written *after*
// the run began as older than it.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { auditGallery, resolveDirs } from "../e2e/playwright/global-teardown.mjs";
import { shotName } from "../e2e/playwright/gallery-audit.mjs";
import { indexPage, rowsFor } from "../e2e/playwright/gallery-index.mjs";
import { openGallery, publishGallery } from "../e2e/playwright/gallery.mjs";
import { GALLERY, stagingFor, staging, STAGING_ENV } from "../e2e/playwright/gallery-paths.mjs";
import { STATES } from "../web/statelab/registry.mjs";
import { VIEWPORTS } from "../web/statelab/selectors.mjs";

/** A staging and a gallery directory, with the gallery already populated. */
async function pair(t, published) {
  const root = await mkdtemp(join(tmpdir(), "bureau-gallery-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const gallery = join(root, "gallery");
  await mkdir(gallery, { recursive: true });
  for (const name of published) {
    await writeFile(join(gallery, name), name, "utf8");
  }
  return { staging: join(root, "staging"), gallery };
}

test("a run that rendered nothing leaves the published gallery in place", async (t) => {
  const { staging: stage, gallery } = await pair(t, ["desktop--old.png", "index.html"]);

  await openGallery(stage);
  const published = await publishGallery(stage, gallery);

  assert.deepEqual(
    [published, (await readdir(gallery)).sort(), await readdir(stage).catch(() => "gone")],
    [[], ["desktop--old.png", "index.html"], "gone"],
  );
});

test("a run that rendered replaces the gallery with exactly what it wrote", async (t) => {
  const { staging: stage, gallery } = await pair(t, ["desktop--dropped.png", "desktop--kept.png"]);

  await openGallery(stage);
  await writeFile(join(stage, "desktop--kept.png"), "fresh", "utf8");
  const published = await publishGallery(stage, gallery);

  assert.deepEqual([published, await readdir(gallery)], [["desktop--kept.png"], ["desktop--kept.png"]]);
});

test("opening staging discards whatever a crashed run left in it", async (t) => {
  const { staging: stage, gallery } = await pair(t, []);
  await mkdir(stage, { recursive: true });
  await writeFile(join(stage, "desktop--abandoned.png"), "stale", "utf8");

  await openGallery(stage);
  const published = await publishGallery(stage, gallery);

  assert.deepEqual([published, await readdir(gallery)], [[], []]);
});

/**
 * Two runs in one checkout must not share a staging directory: `openGallery`
 * empties what it is given, so a fixed path let a `test:visual` started beside
 * a running matrix delete the shots that run had already written, and the
 * matrix would publish the remainder and print a count for it.
 */
test("each run stages under its own process, and a worker without one refuses", () => {
  const before = process.env[STAGING_ENV];
  delete process.env[STAGING_ENV];
  const refused = (() => {
    try {
      staging();
      return null;
    } catch (error) {
      return error.message;
    }
  })();
  process.env[STAGING_ENV] = before ?? "";
  assert.deepEqual(
    [stagingFor(1) === stagingFor(2), refused?.includes(STAGING_ENV) === true],
    [false, true],
  );
});

const VIEWPORT_LIST = Object.values(VIEWPORTS);
const SHOT = (state, viewport) => shotName(state.id, viewport.id);

/** The index page the matrix really writes, over the real registry. */
function realIndex() {
  return indexPage(rowsFor(STATES, VIEWPORT_LIST, SHOT), STATES, VIEWPORT_LIST);
}

/**
 * A staging directory holding one render, the record it filed, and an index.
 *
 * Deliberately one render out of the registry's several hundred, so the audit
 * has something to report and the alarm banner is raised — which is the only
 * condition under which a banner is written at all, and therefore the only way
 * to ask whether the written one reaches disk.
 */
async function staged(t, index, shot) {
  const root = await mkdtemp(join(tmpdir(), "bureau-audit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const stage = join(root, "staging");
  await mkdir(join(stage, "signatures"), { recursive: true });
  await writeFile(join(stage, "index.html"), index, "utf8");
  await writeFile(join(stage, shot), "png", "utf8");
  await writeFile(join(stage, "signatures", `${shot}.json`), JSON.stringify({ signature: "one", settled: false }), "utf8");
  return { staging: stage, gallery: join(root, "gallery") };
}

/** The audit, with its console quiet: what it prints is not what is under test. */
async function audited(dirs, resolve) {
  const spoke = console.log;
  console.log = () => {};
  try {
    return resolve ? await auditGallery(dirs, resolve) : await auditGallery(dirs);
  } finally {
    console.log = spoke;
  }
}

/**
 * The marks a run computes are the marks a reviewer meets.
 *
 * Every other test of the marking works on strings in memory. That left the
 * composition — apply the marks, write the marked page, fold what did not land
 * into the findings — reachable only through a full browser matrix, and each of
 * its three links could be cut with the offline suite still green. The worst
 * wrote the *unmarked* page back to disk: the banner and every amber figure
 * were computed correctly, `unmarked` was legitimately empty, the gate passed,
 * and the artefact a reviewer opens was clean and silent about a gallery
 * missing all but one of its renders.
 *
 * So this runs the real `auditGallery` over a real directory pair and reads the
 * published file back, because what is being asked is whether the mark reached
 * the page — not whether a function returned a string containing it.
 */
test("the marks a run computes are the ones its published gallery carries", async (t) => {
  const shot = SHOT(STATES[0], VIEWPORT_LIST[0]);
  const dirs = await staged(t, realIndex(), shot);

  const audit = await audited(dirs);
  const written = await readFile(join(dirs.gallery, "index.html"), "utf8");

  assert.deepEqual(
    [
      written.includes("This gallery is not the whole matrix"),
      written.includes(`data-shot="${shot}" data-settled="false"`),
      audit.incomplete.some((line) => line.includes("found no anchor")),
    ],
    [true, true, false],
  );
});

/**
 * And an index the marks cannot attach to fails the run.
 *
 * The other half of the same statement, from the side that used to be silent:
 * an index whose anchors have drifted still renders perfectly, and looks exactly
 * like a clean one. It must therefore be a finding the audit spec can gate on,
 * rather than a page published without comment.
 */
test("an index whose anchors have drifted is a finding, not a quiet publish", async (t) => {
  const shot = SHOT(STATES[0], VIEWPORT_LIST[0]);
  const drifted = realIndex().replace("<main>", '<main class="grid">').replaceAll("<figure data-shot=", '<figure class="shot" data-shot=');
  const dirs = await staged(t, drifted, shot);

  const audit = await audited(dirs);
  const written = await readFile(join(dirs.gallery, "index.html"), "utf8");

  assert.deepEqual(
    [
      audit.incomplete.some((line) => line.includes("found no anchor")),
      written.includes("This gallery is not the whole matrix"),
      written.includes(`data-shot="${shot}" data-settled="false"`),
    ],
    [true, false, false],
  );
});

/**
 * The pair a run with no arguments works over is the real one.
 *
 * Both tests above hand `auditGallery` explicit directories, which is what
 * makes them offline — and what leaves the two bindings that matter in
 * production untested by them. Swapping `GALLERY` for the staging directory, or
 * `staging()` for a constant, kept the whole offline suite green: the seam the
 * tests use was proved and the path a real run takes was not.
 */
test("a run given no directories works over this run's staging and the real gallery", () => {
  const before = process.env[STAGING_ENV];
  process.env[STAGING_ENV] = "/tmp/bureau-staging-under-test";

  const resolved = resolveDirs();
  const named = resolveDirs({ staging: "/tmp/a", gallery: "/tmp/b" });

  process.env[STAGING_ENV] = before ?? "";
  assert.deepEqual(
    [resolved, named],
    [{ staging: "/tmp/bureau-staging-under-test", gallery: GALLERY }, { staging: "/tmp/a", gallery: "/tmp/b" }],
  );
});

/**
 * …and `auditGallery` is the thing that asks for it.
 *
 * The test above proves `resolveDirs` alone, which is one binding short of the
 * statement. Drop the call — `const { staging: stageDir, gallery: outDir } =
 * dirs` — and it stays green, along with every other test here, because they
 * all pass both directories explicitly and never take the defaulting path. A
 * real teardown, which passes none, would then audit `undefined`.
 *
 * So the production entry point is called exactly as `globalTeardown` calls it,
 * with no arguments, over a staging directory that this test owns and left
 * empty. Empty is the safe way to ask: `publishGallery` discards an empty
 * staging directory and returns before it can touch `GALLERY`, so a reviewer's
 * gallery is never at risk, and the run still had to resolve a staging path to
 * discover there was nothing in it.
 */
test("the audit a teardown really runs asks resolveDirs for its directories", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "bureau-audit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const before = process.env[STAGING_ENV];
  process.env[STAGING_ENV] = join(root, "staging");

  const audit = await audited().finally(() => {
    process.env[STAGING_ENV] = before ?? "";
  });

  assert.deepEqual([audit.ran, audit.reason], [false, "this run rendered no states, so there is no gallery to audit"]);
});

/**
 * …and it works over the pair the resolver answered with, not the one it was handed.
 *
 * The test above proves the defaulting path is *taken*. It does not prove the
 * defaults are asked for in one place: writing `{ staging: dirs.staging ??
 * staging(), gallery: dirs.gallery ?? GALLERY }` inline inside `auditGallery`
 * behaves identically, so every test here — including that one — stays green
 * while the rule `resolveDirs` exists to be is spelled twice again. The next
 * change to either spelling then moves one of them.
 *
 * A resolver is answerable in a way a behavioural equivalence is not. This hands
 * `auditGallery` a directory pair that was never written to and a resolver that
 * ignores it, and requires the audit to have run over the resolver's answer.
 * An inline copy reads `dirs` directly, finds an empty staging directory, and
 * returns `ran: false` having published nothing — which is the whole distance
 * between a duplicate of the rule and a use of it.
 */
test("the audit works over the pair its resolver answers with, not the one it was handed", async (t) => {
  const shot = SHOT(STATES[0], VIEWPORT_LIST[0]);
  const answered = await staged(t, realIndex(), shot);
  const handed = { staging: join(tmpdir(), "bureau-never-staged"), gallery: join(tmpdir(), "bureau-never-published") };
  const asked = [];

  const audit = await audited(handed, (dirs) => (asked.push(dirs), answered));
  const written = await readFile(join(answered.gallery, "index.html"), "utf8");

  assert.deepEqual(
    [asked, audit.ran, written.includes("This gallery is not the whole matrix")],
    [[handed], true, true],
  );
});
