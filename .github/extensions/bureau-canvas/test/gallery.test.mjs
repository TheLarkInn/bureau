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

import globalTeardown, { auditGallery, resolveDirs, runAudit } from "../e2e/playwright/global-teardown.mjs";
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
    return dirs ? await auditGallery(dirs, resolve ?? resolveDirs) : await runAudit();
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
 * The path patterns the workflow's `actions/upload-artifact` step publishes.
 *
 * Read as that step's own `path:` block rather than as a line anywhere in the
 * file, because a line search cannot tell a published path from a mention of
 * one — a comment, an `on.push.paths` filter, or a different step's input all
 * satisfy it — and, more to the point, it cannot see the one thing that decides
 * whether a reviewer receives the artefact. `upload-artifact` reads a leading
 * `!` as an *exclusion*, so
 *
 *   path: |
 *     .github/extensions/bureau-canvas/e2e/gallery/
 *     !.github/extensions/bureau-canvas/e2e/gallery/**
 *     .github/extensions/bureau-canvas/e2e/playwright/playwright-report/
 *
 * still contains the line the check was looking for, still uploads an artefact,
 * still goes green — and publishes a gallery with nothing in it. The positive
 * line being present was never the claim; the gallery being *selected* is.
 *
 * "That step's own block" is meant literally, and reading it as "anything after
 * the `uses:` line" was the same defect one notch along: renaming the input to
 * `paths:` and adding an unrelated later step carrying a `path:` block of the
 * right shape satisfied the check while `upload-artifact` received no path at
 * all. The step is bounded by its own list-item indent, so a key belonging to
 * another step is not a key of this one.
 *
 * Its `if:` is read for the same reason. Every path pattern can be correct and
 * the step still never run: `if: 0 == 1` publishes nothing, and a check that
 * reads inputs alone approves it. `always()` is required rather than merely
 * something truthy, because the run a reviewer most needs the gallery from is
 * the run that failed.
 */
function uploadStep(workflow) {
  const lines = workflow.split("\n");
  const uses = lines.findIndex((line) => line.trim().startsWith("uses: actions/upload-artifact@"));
  const start = uses === -1 ? -1 : lines.slice(0, uses + 1).findLastIndex((line) => line.trimStart().startsWith("- "));
  if (start === -1) {
    return [];
  }
  const indent = lines[start].search(/\S/u);
  const after = lines.slice(start + 1);
  const ends = after.findIndex((line) => line.trim() && line.search(/\S/u) <= indent);
  return [lines[start], ...after.slice(0, ends === -1 ? after.length : ends)];
}

/** The patterns under one key of a step, as that key's own indented block. */
function blockUnder(step, key) {
  const at = step.findIndex((line) => line.trim() === `${key}: |`);
  if (at === -1) {
    return [];
  }
  const indent = step[at].search(/\S/u);
  const after = step.slice(at + 1);
  const ends = after.findIndex((line) => line.trim() && line.search(/\S/u) <= indent);
  return after.slice(0, ends === -1 ? after.length : ends).map((line) => line.trim()).filter(Boolean);
}

/**
 * The indent a block's own keys are written at: the shallowest line in it that
 * is neither blank nor a comment.
 *
 * Not "whatever the first line happens to be indented by". Any line deeper than
 * the keys — a folded scalar's continuation, or an over-indented comment —
 * moves that reading down into `with:`, which is exactly where the input this
 * rule exists to reject lives:
 *
 *   - name: >-
 *       Upload state gallery
 *     if: 0 == 1
 *     with:
 *       if: always()
 *
 * Comments are excluded because they belong to no mapping and can be written at
 * any indent at all; `#` inside a block scalar is a literal pattern, and those
 * lines are deeper than the key they hang under, so they are never the
 * shallowest.
 */
function keyIndentOf(lines) {
  const keys = lines.filter((line) => line.trim() && !line.trim().startsWith("#"));
  return keys.length ? Math.min(...keys.map((line) => line.search(/\S/u))) : 0;
}

/**
 * The indent a step's keys sit at, read from the step's own dash.
 *
 * A block mapping in a sequence entry begins where the first key sits on the
 * dash line itself, so this is decided before any line below it is read and no
 * line added below can move it. A bare `-` with its keys on the following lines
 * has no such key, and falls back to the shallowest of them.
 */
