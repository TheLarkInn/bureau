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
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { crc32 } from "node:zlib";

import globalTeardown, { auditGallery, resolveDirs, runAudit } from "../e2e/playwright/global-teardown.mjs";
import { PNG_HEAD, shotName } from "../e2e/playwright/gallery-audit.mjs";
import { indexPage, rowsFor } from "../e2e/playwright/gallery-index.mjs";
import { openGallery, publishGallery } from "../e2e/playwright/gallery.mjs";
import { GALLERY, stagingFor, staging, STAGING_ENV } from "../e2e/playwright/gallery-paths.mjs";
import { parse as parseYaml } from "../lib/vendor/yaml.mjs";
import { STATES, RENDER_TWINS } from "../web/statelab/registry.mjs";
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

/** A whole, valid 1×1 PNG — header, one IDAT, and the IEND that closes it. */
const ONE_PIXEL = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

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
 *
 * The render is a real PNG rather than the word `png`, and that is load-bearing
 * now: the audit reads both ends of every published file, so a fixture that is
 * not a PNG would report a malformed render in every test here and drown the
 * finding each one is actually about.
 */
async function staged(t, index, shot, bytes = ONE_PIXEL, record = { signature: "one", settled: false }) {
  const root = await mkdtemp(join(tmpdir(), "bureau-audit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const stage = join(root, "staging");
  await mkdir(join(stage, "signatures"), { recursive: true });
  await writeFile(join(stage, "index.html"), index, "utf8");
  await writeFile(join(stage, shot), bytes);
  await writeFile(join(stage, "signatures", `${shot}.json`), JSON.stringify(record), "utf8");
  return { staging: stage, gallery: join(root, "gallery") };
}

/** The audit, with its console quiet: what it prints is not what is under test. */
async function audited(dirs, resolve) {
  const spoke = console.log;
  console.log = () => {};
  try {
    return dirs ? await auditGallery(dirs, resolve ?? resolveDirs) : await runAudit();
  } finally {
    console.log = spoke;
  }
}

/**
 * A staging directory holding several renders, each with the record it filed.
 *
 * `staged` above is the one-render shape most of these tests want. A comparison
 * needs two, because the finding is about a pair, and both sides must reach the
 * audit through the same real path a run uses.
 */
async function stagedPairs(t, index, shots) {
  const root = await mkdtemp(join(tmpdir(), "bureau-audit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const stage = join(root, "staging");
  await mkdir(join(stage, "signatures"), { recursive: true });
  await writeFile(join(stage, "index.html"), index, "utf8");
  for (const [shot, record] of shots) {
    await writeFile(join(stage, shot), ONE_PIXEL);
    await writeFile(join(stage, "signatures", `${shot}.json`), JSON.stringify(record), "utf8");
  }
  return { staging: stage, gallery: join(root, "gallery") };
}

/**
 * A comparison the audit makes is a comparison the run is answerable for.
 *
 * `auditTwins` is pinned in the unit table, and the unit table hands it records
 * it built itself. Nothing asked whether the answer reaches the caller that owns
 * the verdict — and it did not: `report` folded the claims into the console and
 * the banner and returned `{ ran, incomplete }`, so `broken-twin` and
 * `undeclared-twin` could not be asserted without changing the signature first.
 * Delete `matched()`, delete `parted()`'s `broken-twin` arm, or delete the whole
 * comparison, and the full matrix stayed green. The rule was proved and its one
 * consumer was not, which is the same hole one step along: a declared twin could
 * part — an entry operation quietly becoming a no-op, which is the defect the
 * 256-state matrix exists to catch — and the run would report it in amber and
 * exit 0.
 *
 * So both kinds go in through the real audit, off a real disk, and are read off
 * the returned object rather than a console. Three rows, because a claim that
 * fired on everything would gate every run and be switched off within a week:
 * the matching declared twin must produce no claim at all.
 */
test("a parted twin and an undeclared match are on the object the gate reads", async (t) => {
  const twin = RENDER_TWINS[0];
  const [one, other] = [shotName(twin.a, "desktop"), shotName(twin.b, "desktop")];
  const settled = (signature) => ({ signature, settled: true });
  const cases = [
    [[[one, settled("a")], [other, settled("b")]], "broken-twin"],
    [[[one, settled("same")], [other, settled("same")]], null],
    [[[shotName(STATES[0].id, "desktop"), settled("x")], [shotName(STATES[1].id, "desktop"), settled("x")]], "undeclared-twin"],
  ];

  const found = [];
  for (const [shots] of cases) {
    const audit = await audited(await stagedPairs(t, realIndex(), shots));
    found.push(audit.claims.map((line) => line.slice(0, line.indexOf(":"))));
  }

  assert.deepEqual(found, cases.map(([, kind]) => (kind ? [kind] : [])));
});

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
 * A gallery with no index is audited, not excused.
 *
 * This is the shape of hole the whole branch is written against, made by the
 * audit's own control flow. `auditGallery` used to return `ran: false` for a run
 * that published renders and no `index.html`, and `ran: false` is what
 * `specs/gallery.audit.spec.mjs` *skips* on — so removing the index from a full
 * matrix run did not produce a failing audit, it produced a run with no audit in
 * it at all, reported as `3 passed, 1 skipped` and a zero exit.
 *
 * The absence of the index was excusing the audit from noticing the absence of
 * the index. So the run is audited either way, and the missing index is one of
 * the findings — asserted here from the outside, on `ran` and on `incomplete`
 * together, because a finding recorded on a result nobody reads is the same
 * defect one step along.
 */
test("a gallery published without an index is a finding rather than a skipped audit", async (t) => {
  const shot = SHOT(STATES[0], VIEWPORT_LIST[0]);
  const dirs = await staged(t, realIndex(), shot);
  await rm(join(dirs.staging, "index.html"));

  const audit = await audited(dirs);

  assert.deepEqual(
    [audit.ran, audit.incomplete.some((line) => line.includes("no index"))],
    [true, true],
  );
});

/**
 * And a render with nothing in it is a hole in the gallery.
 *
 * Completeness was arithmetic over a *file list*, which is the one thing a
 * broken render still looks correct in: truncating every published PNG to zero
 * bytes left the gallery suites green at 54 of 54, because every expected name
 * was present and no one had ever asked whether the files had anything behind
 * them. A reviewer following that gallery meets five hundred broken images under
 * five hundred headings.
 *
 * Both accidents are asked about in one table: a file created and never written,
 * and bytes that stop before the PNG does. The second is the one a size check
 * alone would miss, and it is the likelier of the two — it is what a worker
 * killed mid-write leaves behind.
 *
 * The fourth row is the one that got past the check when it read only the two
 * ends: sixteen bytes that are a PNG signature with the closing chunk stapled
 * straight onto it. Both ends were perfect and there was no image between them,
 * so it is here rather than only in the unit table — this is the path a real
 * file takes, through `readEnds` and off a real disk.
 *
 * The fifth is the same lesson one layer in, and it is here for the same reason.
 * `walkChunks` is proved in the unit table, but the unit table builds its own
 * shots: nothing asked whether `readEnds` ever hands the walk's answer to the
 * audit. Replacing `chunks: walkChunks(bytes)` with the constant `["IHDR",
 * "IDAT", "IEND"]` left all 443 offline tests green, restoring the envelope-only
 * audit through a line that still reads as if it walks. So a render that is
 * whole at both ends, correct in every length and every checksum, and holds no
 * image data at all goes in off a real disk — a file no clause but the walk can
 * reject, through the one binding nothing had asked about.
 */
test("a render with no bytes, and one that stops before the PNG does, are both findings", async (t) => {
  const shot = SHOT(STATES[0], VIEWPORT_LIST[0]);
  const cases = [
    { bytes: Buffer.alloc(0), says: "no bytes in them" },
    { bytes: ONE_PIXEL.subarray(0, ONE_PIXEL.length - 4), says: "not a whole PNG" },
    { bytes: Buffer.from("iVBORw0KGgpJRU5ErkJggg==", "base64"), says: "not a whole PNG" },
    { bytes: withoutImageData(), says: "not a whole PNG" },
    { bytes: ONE_PIXEL, says: null },
  ];

  const found = [];
  for (const { bytes, says } of cases) {
    const audit = await audited(await staged(t, realIndex(), shot, bytes));
    const named = audit.incomplete.filter((line) => line.includes("no bytes in them") || line.includes("not a whole PNG"));
    found.push(says ? named.length === 1 && named[0].includes(says) && named[0].includes(shot) : named.length === 0);
  }

  assert.deepEqual(found, [true, true, true, true, true]);
});

/**
 * A structurally perfect PNG that holds no picture: the real one-pixel render
 * with its only `IDAT` renamed, and every checksum repaired afterwards.
 *
 * Repaired, because a renamed chunk breaks its own CRC too and a fixture that
 * fails for two reasons pins neither. The checksums come from `zlib.crc32`
 * rather than from the audit's own table, so a fixture cannot agree with an
 * error in the implementation it is meant to catch.
 */
function withoutImageData() {
  const bytes = Buffer.from(ONE_PIXEL);
  bytes.write("JUNK", PNG_HEAD + 4, "latin1");
  for (let at = 8; at + 12 <= bytes.length;) {
    const end = at + 12 + bytes.readUInt32BE(at);
    bytes.writeUInt32BE(crc32(bytes.subarray(at + 4, end - 4)), end - 4);
    at = end;
  }
  return bytes;
}

/**
 * A record that says nothing about settling is a finding, not a record to skip.
 *
 * `auditMotion` grew an `unproved` list for exactly this: its first two rules
 * were written as filters over records carrying a boolean, so a record with no
 * `settled` field fell out of both and was reported by neither. But the list was
 * only ever asked for directly, in the unit table — no test carried such a
 * record through `auditGallery`, so the line in `report` that turns it into a
 * finding could be deleted with all 438 offline tests green. The rule was
 * proved and its one consumer was not, which is the same hole one step along:
 * a run could publish five hundred renders it can say nothing about and report
 * it nowhere.
 *
 * So a record goes in without the field, through the real audit, off a real
 * disk. Three rows, because absence is not the only way to say nothing: a
 * non-boolean is evidence too and must read the same, and a record that *does*
 * carry the proof must not be named — an `unproved` list that fires on
 * everything would gate every run and be switched off within a week.
 */
test("a render whose record carries no settle evidence is a finding on the run's own artefact", async (t) => {
  const shot = SHOT(STATES[0], VIEWPORT_LIST[0]);
  const cases = [
    [{ signature: "one" }, true],
    [{ signature: "one", settled: "yes" }, true],
    [{ signature: "one", settled: true }, false],
  ];

  const found = [];
  for (const [record] of cases) {
    const audit = await audited(await staged(t, realIndex(), shot, ONE_PIXEL, record));
    found.push(audit.incomplete.some((line) => line.includes("no settle evidence") && line.includes(shot)));
  }

  assert.deepEqual(found, cases.map(([, named]) => named));
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
 * …and that gallery is the directory a reviewer is actually sent to.
 *
 * The test above compares the resolved pair against `GALLERY`, which it imports
 * from the module that produced it. Both ends of that comparison are the same
 * binding, so it holds for any value: point `GALLERY` at `../gallery-review/`
 * and the suite follows it there, green, while the workflow keeps uploading
 * `e2e/gallery/` and a reviewer downloads an empty artefact. A check whose
 * expected value comes from the thing under test is exactly the defect this
 * branch has been closing, standing in the check written to close it.
 *
 * The reviewer-facing path is therefore spelled here, independently, and read
 * back out of the workflow that publishes it — the only two places that decide
 * what a human ends up browsing. Moving the gallery now has to move both, which
 * is the point: they are one contract, not two coincidences.
 */
const REVIEWER_GALLERY = ".github/extensions/bureau-canvas/e2e/gallery/";

/**
 * The events that start the workflow.
 *
 * A step that publishes, in a job that starts, inside a workflow that nothing
 * ever triggers, publishes exactly as much as a step that never runs — and this
 * gallery exists for the reviewer of a *pull request*. Deleting one line of
 * `on:` leaves every reader above answering exactly as it does now.
 *
 * Read from the same parse as the rest, because `on` is three shapes and a
 * trap. It may be a scalar, a sequence, or a mapping; and under YAML 1.1 the
 * key `on` is the *boolean* `true`, which is why this asks the parser rather
 * than assuming — the vendored `yaml` is 1.2, where it stays the string it
 * looks like, and a quoted `"on"` is the same key either way.
 */
function triggersOf(workflow) {
  const on = parseYaml(workflow)?.on;
  if (typeof on === "string") {
    return [on];
  }
  return Array.isArray(on) ? on.filter((event) => typeof event === "string") : Object.keys(on ?? {});
}

/**
 * The `upload-artifact` step, and the job that holds it, as YAML itself reads
 * them.
 *
 * These were four hand-rolled scanners over the file's text, and every round
 * found another spelling that walked past one of them: an input named `if:`, a
 * folded `>-` name whose continuation sat deeper than the keys it was read
 * against, an over-indented comment in that line's place, `if :` spaced away
 * from its colon, `"if"` in quotes. Each fix was right about the case in front
 * of it and the next case was still there, because "which key is this, and
 * whose mapping is it in" is a question about the parse, and it was being
 * answered by looking at characters.
 *
 * Two more arrived this round and settle the argument. `"\u0069f": false` is
 * the key `if` — a double-quoted scalar is escape-decoded, so the letters are
 * not in the file at all — and a comment written at the job's own indent ends
 * no mapping, though a scan for "the next line no deeper than this" reads it
 * as the end of the job. Both defeat the reader that fails *open*: the job's
 * condition is asserted to be absent, so a job that never starts, and
 * therefore publishes nothing, reads as a job with nothing in its way.
 *
 * So the workflow is parsed, by the same vendored YAML the extension itself
 * ships. Spelling stops being a variable: the step is the one whose `uses:`
 * names the action, the job is the mapping it is genuinely inside, and a key
 * is a key however it is written.
 */
function publication(workflow) {
  for (const job of Object.values(parseYaml(workflow)?.jobs ?? {})) {
    const step = (job?.steps ?? []).find((entry) => typeof entry?.uses === "string" && entry.uses.startsWith("actions/upload-artifact@"));
    if (step) {
      return { step, job };
    }
  }
  return { step: {}, job: {} };
}

/**
 * A condition as GitHub would evaluate it, or `""` when the key is absent.
 *
 * `if: false` is a boolean to YAML and a false condition to GitHub, so the
 * value is stringified rather than tested for truth: the distinction each
 * assertion needs is present-and-false against absent, and only the second of
 * those is `""`.
 */
function conditionOf(holder) {
  return holder?.if === undefined ? "" : String(holder.if).trim();
}

/**
 * The path patterns the step hands `upload-artifact`.
 *
 * `upload-artifact` reads a leading `!` as an *exclusion*, so a `path:` block
 * that still contains the gallery's own line can still publish an artefact
 * with nothing in it — the positive line being present was never the claim,
 * the gallery being *selected* is. The patterns are read from this step's own
 * `with.path`, so an input belonging to another step, a comment, or an
 * `on.push.paths` filter is not one of them.
 */
function publishedBy(step) {
  const path = step?.with?.path;
  return typeof path === "string" ? path.split("\n").map((line) => line.trim()).filter(Boolean) : [];
}

test("the gallery a run resolves to is the directory CI publishes for a reviewer", async () => {
  const before = process.env[STAGING_ENV];
  process.env[STAGING_ENV] = "/tmp/bureau-staging-under-test";

  const resolved = resolveDirs().gallery;
  const workflow = await readFile(new URL("../../../workflows/canvas-state-matrix.yml", import.meta.url), "utf8");
  const { step, job } = publication(workflow);
  const published = publishedBy(step);

  process.env[STAGING_ENV] = before ?? "";
  assert.deepEqual(
    [
      resolved.endsWith(`/${REVIEWER_GALLERY}`),
      published.includes(REVIEWER_GALLERY),
      published.filter((pattern) => pattern.startsWith("!")),
      conditionOf(step),
      conditionOf(job),
      triggersOf(workflow).includes("pull_request"),
    ],
    [true, true, [], "always()", "", true],
  );
});

/**
 * …and the trigger reader, held against the spellings of `on:`.
 *
 * A miss here fails open in the same direction as the job's condition: the
 * workflow is asserted to run on a pull request, so a reader that cannot see
 * the event it is looking for reports a workflow nothing starts as one that
 * publishes for every reviewer.
 */
const TRIGGER_SPELLINGS = [
  { id: "a sequence", on: "on: [push, pull_request]", starts: true },
  { id: "a mapping", on: "on:\n  pull_request:\n  push:\n    branches: [main]", starts: true },
  { id: "a lone scalar", on: "on: pull_request", starts: true },
  { id: "a quoted key", on: '"on": [pull_request]', starts: true },
  { id: "…and one whose letters are written as escapes", on: '"\\u006fn": [pull_request]', starts: true },
  { id: "push alone", on: "on: [push]", starts: false },
  { id: "a mapping holding every other event", on: 'on:\n  push:\n    branches: [main]\n  schedule:\n    - cron: "23 8 * * *"\n  workflow_dispatch:', starts: false },
  { id: "an event that merely begins the same way", on: "on: [pull_request_target]", starts: false },
  { id: "no trigger at all", on: "name: Canvas state matrix", starts: false },
];

test("a workflow nothing starts on a pull request publishes nothing for its reviewer", () => {
  const read = TRIGGER_SPELLINGS.map((found) => triggersOf(`${found.on}
jobs:
  matrix:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/upload-artifact@v4.4.3
`).includes("pull_request"));

  assert.deepEqual(read, TRIGGER_SPELLINGS.map((found) => found.starts));
});

/**
 * …and the readers above, held against the workflows they must refuse.
 *
 * A miss is not neutral: the step's condition is asserted to be `always()`, so
 * a miss there fails closed, but the *job's* is asserted to be absent, so a
 * miss there fails open. A spelling the reader does not recognise then reads
 * as "nothing stands in the way of this run" over a job that never starts.
 *
 * Every case below is valid YAML whose path patterns are exactly right and
 * which publishes nothing at all, and each was green against the reader it was
 * written to hold. They are kept rather than run once and thrown away, because
 * what defeated these readers five times over was never the case anyone
 * thought to write a test for — and the last two are the reason the readers
 * are a parse now rather than a scan.
 */
const HOSTILE_WORKFLOWS = [
  {
    id: "an input named `if:`, over a step condition that is false",
    job: "", step: "        if: 0 == 1\n", input: "          if: always()\n", name: "Upload state gallery",
    reads: ["0 == 1", ""],
  },
  {
    id: "…and the step's name folded, so its first line is deeper than its keys",
    job: "", step: "        if: 0 == 1\n", input: "          if: always()\n", name: ">-\n          Upload state gallery",
    reads: ["0 == 1", ""],
  },
  {
    id: "…and an over-indented comment in that line's place",
    job: "", step: "        if: 0 == 1\n", input: "          if: always()\n",
    name: "Upload state gallery\n          # the run a reviewer needs this from is the one that failed",
    reads: ["0 == 1", ""],
  },
  {
    id: "a job condition spaced away from its colon",
    job: "    if : false\n", step: "        if: always()\n", input: "", name: "Upload state gallery",
    reads: ["always()", "false"],
  },
  {
    id: "…and one written as a quoted key",
    job: '    "if": false\n', step: "        if: always()\n", input: "", name: "Upload state gallery",
    reads: ["always()", "false"],
  },
  {
    // A double-quoted scalar is escape-decoded, so the key is `if` and the
    // letters `i` and `f` are nowhere in the file. Nothing that compares
    // characters can see this one at all.
    id: "…and one whose letters are written as escapes",
    job: '    "\\u0069f": false\n', step: "        if: always()\n", input: "", name: "Upload state gallery",
    reads: ["always()", "false"],
  },
  {
    // A comment belongs to no mapping and may sit at any indent, including one
    // shallower than the keys around it. It does not end the job; a scan for
    // "the next line no deeper than this" believes it does, and then reads the
    // condition below it as belonging to nothing.
    id: "…and one below a comment written shallower than the job's own keys",
    job: "  # a comment closes no mapping, whatever column it starts in\n    if: false\n",
    step: "        if: always()\n", input: "", name: "Upload state gallery",
    reads: ["always()", "false"],
  },
];

function hostileWorkflow(found) {
  return `on: [push]
jobs:
  matrix:
    name: Exhaustive UI state coverage
${found.job}    runs-on: ubuntu-latest
    steps:
      - name: ${found.name}
${found.step}        uses: actions/upload-artifact@v4.4.3
        with:
${found.input}          name: canvas-state-gallery
          path: |
            ${REVIEWER_GALLERY}
`;
}

test("a step that never runs, and a job that never starts, are read as written", () => {
  const read = HOSTILE_WORKFLOWS.map((found) => {
    const workflow = hostileWorkflow(found);
    const { step, job } = publication(workflow);
    return [conditionOf(step), conditionOf(job), publishedBy(step).includes(REVIEWER_GALLERY)];
  });

  assert.deepEqual(read, HOSTILE_WORKFLOWS.map((found) => [...found.reads, true]));
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
 * discover there was nothing in it — which is required as the effect it is,
 * rather than as the sentence it returns: the directory this test made has to
 * be gone, and a body that answers without looking cannot make it so.
 */
test("the audit a teardown really runs asks resolveDirs for its directories", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "bureau-audit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const stage = join(root, "staging");
  await mkdir(stage, { recursive: true });
  const before = process.env[STAGING_ENV];
  process.env[STAGING_ENV] = stage;

  const audit = await audited().finally(() => {
    process.env[STAGING_ENV] = before ?? "";
  });

  assert.deepEqual([audit.ran, audit.reason, audit.staging, existsSync(stage)], [false, "this run rendered no states, so there is no gallery to audit", stage, false]);
});

/**
 * …and the resolver it is handed is `resolveDirs` itself, not a copy of it.
 *
 * Every test here is behavioural, and a faithful copy of a rule is behaviourally
 * indistinguishable from the rule — that is what makes a second spelling
 * survivable in the first place. While `auditGallery` carried `resolve =
 * resolveDirs` as a default, writing that default out inline instead left the
 * whole suite green, including the test above: the run with no arguments cannot
 * tell which of two identical functions answered it, and every other test passes
 * its own resolver and overrides the default entirely.
 *
 * Identity is the one question that separates them. `runAudit` is the single
 * place the rule is handed over, so this asks what it hands over, and gets a
 * function rather than a directory pair back. An inline copy is a different
 * function object and fails here while behaving identically everywhere else.
 *
 * Both directions of that hand-over are asked, because the first alone is held
 * by construction: a `runAudit` that ignored its parameter and wrote
 * `audit({}, resolveDirs)` in its body would satisfy an identity check made
 * against the default, and its seam would be decoration. So the second call
 * names a resolver of its own and requires *that* one to arrive.
 *
 * The audit itself is a stand-in, so nothing is published and no directory is
 * read: what is under test is the wiring, and running a real audit to observe it
 * would put a reviewer's gallery in the path of a test about a parameter.
 */
test("the teardown hands the audit resolveDirs itself, not a second spelling of it", async () => {
  const handed = [];
  const spy = (dirs, resolve) => (handed.push({ dirs, resolve }), "audited");
  const named = () => ({ staging: "/tmp/a", gallery: "/tmp/b" });

  const returned = [await runAudit(spy), await runAudit(spy, named)];

  assert.deepEqual(
    [returned, handed.map((call) => call.dirs), handed[0]?.resolve === resolveDirs, handed[1]?.resolve === named],
    [["audited", "audited"], [{}, {}], true, true],
  );
});

/**
 * …and the teardown Playwright actually loads is that hand-over, not a copy.
 *
 * Every test above reaches `runAudit` by importing it. Nothing imported the
 * module's *default export*, which is the only function a real run calls — so
 * `runAudit` could be proved exactly, in both directions, while the thing
 * Playwright loads went around it:
 *
 *   export default async function globalTeardown() {
 *     await auditGallery({}, (dirs) => ({
 *       staging: dirs.staging ?? staging(), gallery: dirs.gallery ?? GALLERY,
 *     }));
 *   }
 *
 * That behaves identically on every run, so no behavioural test here or in the
 * browser suite can tell it apart — and it is the duplicate spelling of the
 * resolver rule that `resolveDirs` exists to be the only one of, restored in
 * the one function none of these tests had ever called.
 *
 * So the default export is called the way Playwright calls it, with a config
 * and nothing else, and asked the same two questions `runAudit` is asked: the
 * defaulted call must hand over `resolveDirs` itself, and a call naming its own
 * resolver must hand over that one. A body that bypasses `runAudit` never calls
 * the stand-in at all.
 */
test("the teardown Playwright loads hands the audit resolveDirs itself", async () => {
  const handed = [];
  const spy = (dirs, resolve) => (handed.push({ dirs, resolve }), "audited");
  const named = () => ({ staging: "/tmp/a", gallery: "/tmp/b" });
  const config = { rootDir: "/tmp/bureau-never-read" };

  const returned = [await globalTeardown(config, spy), await globalTeardown(config, spy, named)];

  assert.deepEqual(
    [returned, handed.map((call) => call.dirs), handed[0]?.resolve === resolveDirs, handed[1]?.resolve === named],
    [["audited", "audited"], [{}, {}], true, true],
  );
});

/**
 * …and it does the real audit when it is handed nothing, which is every run.
 *
 * The test above injects a stand-in on *both* of its calls, so the parameters
 * were proved to be forwarded and the un-forwarded path — the only one
 * Playwright ever takes, because it calls a global teardown with the config and
 * nothing else — was never walked. Adding a guard on the seam:
 *
 *   export default async function globalTeardown(_config, audit, resolve) {
 *     if (audit === undefined) {
 *       return;
 *     }
 *     return runAudit(audit, resolve);
 *   }
 *
 * leaves every test here green, every identity check on `runAudit` green, and
 * the browser suite green — `specs/gallery.audit.spec.mjs` calls `runAudit`
 * directly and never reaches this function — while the teardown a real run
 * loads does nothing at all. A seam proved only through the stand-in that uses
 * it is a seam, not a teardown.
 *
 * So it is called exactly as Playwright calls it, over a staging directory this
 * test owns and left empty. Empty is the safe way to ask: `publishGallery`
 * discards an empty staging directory and returns before it can touch
 * `GALLERY`, so a reviewer's gallery is never at risk.
 *
 * And what is required of it is that discarding: the directory this test made
 * is gone afterwards. The returned value alone is a literal — `{ ran: false,
 * reason: …, incomplete: [] }` written straight into the guard above answers
 * this test correctly without resolving a path or reading a directory, which is
 * the same vacuous pass one layer in. A directory this test created and the
 * teardown removed is an effect, and nothing that returned early can produce it.
 */
test("the teardown Playwright loads audits for real when it is handed nothing", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "bureau-audit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const stage = join(root, "staging");
  await mkdir(stage, { recursive: true });
  const before = process.env[STAGING_ENV];
  process.env[STAGING_ENV] = stage;
  const spoke = console.log;
  console.log = () => {};

  const audit = await globalTeardown({ rootDir: root }).finally(() => {
    console.log = spoke;
    process.env[STAGING_ENV] = before ?? "";
  });

  assert.deepEqual(
    [audit, existsSync(stage)],
    [{
      ran: false,
      reason: "this run rendered no states, so there is no gallery to audit",
      incomplete: [],
      claims: [],
      staging: stage,
    }, false],
  );
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
