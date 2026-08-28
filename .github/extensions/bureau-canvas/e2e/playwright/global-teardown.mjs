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

import { open, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { RENDER_TWINS, STATES } from "../../web/statelab/registry.mjs";
import { VIEWPORTS } from "../../web/statelab/selectors.mjs";
import { auditBytes, auditMotion, auditNames, auditSettled, auditTwins, auditUnaudited, expectedShots, movingShots, partitionFindings, PNG_ENDS } from "./gallery-audit.mjs";
import { applyMarks, escape, SETTLED_INK, SETTLED_PHRASE } from "./gallery-index.mjs";
import { publishGallery } from "./gallery.mjs";
import { GALLERY, staging } from "./gallery-paths.mjs";

const SIGNATURES = "signatures";

/**
 * The teardown Playwright loads, which is `runAudit` and nothing else.
 *
 * `audit` and `resolve` are forwarded rather than defaulted here, and that is
 * the whole point of the parameters: while this took none, the body was a place
 * the resolver rule could be written a second time. `await auditGallery({},
 * (dirs) => ({ staging: dirs.staging ?? staging(), gallery: dirs.gallery ??
 * GALLERY }))` behaves identically to `runAudit()` on every run and left the
 * whole suite green — including the identity checks on `runAudit`, which never
 * imported this function and so could not see it bypass `runAudit` entirely.
 *
 * Playwright calls a global teardown with the run's config and nothing else, so
 * the seams sit behind it, where a real run always leaves them defaulted.
 */
export default async function globalTeardown(_config, audit, resolve) {
  return runAudit(audit, resolve);
}

/**
 * The audit a real run performs.
 *
 * This exists so `auditGallery` needs no default resolver. A default is a place
 * the rule can be written a second time: `resolve = (dirs) => ({ staging:
 * dirs.staging ?? staging(), gallery: dirs.gallery ?? GALLERY })` behaves
 * identically to `resolve = resolveDirs` in every test — the ones that pass a
 * resolver override it, and the one that passes nothing cannot tell a faithful
 * copy from the original, because no behavioural test ever can. So the rule is
 * spelled once and *handed over* once, and which function is handed over is a
 * question with an answer: `strictEqual` against `resolveDirs` distinguishes a
 * use of the rule from a copy of it, where behaviour cannot.
 *
 * `audit` is a parameter for the same reason: it is how that hand-over is
 * observed without running a real audit.
 */
export async function runAudit(audit = auditGallery, resolve = resolveDirs) {
  return audit({}, resolve);
}

/**
 * The directory pair a run works over: this run's, unless a caller names another.
 *
 * Exported so the offline suite can assert that an absent `dirs` still resolves
 * to the real pair. Without that, the two bindings that matter in production —
 * `staging()` and `GALLERY` — were reachable only from a 651-test browser run,
 * and swapping either for something else left the offline suite green: the seam
 * the tests use would be proved and the path a real run takes would not.
 */
export function resolveDirs(dirs = {}) {
  return { staging: dirs.staging ?? staging(), gallery: dirs.gallery ?? GALLERY };
}

/**
 * Publishes this run's renders, says what the gallery holds, and hands back the
 * findings a run may be failed for.
 *
 * Called from `specs/gallery.audit.spec.mjs` rather than only from the hook
 * above, and that indirection is the point. For as long as the audit lived in
 * the hook it produced *findings* and nothing was answerable for them: a run
 * could print `This gallery is not the whole matrix` in red, at length, and the
 * only thing carrying that news was a console line and a banner nobody is
 * gated on. Throwing from the hook does fail the run — that was measured, not
 * assumed — but it fails it as an error belonging to no test, so the reporter
 * says "1 error was not a part of any test" and the run's own record of what it
 * checked does not contain the check. In a change about making the harness's
 * marks answerable as checks, this one should be a named check. A teardown
 * *project* runs from the same vantage point, after every worker has finished,
 * and asserts with `expect`.
 *
 * The hook stays because it is still the only thing that runs when the audit
 * spec does not — `test:pr` and `test:visual` filter it out — and there it is a
 * no-op that discards a staging directory nobody published. After the spec has
 * run, staging has already been renamed over the gallery, so this second call
 * finds nothing and returns without touching a reviewer's artefact.
 *
 * `dirs` is how the offline suite runs this whole chain against a temporary
 * pair. It defaults to the real ones, so no caller in the suite passes it and
 * nothing about a run changes. Before it existed, `stamp`'s composition —
 * apply the marks, write the marked page, fold what did not land into the
 * findings — was reachable only through a 651-test browser run, and each of its
 * three links could be cut with the offline suite still green. The worst of
 * them wrote the *unmarked* page back to disk: the marks were computed
 * correctly, `unmarked` was legitimately empty, the gate passed, and the
 * artefact a reviewer opens was clean and silent. That is the exact defect this
 * file exists to remove, so it is now decided here rather than in a comment.
 *
 * `resolve` is a parameter, and deliberately has no default. Calling this with
 * no arguments proved the defaulting path was *taken*, but not that the defaults
 * came from one place: writing `{ staging: dirs.staging ?? staging(), gallery:
 * dirs.gallery ?? GALLERY }` inline here behaved identically and left every test
 * green, restoring the duplicate spelling of the rule that `resolveDirs` exists
 * to be. Giving `resolve` a default only moved that hiding place into the
 * parameter list, where a faithful copy is equally invisible. There is now
 * nowhere in this function to write the rule at all: `runAudit` hands it in, and
 * the offline suite asks which function was handed. As a seam it is answerable
 * twice over — a resolver that returns a pair unrelated to `dirs` must be the
 * pair audited, which an inline copy reading `dirs` directly cannot satisfy.
 *
 * There is exactly one way out of here without an audit, and it is that the run
 * published nothing. A second one used to exist: a run that published renders
 * but no `index.html` returned `ran: false`, on the reasoning that a narrow
 * `--grep` can leave the index out and its renders are still worth keeping. The
 * reasoning was sound and the return value was not — `ran: false` is what
 * `specs/gallery.audit.spec.mjs` skips on, so deleting the staged index from a
 * full matrix run turned a gallery with five hundred unaudited figures into a
 * *skipped* check and a green run (3 passed, 1 skipped). The absence of the
 * index was excusing the audit from noticing the absence of the index. A missing
 * index is a finding now, reported with the rest.
 */
export async function auditGallery(dirs, resolve) {
  const { staging: stageDir, gallery: outDir } = resolve(dirs);
  const { records, unreadable } = await collapseSignatures(stageDir);
  const published = await publishGallery(stageDir, outDir);
  if (!published.length) {
    // The directory is part of the answer, not decoration: for an empty staging
    // directory the whole of what this function does is resolve a path, discard
    // it and say so, and a stand-in that reports the sentence without the path
    // never had to resolve anything. Saying where it looked is also what a
    // person reading "this run rendered no states" wants to know next.
    return { ran: false, reason: "this run rendered no states, so there is no gallery to audit", incomplete: [], staging: stageDir };
  }
  console.log(`gallery: published ${published.length} file(s) to ${outDir}`);
  return { ...await report(published, records, unreadable, outDir), reason: null };
}

/**
 * Folds the per-render records into one `signatures.json` and one
 * `settled.json` inside the staging directory, so the published gallery carries
 * a diffable record of what each state actually drew — and of which of those
 * renders it is entitled to be believed about.
 *
 * The full signature a twin participant carries stays out of the published
 * directory. It is a description of every element on a page, it exists only so
 * a broken claim can name what broke, and the finding that needs it is written
 * here.
 */
async function collapseSignatures(dir) {
  const names = await readdir(join(dir, SIGNATURES)).catch(() => []);
  const records = {};
  const unreadable = [];
  for (const name of names) {
    const record = await readRecord(join(dir, SIGNATURES, name));
    if (record) {
      records[name.replace(/\.json$/u, "")] = record;
    } else {
      unreadable.push(name.replace(/\.json$/u, ""));
    }
  }
  await rm(join(dir, SIGNATURES), { recursive: true, force: true });
  if (names.length) {
    await writeFile(join(dir, "signatures.json"), `${stringify(records, (record) => record.signature)}\n`, "utf8");
    await writeFile(join(dir, "settled.json"), `${stringify(records, (record) => record.settled)}\n`, "utf8");
  }
  return { records, unreadable };
}

/**
 * One render's record, or nothing when it cannot be read.
 *
 * A worker killed mid-write leaves a truncated file, and letting that throw
 * would take the whole teardown down — losing the gallery for every render that
 * did complete, over one that did not. The name is carried out instead, so the
 * report says which render it knows nothing about rather than either crashing
 * or quietly dropping it.
 */
async function readRecord(path) {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch {
    return null;
  }
}

function stringify(records, pick) {
  return JSON.stringify(Object.fromEntries(Object.entries(records).map(([name, record]) => [name, pick(record)])), null, 1);
}

/**
 * The size and both ends of every published render, which is all `auditBytes`
 * needs to say whether a file has a whole PNG in it.
 *
 * Ends rather than whole files: five hundred screenshots is some tens of
 * megabytes, and reading all of it to look at sixteen bytes per file would put
 * a cost on the teardown that the check does not need.
 *
 * A file that cannot be opened is reported as a render of no size rather than
 * allowed to throw. It is the same news to a reviewer — there is nothing behind
 * this figure — and a throw here would take down the audit that was about to
 * say so.
 */
async function readEnds(dir, names) {
  return Promise.all(names.map(async (name) => {
    const handle = await open(join(dir, name), "r").catch(() => null);
    if (!handle) {
      return { name, size: 0 };
    }
    try {
      const { size } = await handle.stat();
      return { name, size, open: await slice(handle, size, 0), close: await slice(handle, size, size - PNG_ENDS) };
    } finally {
      await handle.close();
    }
  }));
}

async function slice(handle, size, at) {
  if (size < PNG_ENDS) {
    return [];
  }
  const buffer = Buffer.alloc(PNG_ENDS);
  await handle.read(buffer, 0, PNG_ENDS, at);
  return [...buffer];
}

/**
 * States the gallery links but does not hold, states it holds that the registry
 * does not, renders this run could not prove had stopped changing, and states
 * that draw one another's screen without saying so.
 *
 * Two halves, and only one of them is asserted. The split is not a compromise
 * between them; they are answers to two different kinds of question.
 *
 * `incomplete` is arithmetic over a file list: did this run write every render
 * the registry asked for, does every published render belong to a state, is
 * there a whole PNG behind each of those names, does the gallery carry an index
 * saying what it holds, and did every published render file a record that can be
 * read? None of that depends on comparing one render against another, so none of
 * it can drift, and a run that gets it wrong has published an artefact that lies
 * about its own extent — the banner has always said so in red, and until now
 * nothing was answerable for it. A red notice carried only by a console line is
 * the exact defect this branch exists to remove, made by the instrument that
 * reports it.
 *
 * `claims` — two states drawing one screen, or a declared twin that parted —
 * stays reported rather than asserted, and that remains a measured decision. It
 * is a comparison between two renders, the signature still drifts on about an
 * eighth of them between two runs of one tree because some content arrives
 * after the surface has stopped changing for a poll interval, and this
 * repository's rule is that a flaky gate is worse than no gate. `unchecked-twin`
 * moves across into `incomplete`, though, because it is not a comparison at all:
 * it says the run rendered one side or neither, which is the same arithmetic as
 * a missing render and is just as deterministic.
 *
 * Drift — a finding whose own words say the difference is a frame — is the
 * third list and is neither asserted nor alarmed.
 *
 * The findings are returned rather than thrown here, so the caller that owns
 * the verdict is a named check rather than a hook. The gallery is stamped
 * first either way: a failing run still publishes the artefact that says why it
 * failed, which is the one thing a review surface owes on a bad day.
 *
 * What it publishes is still a contradiction the registry could not previously
 * face: `signatures.json` is a diffable record of what each state actually
 * drew, `settled.json` of which of those records may be relied on, the index
 * says out loud when it is not the whole matrix, and every pair of states
 * drawing one screen is either declared in `RENDER_TWINS` with a reason or
 * named here as news.
 */
async function report(published, records, unreadable, outDir) {
  const expected = expectedShots(STATES, Object.values(VIEWPORTS));
  const names = auditNames(expected, published);
  const twins = auditTwins(records, RENDER_TWINS);
  const unsettled = auditSettled(records);
  const motion = auditMotion(records, movingShots(STATES, Object.values(VIEWPORTS)));
  const unaudited = auditUnaudited(expected, published, records, unreadable);
  const bytes = auditBytes(await readEnds(outDir, published.filter((name) => name.endsWith(".png"))));
  // The two ways a render ends up with no usable record read differently to
  // whoever has to fix them, and identically to whoever has to review the
  // gallery: nothing is known about the screen either way. So they are reported
  // apart and marked together.
  const unknown = [...new Set([...unreadable, ...unaudited])].sort();
  const parted = partitionFindings(twins);
  const drift = parted.drift.map((finding) => `${finding.kind}: ${finding.detail}`);
  const incomplete = [
    ...(published.includes("index.html") ? [] : ["the gallery holds renders and no index, so nothing in it says which states it claims to hold"]),
    ...(names.missing.length ? [`${names.missing.length} render(s) were never written by this run`] : []),
    ...(names.stray.length ? [`${names.stray.length} render(s) belong to no state in the registry`] : []),
    ...(bytes.empty.length ? [`${bytes.empty.length} render(s) were published with no bytes in them, so the gallery links a broken image: ${bytes.empty.slice(0, 5).join(", ")}`] : []),
    ...(bytes.malformed.length ? [`${bytes.malformed.length} render(s) are not a whole PNG, so the writer never reached the end of them: ${bytes.malformed.slice(0, 5).join(", ")}`] : []),
    ...(unreadable.length ? [`${unreadable.length} render(s) filed a record this run could not read, so nothing is known about them: ${unreadable.slice(0, 5).join(", ")}`] : []),
    ...(unaudited.length ? [`${unaudited.length} render(s) were published without a record, so nothing is known about them: ${unaudited.slice(0, 5).join(", ")}`] : []),
    ...(motion.stray.length ? [`${motion.stray.length} render(s) never stopped changing and no state declares them in motion, so their screenshots are whichever frame the run caught: ${motion.stray.slice(0, 5).join(", ")}`] : []),
    ...(motion.still.length ? [`${motion.still.length} render(s) declare themselves in motion and came to rest, so the exemption is stale: ${motion.still.slice(0, 5).join(", ")}`] : []),
    ...parted.unchecked.map((finding) => `${finding.kind}: ${finding.detail}`),
  ];
  const lines = [...incomplete, ...parted.claims.map((finding) => `${finding.kind}: ${finding.detail}`)];
  console.log(`gallery: ${Object.keys(records).length} render(s) audited`);
  if (unsettled.length) {
    console.log(`gallery: ${unsettled.length} render(s) were ${SETTLED_PHRASE} and are marked on their own figures`);
  }
  for (const line of [...lines, ...drift]) {
    console.log(`gallery: ${line}`);
  }
  if (!lines.length && !drift.length) {
    console.log("gallery: complete, and every state draws its own screen or a declared twin's");
  }
  // Stamped before the verdict is returned, so a failing run still publishes the
  // artefact that says why it failed. Reversing the two would take the evidence
  // down with the run, which is the one thing a review surface owes on a bad
  // day.
  //
  // A mark that found no anchor joins the deterministic findings: it is decided
  // by `indexOf` over a file this run wrote, not by comparing two renders, so it
  // cannot come out differently on a contended machine and belongs where the
  // audit spec can gate on it.
  const unmarked = await stamp(lines, names.missing, unsettled, drift, unknown, outDir);
  for (const line of unmarked) {
    console.log(`gallery: ${line}`);
  }
  return { ran: true, incomplete: [...incomplete, ...unmarked] };
}

/**
 * Marks the findings in the artefact itself, not only on a console, and says
 * which marks found nothing to attach to.
 *
 * The anchors it replaces belong to `gallery-index.mjs`, which is also what
 * wrote the page, so the two cannot drift apart in silence. `applyMarks`
 * returns the marks that landed nowhere; they are returned rather than swallowed
 * because an index that lost its anchors still renders — cleanly, and without
 * the alarm — which is precisely the unmarked-but-broken gallery this suite is
 * written against.
 *
 * Two notices, because they are two different sentences and only one of them is
 * an alarm. "This gallery is not the whole matrix" has to keep meaning that:
 * an unsettled render is not a hole in the gallery, and at least one is the
 * *expected* result of any full run — `transport:playing` advances on a 100ms
 * interval, so its two renders can never reach the settle window. Folding the
 * count into the red banner would have lit it on every clean run, which is a
 * gallery that cries wolf about itself in a change whose whole subject is
 * saying only what is true.
 *
 * The unsettled renders are also marked on their own figures rather than only
 * counted at the top. A reviewer scrolls to the state they care about; a number
 * on a five-hundred-state page does not travel with them, and the whole point
 * of the mark is that it is attached to the screen being judged.
 * `outDir` is required rather than defaulted. A default here is not a
 * convenience: it turns a caller that forgot to pass one — an offline test
 * working over a temporary directory, say — from a loud failure into a silent
 * rewrite of the real published gallery, which then carries a red banner about
 * a run that never touched it. An artefact that lies about its own extent is
 * the one thing this file exists to prevent.
 */
async function stamp(lines, missing, unsettled, drift, unknown, outDir) {
  const index = join(outDir, "index.html");
  const written = await readFile(index, "utf8").catch(() => null);
  if (!written) {
    return ["index.html could not be read, so none of this run's findings are marked on it"];
  }
  const notice = notices(lines, missing, unsettled, drift, unknown);
  const marked = applyMarks(written, notice, [...unsettled, ...unknown]);
  await writeFile(index, marked.html, "utf8");
  return marked.unmarked.length
    ? [`${marked.unmarked.length} of this run's marks found no anchor in the gallery index, so the artefact understates what was found: ${marked.unmarked.slice(0, 5).join(", ")}`]
    : [];
}

/**
 * The notices that go above the gallery, as HTML.
 *
 * Pure, so the rule that an unsettled render never raises the alarm is decided
 * here and held by the offline suite rather than by reading a written file.
 *
 * Two notices, because they are two different sentences and only one of them is
 * an alarm. "This gallery is not the whole matrix" has to keep meaning that:
 * an unsettled render is not a hole in the gallery, and at least one is the
 * *expected* result of any full run — `transport:playing` advances on a 100ms
 * interval, so its two renders can never reach the settle window. Folding the
 * count into the red banner would have lit it on every clean run, which is a
 * gallery that cries wolf about itself in a change whose whole subject is
 * saying only what is true.
 *
 * `drift` is the rest of that same sentence, and leaving it out was the hole in
 * the first version of this rule. The count was kept from the alarm but the
 * findings *about* those renders were not: an `unproven-twin` went into `lines`
 * with everything else, so the banner announced "not every state in it draws its
 * own screen" above a finding whose own words say the difference is a frame.
 * The two halves are separated by kind, in `isDrift`, so a finding cannot reach
 * the alarm by being added to the wrong list here.
 *
 * All three lists are capped. A run in which every state drew one screen
 * produces a finding per group and a missing entry per render, and a banner
 * carrying all of them unabridged is megabytes of text at the top of the page a
 * reviewer came to read — the run where the gallery most needs to stay legible
 * is exactly the run that would make it unreadable.
 *
 * `unknown` renders are marked on their figures with the same attribute, so the
 * count here is the number of marks a reviewer will actually meet. They are a
 * different sentence from an unsettled render — one was still moving, the other
 * has no record this run could read — so the opening line says only what is
 * true of both, and the difference is named in its own clause.
 *
 * Every dynamic line is escaped. What goes in here is not a literal: findings
 * carry state ids, filenames and — for a twin difference — a DOM signature
 * quoted back off a rendered page. Written raw, a finding that merely *mentions*
 * markup stops being reported text and becomes markup, so the one artefact a
 * reviewer reads to learn a run went wrong is the artefact the finding corrupts.
 * The counts are `.length` values and cannot carry any.
 */
export function notices(lines, missing, unsettled, drift = [], unknown = []) {
  const shown = lines.slice(0, 20);
  const marked = unsettled.length + unknown.length;
  const alarm = lines.length
    ? '<p style="margin:0;padding:.75rem 1.5rem;background:#ffebe9;color:#cf222e;font-weight:700">'
      + `This gallery is not the whole matrix, or not every state in it draws its own screen: ${shown.map(escape).join("; ")}`
      + `${lines.length > shown.length ? `; and ${lines.length - shown.length} more` : ""}. `
      + `${missing.length ? `Missing: ${missing.slice(0, 20).map(escape).join(", ")}${missing.length > 20 ? ", …" : ""}` : ""}</p>`
    : "";
  const note = marked || drift.length
    ? `<p style="margin:0;padding:.75rem 1.5rem;background:#fff8c5;color:${SETTLED_INK}">`
      + `${marked} render(s) below are marked <strong>${SETTLED_PHRASE}</strong>: this run could not vouch for them, `
      + "so read those as a frame rather than as a screen. A state that animates by design is expected here."
      + `${unknown.length ? ` ${unknown.length} of them filed no record this run could read, so nothing at all is known about those.` : ""}`
      + `${drift.length ? ` ${drift.slice(0, 20).map(escape).join("; ")}${drift.length > 20 ? `; and ${drift.length - 20} more` : ""}.` : ""}</p>`
    : "";
  return `${alarm}${note}`;
}