function stepIndent(step) {
  const dash = step[0].search(/\S/u);
  const first = step[0].slice(dash + 1).search(/\S/u);
  return first === -1 ? keyIndentOf(step.slice(1)) : dash + 1 + first;
}

/**
 * `if:` as a key, not as a prefix of a trimmed line.
 *
 * `if : false` and `"if": false` are the same key to YAML, and neither begins
 * `if:`. Reading the key by prefix therefore reports them as *no condition at
 * all* — which matters most where the claim being made is an absence, because
 * there a miss fails open: a job that never starts reads as a job with nothing
 * standing in its way.
 */
const IF_KEY = /^(?<quote>['"]?)if\k<quote>\s*:(?<value>.*)$/u;

/** The condition written at exactly `indent` among `lines`, as written. */
function conditionAt(lines, indent) {
  const found = lines
    .filter((line) => line.search(/\S/u) === indent)
    .map((line) => IF_KEY.exec(line.trim()))
    .find(Boolean);
  return found ? found.groups.value.trim() : "";
}

/**
 * The condition a step runs under, as written — read at the step's own key
 * indent and nowhere deeper.
 *
 * `with:` is a map of *inputs*, and an input may be called anything at all. A
 * scan for the first line beginning `if:` after trimming therefore reads
 *
 *   with:
 *     if: always()
 *   if: 0 == 1
 *
 * as `always()`: an input GitHub hands to the action, approved as the condition
 * GitHub evaluates. Indentation is the only thing that separates them, so it is
 * not thrown away before the search — and the indent it is compared against
 * comes from the step's own dash rather than from a line inside it.
 */
function conditionOf(step) {
  return conditionAt(step, stepIndent(step));
}

/**
 * The condition the job containing the upload step runs under.
 *
 * A step that says `always()` inside a job that never starts publishes exactly
 * as much as a step that says nothing. `if:` at the job's own key indent is the
 * one that decides whether any of this runs at all.
 */
function jobConditionOf(workflow) {
  const lines = workflow.split("\n");
  const uses = lines.findIndex((line) => line.trim().startsWith("uses: actions/upload-artifact@"));
  const job = uses === -1 ? -1 : lines.slice(0, uses + 1).findLastIndex((line) => /^ {2}\S.*:\s*$/u.test(line));
  if (job === -1) {
    return "";
  }
  const after = lines.slice(job + 1);
  const ends = after.findIndex((line) => line.trim() && line.search(/\S/u) <= 2);
  const body = after.slice(0, ends === -1 ? after.length : ends);
  return conditionAt(body, keyIndentOf(body));
}

test("the gallery a run resolves to is the directory CI publishes for a reviewer", async () => {
  const before = process.env[STAGING_ENV];
  process.env[STAGING_ENV] = "/tmp/bureau-staging-under-test";

  const resolved = resolveDirs().gallery;
  const workflow = await readFile(new URL("../../../workflows/canvas-state-matrix.yml", import.meta.url), "utf8");
  const step = uploadStep(workflow);
  const published = blockUnder(step, "path");

  process.env[STAGING_ENV] = before ?? "";
  assert.deepEqual(
    [
      resolved.endsWith(`/${REVIEWER_GALLERY}`),
      published.includes(REVIEWER_GALLERY),
      published.filter((pattern) => pattern.startsWith("!")),
      conditionOf(step),
      jobConditionOf(workflow),
    ],
    [true, true, [], "always()", ""],
  );
});

/**
 * …and the two readers above, held against the workflows they must refuse.
 *
 * Both are scanners over text, and a scanner that misses is not neutral: the
 * step's condition is asserted to be `always()`, so a miss there fails closed,
 * but the *job's* is asserted to be absent, so a miss there fails open. A
 * spelling neither one recognises reads as "nothing stands in the way of this
 * run" over a job that never starts.
 *
 * Every case below is valid YAML whose path patterns are exactly right and
 * which publishes nothing at all, and each was green against the reader this
 * fixture was written to hold.
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
    const step = uploadStep(workflow);
    return [conditionOf(step), jobConditionOf(workflow), blockUnder(step, "path").includes(REVIEWER_GALLERY)];
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
