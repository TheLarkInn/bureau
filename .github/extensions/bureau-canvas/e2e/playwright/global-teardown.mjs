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
import { auditNames, auditSettled, auditTwins, auditUnaudited, expectedShots, partitionFindings } from "./gallery-audit.mjs";
import { publishGallery } from "./gallery.mjs";
import { GALLERY, staging } from "./gallery-paths.mjs";

const SIGNATURES = "signatures";

export default async function globalTeardown() {
  await auditGallery();
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
 */
export async function auditGallery() {
  const { records, unreadable } = await collapseSignatures(staging());
  const published = await publishGallery(staging(), GALLERY);
  if (!published.length) {
    return { ran: false, reason: "this run rendered no states, so there is no gallery to audit", incomplete: [] };
  }
  console.log(`gallery: published ${published.length} file(s) to ${GALLERY}`);
  // A full matrix run always writes the index, including a failing one — it is
  // an ordinary test and nothing short-circuits the run. A narrower `--grep`
  // can leave it out, and the renders are still worth keeping, so this says so
  // rather than withholding them.
  if (!published.includes("index.html")) {
    console.log(`gallery: this run rendered no index; browse the files directly under ${GALLERY}`);
    return { ran: false, reason: "this run rendered states but no index, so the gallery is not a matrix to audit", incomplete: [] };
  }
  return { ...await report(published, records, unreadable), reason: null };
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
 * States the gallery links but does not hold, states it holds that the registry
 * does not, renders this run could not prove had stopped changing, and states
 * that draw one another's screen without saying so.
 *
 * Two halves, and only one of them is asserted. The split is not a compromise
 * between them; they are answers to two different kinds of question.
 *
 * `incomplete` is arithmetic over a file list: did this run write every render
 * the registry asked for, does every published render belong to a state, and
 * did every published render file a record that can be read? None of that
 * depends on comparing one render against another, so none of it can drift, and
 * a run that gets it wrong has published an artefact that lies about its own
 * extent — the banner has always said so in red, and until now nothing was
 * answerable for it. A red notice carried only by a console line is the exact
 * defect this branch exists to remove, made by the instrument that reports it.
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
async function report(published, records, unreadable) {
  const expected = expectedShots(STATES, Object.values(VIEWPORTS));
  const names = auditNames(expected, published);
  const twins = auditTwins(records, RENDER_TWINS);
  const unsettled = auditSettled(records);
  const unaudited = auditUnaudited(expected, published, records, unreadable);
  // The two ways a render ends up with no usable record read differently to
  // whoever has to fix them, and identically to whoever has to review the
  // gallery: nothing is known about the screen either way. So they are reported
  // apart and marked together.
  const unknown = [...new Set([...unreadable, ...unaudited])].sort();
  const parted = partitionFindings(twins);
  const drift = parted.drift.map((finding) => `${finding.kind}: ${finding.detail}`);
  const incomplete = [
    ...(names.missing.length ? [`${names.missing.length} render(s) were never written by this run`] : []),
    ...(names.stray.length ? [`${names.stray.length} render(s) belong to no state in the registry`] : []),
    ...(unreadable.length ? [`${unreadable.length} render(s) filed a record this run could not read, so nothing is known about them: ${unreadable.slice(0, 5).join(", ")}`] : []),
    ...(unaudited.length ? [`${unaudited.length} render(s) were published without a record, so nothing is known about them: ${unaudited.slice(0, 5).join(", ")}`] : []),
    ...parted.unchecked.map((finding) => `${finding.kind}: ${finding.detail}`),
  ];
  const lines = [...incomplete, ...parted.claims.map((finding) => `${finding.kind}: ${finding.detail}`)];
  console.log(`gallery: ${Object.keys(records).length} render(s) audited`);
  if (unsettled.length) {
    console.log(`gallery: ${unsettled.length} render(s) were not proved settled and are marked on their own figures`);
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
  await stamp(lines, names.missing, unsettled, drift, unknown);
  return { ran: true, incomplete };
}

/**
 * Marks the findings in the artefact itself, not only on a console.
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
 */
async function stamp(lines, missing, unsettled, drift, unknown = []) {
  const index = join(GALLERY, "index.html");
  const written = await readFile(index, "utf8").catch(() => null);
  if (!written) {
    return;
  }
  const page = written.replace("<main>", `${notices(lines, missing, unsettled, drift, unknown)}<main>`);
  await writeFile(index, markUnsettled(page, [...unsettled, ...unknown]), "utf8");
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
 */
export function notices(lines, missing, unsettled, drift = [], unknown = []) {
  const shown = lines.slice(0, 20);
  const marked = unsettled.length + unknown.length;
  const alarm = lines.length
    ? '<p style="margin:0;padding:.75rem 1.5rem;background:#ffebe9;color:#cf222e;font-weight:700">'
      + `This gallery is not the whole matrix, or not every state in it draws its own screen: ${shown.join("; ")}`
      + `${lines.length > shown.length ? `; and ${lines.length - shown.length} more` : ""}. `
      + `${missing.length ? `Missing: ${missing.slice(0, 20).join(", ")}${missing.length > 20 ? ", …" : ""}` : ""}</p>`
    : "";
  const note = marked || drift.length
    ? '<p style="margin:0;padding:.75rem 1.5rem;background:#fff8c5;color:#9a6700">'
      + `${marked} render(s) below are marked <strong>not proved settled</strong>: this run could not vouch for them, `
      + "so read those as a frame rather than as a screen. A state that animates by design is expected here."
      + `${unknown.length ? ` ${unknown.length} of them filed no record this run could read, so nothing at all is known about those.` : ""}`
      + `${drift.length ? ` ${drift.slice(0, 20).join("; ")}${drift.length > 20 ? `; and ${drift.length - 20} more` : ""}.` : ""}</p>`
    : "";
  return `${alarm}${note}`;
}

/** Tags every unsettled render's figure, which the index's own CSS then draws. */
export function markUnsettled(html, unsettled) {
  return unsettled.reduce(
    (page, shot) => page.replace(`<figure data-shot="${shot}">`, `<figure data-shot="${shot}" data-settled="false">`),
    html,
  );
}
