// The gallery's two claims, and what contradicts each.
//
// A published gallery says two things a reviewer relies on: that it holds a
// render for every state the registry has, and that the states it separates
// really are separate screens. Neither could previously be false out loud —
// the index links a figure per state whether or not the file behind it exists,
// and nothing ever compared two renders to each other.
//
// Pure, so it is decided here rather than by a browser: the audit takes a file
// list and a signature map and returns findings.

import assert from "node:assert/strict";
import test from "node:test";
import { crc32 } from "node:zlib";

import { auditBytes, auditMotion, auditNames, auditSettled, auditTwins, auditUnaudited, expectedShots, isDrift, movingShots, partitionFindings, PNG_HEAD, PNG_TAIL, shotName, walkChunks } from "../e2e/playwright/gallery-audit.mjs";
import { notices } from "../e2e/playwright/global-teardown.mjs";
import { applyMarks, escape, figurePrefix, figureTag, indexPage, markTag, NOTICE_ANCHOR, rowsFor, SETTLED_INK, SETTLED_MARK, SETTLED_SLOT } from "../e2e/playwright/gallery-index.mjs";
import { STATES as REGISTRY_STATES } from "../web/statelab/registry.mjs";
import { VIEWPORTS as REAL_VIEWPORTS } from "../web/statelab/selectors.mjs";

const VIEWPORTS = [{ id: "desktop" }, { id: "compact" }];
const STATES = [{ id: "surface:config+card:expanded" }, { id: "probe--draft-bar" }];

/** One real 1×1 PNG, as a genuine encoder wrote it. */
const REAL_PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/**
 * A published file as the teardown reads it: its size, both ends and the walked
 * chunk stream, with everything but the size withheld when the file is too small
 * to have two ends that do not overlap. Mirrors `readEnds` so the audit is
 * tested on the shape it is really given rather than on a hand-written one that
 * cannot be wrong.
 */
function ends(name, bytes) {
  return bytes.length < PNG_HEAD + PNG_TAIL
    ? { name, size: bytes.length }
    : { name, size: bytes.length, open: bytes.slice(0, PNG_HEAD), close: bytes.slice(-PNG_TAIL), chunks: walkChunks(bytes) };
}

/**
 * One record per render, which is the shape the audit reads: a digest of what
 * the render drew, whether it was proved to have stopped changing, and — for
 * the handful of renders a declared twin names — the signature itself.
 */
function drew(signatures, settled = {}, details = {}) {
  return Object.fromEntries(Object.entries(signatures).map(([name, signature]) => [
    name,
    {
      signature,
      ...(name in settled ? { settled: settled[name] } : {}),
      ...(name in details ? { detail: details[name] } : {}),
    },
  ]));
}

test("a state's render is named the same way by the registry and the audit", () => {
  assert.deepEqual(
    expectedShots(STATES, VIEWPORTS),
    [
      "desktop--surface_config_card_expanded.png",
      "compact--surface_config_card_expanded.png",
      "desktop--probe_draft_bar.png",
      "compact--probe_draft_bar.png",
    ],
  );
});

/**
 * The two ways a gallery and a registry disagree about which states exist. They
 * are reported apart because they mean opposite things: a missing render is a
 * state the run never drew, and a stray one is a screen the product no longer
 * has being offered for review.
 */
test("the audit names both the renders a run never wrote and the ones no state claims", () => {
  const expected = expectedShots(STATES, VIEWPORTS);
  const present = [...expected.slice(0, 3), "desktop--surface_gone.png", "index.html", "signatures.json"];

  assert.deepEqual(auditNames(expected, present), {
    missing: ["compact--probe_draft_bar.png"],
    stray: ["desktop--surface_gone.png"],
  });
});

test("a gallery holding exactly its registry's renders has nothing to report", () => {
  const expected = expectedShots(STATES, VIEWPORTS);

  assert.deepEqual(auditNames(expected, [...expected, "index.html"]), { missing: [], stray: [] });
});

const A = shotName("probe--refusal-dismissed", "desktop");
const B = shotName("surface:config+disclosure:create", "desktop");
const C = shotName("surface:config+card:expanded", "desktop");

test("two states drawing one screen are reported unless the registry says why", () => {
  const same = { [A]: "digest-1", [B]: "digest-1", [C]: "digest-2" };
  const twin = { a: "probe--refusal-dismissed", b: "surface:config+disclosure:create", viewports: ["desktop"], why: "the refusal left with the create it was about" };

  assert.deepEqual(
    [auditTwins(drew(same), []).map((finding) => finding.kind), auditTwins(drew(same), [twin])],
    [["undeclared-twin"], []],
  );
});

/**
 * A declaration is a claim, so it fails in both directions. A twin that stops
 * matching means either the screen regressed or the reason was never true, and
 * either way the sentence in the registry has to be re-read — so it is reported
 * exactly as loudly as an undeclared pair.
 */
test("a declared twin that no longer draws one screen is reported too", () => {
  const parted = { [A]: "digest-1", [B]: "digest-9" };
  const twin = { a: "probe--refusal-dismissed", b: "surface:config+disclosure:create", viewports: ["desktop"], why: "the refusal left with the create it was about" };

  const findings = auditTwins(drew(parted), [twin]);

  assert.deepEqual(
    [findings.map((finding) => finding.kind), findings[0].detail.includes("the refusal left with the create")],
    [["broken-twin"], true],
  );
});

/**
 * The two layouts are two screens. A pair declared at one viewport says nothing
 * about the other, so a twin that starts holding at the viewport it was never
 * declared for is still news.
 */
test("a twin declared at one viewport does not excuse the other", () => {
  const compactA = shotName("probe--refusal-dismissed", "compact");
  const compactB = shotName("surface:config+disclosure:create", "compact");
  const same = { [A]: "digest-1", [B]: "digest-1", [compactA]: "digest-3", [compactB]: "digest-3" };
  const twin = { a: "probe--refusal-dismissed", b: "surface:config+disclosure:create", viewports: ["desktop"], why: "the refusal left with the create it was about" };

  assert.deepEqual(auditTwins(drew(same), [twin]).map((finding) => finding.detail.includes("compact--")), [true]);
});

/**
 * One state's two renders are the same DOM at two widths, on purpose — the
 * signature is geometry-free, so that is what "the layout differs" looks like
 * to it. Comparing across viewports therefore reports every state that does not
 * reflow as a coincidence, which is most of them.
 */
test("one state rendered at both viewports is not a twin of itself", () => {
  const both = { [A]: "digest-1", [shotName("probe--refusal-dismissed", "compact")]: "digest-1" };

  assert.deepEqual(auditTwins(drew(both), []), []);
});

/**
 * A twin the run did not render is not a twin that held.
 *
 * `broken` only ever spoke when both renders were present, so a partial run — a
 * `--grep`, a shard, an early bail — returned clean for claims it never
 * compared. That is the audit saying "checked" about work it did not do, which
 * is the one thing a review surface may not do, and it is the same defect in
 * miniature as an index that links figures it does not hold.
 */
test("a twin this run did not render is reported as unchecked, not as holding", () => {
  const twin = { a: "probe--refusal-dismissed", b: "surface:config+disclosure:create", viewports: ["desktop"], why: "the refusal left with the create it was about" };

  assert.deepEqual(
    [
      auditTwins(drew({ [A]: "digest-1" }), [twin]).map((finding) => finding.kind),
      auditTwins({}, [twin]).map((finding) => finding.kind),
      auditTwins(drew({ [A]: "digest-1", [B]: "digest-1" }), [twin]).map((finding) => finding.kind),
    ],
    [["unchecked-twin"], ["unchecked-twin"], []],
  );
});

/**
 * A group of identical renders is one finding, not one per pair.
 *
 * The run that makes this matter is the catastrophic one — every state drawing
 * a single "Loading…" screen — where per-pair reporting is quadratic and the
 * banner built from it is megabytes at the top of the page a reviewer came to
 * read. The gallery has to stay legible in exactly the run that broke it.
 */
test("a group of renders drawing one screen is reported once, with a count", () => {
  const D = shotName("surface:config+section:empty", "desktop");
  const crowd = { [A]: "same", [B]: "same", [C]: "same", [D]: "same" };

  const findings = auditTwins(drew(crowd), []);

  assert.deepEqual(
    [findings.length, findings[0].kind, findings[0].detail.includes("4 renders draw the same screen")],
    [1, "undeclared-twin", true],
  );
});

/**
 * Two state ids that differ only in punctuation must not name one file.
 *
 * `shotName` folds every run of non-alphanumerics to `_`, which is not
 * injective. A collision is the one failure the completeness check cannot
 * report: the second render silently overwrites the first, and `expectedShots`
 * collapses the same way, so `auditNames` calls the gallery complete while a
 * state a reviewer believes they reviewed was never on the page.
 */
test("every state in the registry names its own render file", () => {
  const shots = expectedShots(REGISTRY_STATES, Object.values(REAL_VIEWPORTS));

  assert.deepEqual(
    [shots.length, new Set(shots).size],
    [REGISTRY_STATES.length * Object.values(REAL_VIEWPORTS).length, shots.length],
  );
});

const TWIN = {
  a: "probe--refusal-dismissed",
  b: "surface:config+disclosure:create",
  viewports: ["desktop"],
  why: "the refusal left with the create it was about",
};

/**
 * A render whose DOM never stopped changing is not a screen, and the audit may
 * not report a difference between one of those and anything else as news about
 * the product.
 *
 * This is not hypothetical. Three of this tree's declared twins reported broken
 * on a fully-parallel matrix run and matched exactly when the same six states
 * were rendered one at a time — the harness's own contention, published in the
 * gallery banner as a claim that the UI had regressed. A reviewer acting on it
 * would go looking for a difference that is not there, which is worse than the
 * audit saying nothing: it spends the one thing a review surface trades in.
 */
test("a twin that differs on a render nothing could settle is unproven, not broken", () => {
  const parted = { [A]: "digest-1", [B]: "digest-9" };

  assert.deepEqual(
    [
      auditTwins(drew(parted, { [A]: true, [B]: false }), [TWIN]).map((finding) => finding.kind),
      auditTwins(drew(parted, { [A]: true, [B]: true }), [TWIN]).map((finding) => finding.kind),
    ],
    [["unproven-twin"], ["broken-twin"]],
  );
});

/** The unproven finding names which side was never proved, so it is actionable. */
test("an unproven twin names the render that never settled", () => {
  const parted = drew({ [A]: "digest-1", [B]: "digest-9" }, { [A]: false, [B]: false });

  const findings = auditTwins(parted, [TWIN]);

  assert.deepEqual(
    [findings.length, findings[0].detail.includes(A), findings[0].detail.includes(B)],
    [1, true, true],
  );
});

/**
 * A settled pair that matches is still no finding, and an artefact from a run
 * that filed no settle-proof reads exactly as it did before: absence of a
 * record is not evidence of doubt, or every gallery written before this rule
 * existed would re-read as unreliable.
 */
test("settle-proof changes nothing about a twin that holds, or one with no record", () => {
  const same = { [A]: "digest-1", [B]: "digest-1" };
  const parted = { [A]: "digest-1", [B]: "digest-9" };

  assert.deepEqual(
    [
      auditTwins(drew(same, { [A]: false, [B]: false }), [TWIN]),
      auditTwins(drew(parted), [TWIN]).map((finding) => finding.kind),
    ],
    [[], ["broken-twin"]],
  );
});

/**
 * The rule read in the other direction, which was missing.
 *
 * A pair that *matches* is evidence of sameness only when both sides were
 * proved, exactly as a pair that differs is evidence of a difference only then.
 * A render captured a beat early is missing whatever had not arrived yet, so
 * two states each missing the same late region collide on a signature neither
 * will still have a moment later — and the audit told a reviewer to go and
 * declare a twin between two states that do not draw the same screen at all.
 */
test("an undeclared match on a render nothing could settle is unproven, not a twin", () => {
  const same = { [A]: "digest-1", [B]: "digest-1" };

  assert.deepEqual(
    [
      auditTwins(drew(same, { [A]: true, [B]: false }), []).map((finding) => finding.kind),
      auditTwins(drew(same, { [A]: true, [B]: true }), []).map((finding) => finding.kind),
    ],
    [["unproven-match"], ["undeclared-twin"]],
  );
});

/**
 * Which findings are news about the product, and which are news about this
 * harness. Named by kind rather than by which list a caller put them in, so a
 * finding cannot reach the alarm by being appended to the wrong array.
 */
test("the drift findings are exactly the unproven ones", () => {
  const kinds = ["broken-twin", "unproven-twin", "undeclared-twin", "unproven-match", "unchecked-twin"];

  assert.deepEqual(kinds.filter((kind) => isDrift({ kind })), ["unproven-twin", "unproven-match"]);
});

/**
 * The alarm's headline is a claim, and drift may not make it.
 *
 * "Not every state in it draws its own screen" asserts a defect in the UI. An
 * unproven finding's own words say the opposite — that the difference is a
 * frame rather than a finding — so routing one through the red banner prints a
 * headline contradicted by the sentence underneath it, at the top of the page a
 * reviewer came to trust. Keeping the *count* out of the alarm while letting
 * the *findings* in was the hole in the first version of this rule, so the
 * partition is asserted here rather than assumed.
 */
test("a drift finding is noted in amber and never raises the alarm banner", () => {
  const drift = ["unproven-twin: a and b are declared to draw the same screen and differ"];

  const only = notices([], [], [B], drift);

  assert.deepEqual(
    [only.includes("not the whole matrix"), only.includes("unproven-twin"), only.includes("not proved settled")],
    [false, true, true],
  );
});

/**
 * A group is not all one claim.
 *
 * One unsettled render joining a signature group used to take the group's whole
 * news with it. Here `a` never settled and `b` and `c` both did, all three
 * collide, and `b`/`c` is undeclared — a finding this harness owes a reviewer.
 * Answering it only with "a never stopped changing" drops a defect on the
 * floor, so both sentences are said.
 */
test("an unsettled render does not silence a proved undeclared pair beside it", () => {
  const same = { [A]: "digest-1", [B]: "digest-1", [C]: "digest-1" };

  const kinds = auditTwins(drew(same, { [A]: false, [B]: true, [C]: true }), []).map((finding) => finding.kind);

  assert.deepEqual(kinds.sort(), ["undeclared-twin", "unproven-match"]);
});

test("the audit names every render this run could not prove had settled", () => {
  assert.deepEqual(auditSettled(drew({ [B]: "s", [A]: "s", [C]: "s" }, { [B]: true, [A]: false, [C]: false })), [A, C].sort());
});

/**
 * A render with no record is one nothing in this audit can see.
 *
 * The screenshot is written before the record, so a worker killed between the
 * two leaves a PNG and no record — the same accident `unreadable` already
 * exists for, one instruction earlier. But `unreadable` only sees a record that
 * exists and will not parse. A record never written is absent from `records`,
 * and therefore from `auditSettled`, from the twin groups, and from every mark
 * on the page: the reviewer meets a figure captioned exactly like a proved one,
 * for a render this run cannot say anything at all about.
 *
 * Partitioned against the other two answers rather than overlapping them: a
 * render the run never published is `missing`, one belonging to no state is
 * `stray`, and only a published, expected render with no record is unaudited.
 */
test("a render published without a record is named rather than believed", () => {
  const expected = [A, B, C];
  const published = [A, B, "index.html"];

  assert.deepEqual(
    [
      auditUnaudited(expected, published, drew({ [A]: "s" }, { [A]: true })),
      auditUnaudited(expected, published, drew({ [A]: "s", [B]: "s" }, { [A]: true, [B]: true })),
    ],
    [[B], []],
  );
});

/**
 * A render is answered once, not twice in two voices.
 *
 * A record that exists and will not parse is already reported as unreadable and
 * is absent from `records` — so the no-record rule saw it too, and the same
 * render was told both "this run could not read its record" and "this was
 * published without a record". A partition that contradicts itself is worse
 * than either sentence alone.
 */
test("an unreadable record is not also reported as no record at all", () => {
  assert.deepEqual(auditUnaudited([A, B], [A, B], drew({ [A]: "s" }, { [A]: true }), [B]), []);
});

/**
 * The mark has to be a mark. A render with no usable record carries the same
 * attribute an unsettled one does — it is the same instruction to the reader,
 * "do not read this as evidence" — and the amber note counts it, so the number
 * at the top matches the number of marks below it.
 */
test("a render with no usable record is marked on its figure and counted in the note", () => {
  const note = notices([`1 render(s) were published without a record, so nothing is known about them: ${B}`], [], [A], [], [B]);

  assert.deepEqual(
    [note.includes("2 render(s) below are marked"), note.includes("filed no record this run could read"), applyMarks(figureTag(B), "", [B]).html],
    [true, true, `<figure data-shot="${B}" data-settled="false">`],
  );
});

/**
 * A broken claim has to say what broke.
 *
 * "These two no longer draw the same screen" leaves a reviewer with two
 * screenshots and a hash, and the difference the audit is reporting is often a
 * single attribute on a single element — exactly the thing a person cannot find
 * by eye and a diff finds instantly. So the renders a twin names carry their
 * signature, and the finding quotes the first line the two disagree on.
 */
test("a broken twin names the element the two renders disagree on", () => {
  const one = "DIV class=app-shell\nBUTTON data-testid=create-submit,disabled=\nP class=note";
  const other = "DIV class=app-shell\nBUTTON data-testid=create-submit\nP class=note";
  const parted = drew({ [A]: "digest-1", [B]: "digest-9" }, {}, { [A]: one, [B]: other });

  const findings = auditTwins(parted, [TWIN]);

  assert.deepEqual(
    [findings[0].kind, findings[0].detail.includes("first difference at element 2 of 3"), findings[0].detail.includes("disabled=")],
    ["broken-twin", true, true],
  );
});

/**
 * Only twin participants carry a signature, and a run that filed none must not
 * grow an invented explanation. The finding still stands; it simply stops at
 * what it knows.
 */
test("a broken twin with no signature filed reports the break and no difference", () => {
  const findings = auditTwins(drew({ [A]: "digest-1", [B]: "digest-9" }), [TWIN]);

  assert.deepEqual(
    [findings[0].kind, findings[0].detail.includes("first difference")],
    ["broken-twin", false],
  );
});

/**
 * The mark goes on the figure, not only in the banner.
 *
 * A reviewer scrolls to the state they care about, and a count at the top of a
 * five-hundred-state page does not travel with them. The whole value of knowing
 * a render was not settled is that it is attached to the screen being judged —
 * so the tag lands on that figure and on no other.
 */
test("marking tags the unsettled figure and leaves its neighbours alone", () => {
  const html = `${figureTag(A)}<img src="./${A}"></figure>${figureTag(B)}<img src="./${B}"></figure>`;

  assert.equal(
    applyMarks(html, "", [A]).html,
    `<figure data-shot="${A}" data-settled="false"><img src="./${A}"></figure>${figureTag(B)}<img src="./${B}"></figure>`,
  );
});

/**
 * An unsettled render is not a hole in the gallery, and must not raise the
 * alarm that says it is.
 *
 * The red banner asserts one specific thing — "this gallery is not the whole
 * matrix, or not every state in it draws its own screen" — and at least one
 * unsettled render is the *expected* result of every full run, because
 * `transport:playing` advances on a 100ms interval and can never reach the
 * settle window. Counting it as a finding would light the alarm on every clean
 * run, which is a review surface that cries wolf about itself: after the second
 * time, the banner that means a state was never rendered is read as background.
 */
test("an unsettled render is noted without raising the alarm banner", () => {
  const only = notices([], [], [A, B]);

  assert.deepEqual(
    [only.includes("not the whole matrix"), only.includes("not proved settled"), notices([], [], [])],
    [false, true, ""],
  );
});

/** A real finding still raises it, and carries the note alongside rather than instead. */
test("a gallery that is missing renders raises the alarm, unsettled or not", () => {
  const both = notices(["1 render(s) were never written by this run"], [A], [B]);

  assert.deepEqual(
    [both.includes("not the whole matrix"), both.includes(A), both.includes("not proved settled")],
    [true, true, true],
  );
});

/**
 * Which findings may fail a run, decided by what they are computed from.
 *
 * The audit produced findings and gated on none of them: a run could print
 * `This gallery is not the whole matrix` in red and still exit 0, which is the
 * same defect this branch exists to remove — an amber mark standing in for a
 * check that found something — made by the instrument that reports it. Gating
 * on everything was not the answer either, because a comparison between two
 * renders still drifts and a flaky gate is worse than no gate here.
 *
 * So the line is drawn at arithmetic. `unchecked-twin` says the run rendered
 * one side or neither, which is a fact about a file list and cannot come out
 * differently on a loaded machine, so it gates alongside a missing render.
 * `broken-twin` and `undeclared-twin` are comparisons and stay advisory.
 *
 * `total` is asserted so the partition stays total: a kind added later cannot
 * fall out of all three buckets and quietly stop being reported at all.
 */
test("only the findings that are arithmetic over the file list may gate a run", () => {
  const findings = [
    { kind: "unchecked-twin", detail: "neither side rendered" },
    { kind: "broken-twin", detail: "declared and parted" },
    { kind: "undeclared-twin", detail: "two states, one screen" },
    { kind: "unproven-twin", detail: "parted, but one side never settled" },
    { kind: "unproven-match", detail: "matched, but one side never settled" },
  ];
  const parted = partitionFindings(findings);

  assert.deepEqual(
    {
      unchecked: parted.unchecked.map((finding) => finding.kind),
      claims: parted.claims.map((finding) => finding.kind),
      drift: parted.drift.map((finding) => finding.kind),
      total: parted.unchecked.length + parted.claims.length + parted.drift.length,
    },
    {
      unchecked: ["unchecked-twin"],
      claims: ["broken-twin", "undeclared-twin"],
      drift: ["unproven-twin", "unproven-match"],
      total: findings.length,
    },
  );
});

/**
 * The marks are held against the page the suite actually writes.
 *
 * Every other test here hands `applyMarks` a hand-written `<figure …>` literal,
 * which proves the replacement works on that literal and nothing about whether
 * the index still contains one. While the template was private to
 * `state-matrix.spec.mjs`, that was the whole coverage: an attribute added to
 * `<figure>`, or a `<main>` that gained one, turned both marks into silent
 * no-ops — the red banner and every amber figure would stop appearing and no
 * check would notice, because an unmarked gallery renders exactly like a clean
 * one. So the anchors now belong to `gallery-index.mjs`, and this asks the real
 * template for a real page and requires both marks to land in it.
 */
test("both of the gallery's marks land in the page the suite really writes", () => {
  const shot = (state, viewport) => shotName(state.id, viewport.id);
  const page = indexPage(rowsFor(STATES, VIEWPORTS, shot), STATES, VIEWPORTS);
  const target = shot(STATES[0], VIEWPORTS[0]);

  const marked = applyMarks(page, notices(["a hole"], [], [target], [], []), [target]);

  assert.deepEqual(
    [marked.unmarked, marked.html.includes("This gallery is not the whole matrix"), marked.html.includes(`data-shot="${target}" data-settled="false"`), page.includes(NOTICE_ANCHOR)],
    [[], true, true, true],
  );
});

/**
 * A mark that finds no anchor is news, not a shrug.
 *
 * This is the failure the extraction above is guarding against, asserted from
 * the other side: given a page whose anchors have drifted, `applyMarks` must
 * name what it could not attach rather than returning the page unchanged and
 * letting the run look clean. `stamp` folds this list into the audit's
 * deterministic findings, so the gate fires.
 *
 * The drift is an attribute inserted *before* `data-shot`. Appending one after
 * it is deliberately not a drift — the marker anchors on the tag's opening and
 * stamps whatever it finds, which is what lets the test below hand it a tag
 * carrying attributes this module never wrote.
 */
test("a mark with nothing to attach to is reported rather than dropped", () => {
  const drifted = `<main class="grid">${figureTag(A).replace("<figure ", '<figure class="shot" ')}</figure></main>`;

  const marked = applyMarks(drifted, notices(["a hole"], [], [A], [], []), [A]);

  assert.deepEqual(
    [marked.unmarked, marked.html === drifted],
    [["notices", A], true],
  );
});

/**
 * The notices carry text quoted back from a rendered page, and a signature line
 * can contain anything an attribute value can. `String.prototype.replace` with a
 * string pattern expands `$&`, `` $` ``, `$'` and `$n` *in the replacement*, so
 * a banner quoting one of those would have been corrupted by its own insertion.
 */
test("a notice containing a dollar pattern is inserted literally", () => {
  const page = indexPage(rowsFor(STATES, VIEWPORTS, (state, viewport) => shotName(state.id, viewport.id)), STATES, VIEWPORTS);

  const marked = applyMarks(page, "<p>broken-twin: attr=$& and $` and $'</p>", []);

  assert.deepEqual(
    [marked.unmarked, marked.html.includes("attr=$& and $` and $'")],
    [[], true],
  );
});

/**
 * …and a finding that mentions markup is reported, not obeyed.
 *
 * The three dynamic lists a banner interpolates are not literals: findings carry
 * state ids and filenames, and a drift finding carries a DOM signature quoted
 * straight off a rendered page. Written raw, a finding that merely *names* an
 * element becomes one — so the single artefact a reviewer opens to learn that a
 * run went wrong is the artefact that finding quietly rewrites, which is this
 * file's own subject turned on the notice that reports it.
 */
test("a finding that mentions markup is written as text, not as markup", () => {
  const hostile = "<img src=x onerror=boom>";

  const banner = notices([`broken-twin: ${hostile}`], [hostile], [], [`drift: ${hostile}`], []);

  assert.deepEqual(
    [banner.includes(hostile), banner.split(escape(hostile)).length - 1, banner.includes("<img")],
    [false, 3, false],
  );
});

/**
 * A mark that lands and is not drawn is not a mark.
 *
 * Stamping the figure is only half of the amber mark; the other half is the
 * stylesheet selecting what was stamped. Those were separate spellings of one
 * attribute, so renaming the selector alone left every mark landing correctly,
 * `unmarked` empty and the gate green — with not one mark visible on the page a
 * reviewer opens. It is the same defect as an anchor that drifted, one layer
 * further down: the attachment was held and the *rendering* of it was not.
 *
 * So the attribute is read back out of a stamped figure and the page is
 * required to draw exactly that, rather than both being compared against a
 * third spelling written here — which is the shape of agreement that failed.
 *
 * The caption half is read through `SETTLED_SLOT` for the same reason: the
 * phrase is written into an element of that class, and a sheet that stopped
 * selecting it would leave the mark landing on a figure and drawn on nothing.
 */
test("the attribute a figure is stamped with is the one the page's styling draws", () => {
  const page = indexPage(rowsFor(STATES, VIEWPORTS, (state, viewport) => shotName(state.id, viewport.id)), STATES, VIEWPORTS);
  const target = shotName(STATES[0].id, VIEWPORTS[0].id);
  const literal = target.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

  const marked = applyMarks(page, "", [target]);
  const stamped = new RegExp(`<figure data-shot="${literal}"\\s+([^>]+)>`, "u").exec(marked.html)?.[1];

  assert.deepEqual(
    [typeof stamped, page.includes(`figure[${stamped}] img`), page.includes(`figure[${stamped}] figcaption .${SETTLED_SLOT}::after`)],
    ["string", true, true],
  );
});
/**
 * …and the ink it is drawn in is the ink the browser check hunts for.
 *
 * `@matrix an unsettled figure is drawn unlike a settled one` no longer reads a
 * computed property, because every property is one of an endless list of ways to
 * be invisible. It reads the pixels and requires the mark's own colour to be
 * among them, which nothing silent can pass — but that turns the colour into a
 * value two files have to agree on, and this branch's whole subject is what
 * happens when they are two spellings instead of one. Restyle the sheet alone
 * and the check hunts an ink that is on no page: green, over marks a reviewer
 * cannot see, which is precisely the defect the pixel read was written to close.
 *
 * So the sheet draws `SETTLED_INK` and the amber notice is written in it too —
 * they are one mark and its caption, and a reviewer who meets them apart has
 * been told two things. Both are read back here.
 */
test("the ink a mark is drawn in is the ink its own notice is written in", () => {
  const page = indexPage(rowsFor(STATES, VIEWPORTS, (state, viewport) => shotName(state.id, viewport.id)), STATES, VIEWPORTS);
  const banner = notices([], [], ["desktop--one.png"], [], []);

  assert.deepEqual(
    [/^#[0-9a-f]{6}$/u.test(SETTLED_INK), page.split(SETTLED_INK).length - 1, banner.includes(`color:${SETTLED_INK}`)],
    [true, 2, true],
  );
});

/**
 * The mark is *added to* the tag the page writes, never a second spelling of it.
 *
 * `applyMarks` searches for `figureTag(shot)` and then rebuilt the opening tag
 * it wrote back, which bound the two ends in the *search* direction only. Give
 * `figureTag` another attribute — a class, a `loading` hint, an id — and the
 * marker still finds the figure, and then silently drops that attribute from
 * every figure it rewrites. The marked figures lose it while the unmarked ones
 * keep it, `unmarked` stays empty, and the gate stays green: the same drift as
 * an anchor that moved, in the one direction nothing was looking.
 *
 * Rebuilding and inserting produce the identical string for the tag written
 * today, so asking this of `figureTag`'s current shape would prove nothing —
 * the test would hold by construction, which is the defect, not the check. It
 * is therefore asked of `markTag` over a tag carrying attributes no figure has
 * yet: what must be true is that the mark is added and nothing else changes.
 */
test("stamping adds the mark to a figure tag and drops nothing it already carried", () => {
  const tag = '<figure class="shot" id="first" data-shot="a.png">';

  const stamped = markTag(tag);

  assert.deepEqual(
    [stamped.startsWith(tag.slice(0, -1)), stamped.includes(SETTLED_MARK), stamped.endsWith(">"), stamped.length],
    [true, true, true, tag.length + SETTLED_MARK.length + 1],
  );
});

/**
 * …and the page's stamped figures are that function over that tag.
 *
 * The binding between the two, over the page the suite really writes. It cannot
 * on its own tell insertion from rebuilding — for the tag `figureTag` writes
 * today the two produce the identical string — which is what the test below is
 * for. What it does hold is that whatever `applyMarks` writes is what `markTag`
 * would write, on the real index rather than on a fixture.
 */
test("the figures a marked page carries are the page's own tags, stamped", () => {
  const page = indexPage(rowsFor(STATES, VIEWPORTS, (state, viewport) => shotName(state.id, viewport.id)), STATES, VIEWPORTS);
  const target = shotName(STATES[0].id, VIEWPORTS[0].id);
  const literal = target.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

  const marked = applyMarks(page, "", [target]);
  const stamped = new RegExp(`<figure[^>]*data-shot="${literal}"[^>]*>`, "u").exec(marked.html)?.[0];

  assert.equal(stamped, markTag(figureTag(target)));
});

/**
 * …and it stamps the tag the page carries, not one it rebuilds from the shot.
 *
 * This is the question `markTag` was extracted to make askable and could not,
 * until now, actually be asked. While `applyMarks` searched for the whole of
 * `figureTag(shot)`, a page whose figure carried an attribute this module never
 * wrote was *unfindable*: the search could only ever match tags of exactly the
 * shape written today, so every fixture that could have distinguished insertion
 * from rebuilding was rejected before either happened. `markTag` was proved
 * alone, `applyMarks` was proved to agree with it on the one tag for which
 * every implementation agrees, and reverting the marker to
 * `` `<figure data-shot="${escape(shot)}" ${SETTLED_MARK}>` `` passed both.
 *
 * The marker anchors on the tag's opening instead, so a real page's figure can
 * carry anything after `data-shot` and still be found — and what comes back has
 * to be that tag with the mark added. A rebuild drops the two attributes it was
 * never told about.
 */
test("the marker stamps attributes the page carries that it never wrote", () => {
  const carried = `${figurePrefix(A)} class="shot" id="first"`;
  const page = `<main>${carried}><img src="./${A}"></figure></main>`;

  const marked = applyMarks(page, "", [A]);
  const stamped = /<figure[^>]*>/u.exec(marked.html)?.[0];

  assert.deepEqual(
    [marked.unmarked, stamped?.includes('class="shot"'), stamped?.includes('id="first"'), stamped],
    [[], true, true, markTag(`${carried}>`)],
  );
});

/**
 * The attributes of one element, read the way a browser reads them.
 *
 * A row is asked what its `<figure>` and `<img>` actually carry, because
 * `row.includes('src="./…"')` answers yes to a *suffix* of some other
 * attribute's value. Adding `data-original-src="./${escape(shot)}"` beside a
 * `src` written raw satisfied that check while the real `src` broke open on the
 * hostile quote — the substring was found, in the wrong attribute.
 *
 * First occurrence wins, and a repeat is reported. `Object.fromEntries` takes
 * the *last* of a repeated name and a browser takes the first, so a row could
 * carry `src` twice — the raw one the page would use, then an escaped one — and
 * this read the escaped copy and agreed with it while Chromium loaded the
 * broken one. A check that parses the markup differently from the renderer is
 * reading a page nobody will ever see, so the disagreement is closed in the
 * renderer's favour and the duplicate itself is a finding: a row is malformed
 * output whether or not the winning value happens to be right.
 *
 * Names are folded to lower case for the same reason, and it is the same defect
 * one notch along. HTML attribute names are ASCII case-insensitive, so
 * `SRC="./broken.png" src="./real.png"` is *one* attribute to Chromium, which
 * keeps the first and loads the broken one. Keyed by the name as written, this
 * saw two different attributes, reported no repeat, and agreed with the second —
 * green over a gallery of broken images. Case-folding closes the disagreement
 * and makes the collision a repeat, which is what it is.
 *
 * `unparsed` is what the parser could not account for, and it is the guard on
 * the parser's own competence. This understands double-quoted values and
 * nothing else, which is enough because every attribute a row writes goes
 * through `escape`; but a value written single-quoted or bare would simply not
 * match, and an attribute this cannot see is one it silently reports as absent.
 * So everything between the tag name and its `>` must be consumed by the
 * matches, and whatever is left over is returned to be asserted empty rather
 * than quietly dropped.
 */
function attributesOf(row, tag) {
  const opens = row.indexOf(`<${tag}`);
  const text = row.slice(opens, row.indexOf(">", opens) + 1);
  const attributes = new Map();
  const repeated = [];
  let rest = text.slice(`<${tag}`.length, -1);
  for (const match of text.matchAll(/([^\s"'=<>/]+)="([^"]*)"/gu)) {
    const name = match[1].toLowerCase();
    rest = rest.replace(match[0], " ");
    if (attributes.has(name)) {
      repeated.push(name);
    } else {
      attributes.set(name, match[2]);
    }
  }
  return { attributes: Object.fromEntries(attributes), repeated, unparsed: rest.replace(/\/\s*$/u, "").trim() };
}

/**
 * A state's own text is written into `alt`, `data-shot`, the heading, the
 * summary, the meta line and the fixture list, and nothing stops any of them
 * from containing a quote. No state carries one today, so `escape` doing
 * nothing at all was invisible: `@matrix gallery index` looks only for
 * `src="./…"` substrings, which survive an attribute broken open three
 * characters earlier.
 *
 * The fixture is hostile in *every* field a row interpolates, because a
 * fixture hostile in two of them proved only those two: dropping the escape
 * from `summary`, `kind` and the fixture join left the suite green.
 *
 * Looking only for leaked tags is the same half-check one layer along: a shot
 * name written raw breaks `data-shot` and `src` open three characters early
 * without introducing a single `<`, so the tag filter stays empty and the marker
 * then hunts for an anchor the page never wrote. The two attributes a mark and a
 * reviewer's eye depend on are therefore read as attributes and compared whole,
 * not searched for as text anywhere in the row.
 */
test("a state's own text cannot break out of the attribute it is written into", () => {
  const hostile = {
    id: 'probe--"><script>x</script>',
    summary: "<em>tea</em> & toast",
    kind: "<b>probe</b>",
    covers: "<i>a card</i>",
    fixture: ["<u>one</u>"],
  };
  const shot = 'desktop--"x.png';

  const row = rowsFor([hostile], VIEWPORTS, () => shot);
  const figure = attributesOf(row, "figure");
  const image = attributesOf(row, "img");

  assert.deepEqual(
    [
      escape('a "b" <c> & d'),
      ["<script>", "<em>", "<b>", "<i>", "<u>"].filter((tag) => row.includes(tag)),
      figure.attributes["data-shot"],
      image.attributes.src,
      [...figure.repeated, ...image.repeated],
      [figure.unparsed, image.unparsed],
      row.includes(`data-shot="${shot}"`),
    ],
    ["a &quot;b&quot; &lt;c&gt; &amp; d", [], escape(shot), `./${escape(shot)}`, [], ["", ""], false],
  );
});

/**
 * Motion has to be attributable, not merely counted.
 *
 * `auditSettled` returns a number, and a number is not a correspondence: "2
 * render(s) were not proved settled" reads identically whether the two are the
 * `transport:playing` figures, which advance on a timer and are supposed to
 * move, or two ordinary screens that have quietly become nondeterministic. The
 * gallery published both worlds as the same clean note.
 *
 * So the two directions are separated and both are findings. `stray` is a
 * render that moved without any state claiming it would. `still` is the
 * opposite and the subtler one: a declaration that has gone stale, which is
 * what a regression in the transport would look like — Play stops advancing the
 * run, the render comes to rest, and an exemption phrased only as permission
 * would absorb it in silence.
 */
test("an unsettled render is a finding unless a state declared it in motion", () => {
  const records = {
    "desktop--moving.png": { settled: false },
    "desktop--drifted.png": { settled: false },
    "desktop--stale.png": { settled: true },
    "desktop--ordinary.png": { settled: true },
  };
  const found = auditMotion(records, ["desktop--moving.png", "desktop--stale.png"]);
  assert.deepStrictEqual(found, { stray: ["desktop--drifted.png"], still: ["desktop--stale.png"], unproved: [] });
});

/**
 * A record with no settle evidence is a third finding, and it exists because
 * the two above were written as a filter on records carrying a boolean — so a
 * record without the field fell out of both lists and was reported by neither.
 * Deleting the field from the record writer left the whole matrix green over
 * five hundred renders the artefact made no motion claim about at all.
 *
 * Absent and unreadable are the same news here: nothing says whether this
 * screenshot is a result or whichever frame the run happened to catch.
 */
test("a record that carries no settle evidence is a finding rather than a record to skip", () => {
  const records = {
    "desktop--proved.png": { settled: true },
    "desktop--silent.png": { signature: "abc" },
    "desktop--wrongtype.png": { settled: "yes" },
    "desktop--null.png": { settled: null },
  };
  const found = auditMotion(records, []);
  assert.deepStrictEqual(found, {
    stray: [],
    still: [],
    unproved: ["desktop--null.png", "desktop--silent.png", "desktop--wrongtype.png"],
  });
});

/**
 * The declared set is read off the registry rather than kept beside it, so the
 * audit and the per-render assertion in `state-matrix.spec.mjs` cannot disagree
 * about which figures are entitled to move.
 */
test("the renders entitled to move are the registry's own, at every viewport", () => {
  const moving = movingShots(REGISTRY_STATES, Object.values(REAL_VIEWPORTS));
  const declared = REGISTRY_STATES.filter((state) => state.expect?.settles === false);
  assert.deepStrictEqual(
    { count: moving.length, expected: declared.length * Object.values(REAL_VIEWPORTS).length, unique: new Set(moving).size },
    { count: 2, expected: 2, unique: 2 },
  );
});

/**
 * A name in a directory listing is not a picture.
 *
 * `auditNames` decides completeness from a file list, and a file list is the one
 * place a broken render still looks right: truncating every published PNG to
 * zero bytes left the gallery suites green at 54 of 54, and the artefact a human
 * had been sent to review was five hundred broken images under five hundred
 * headings.
 *
 * A published file is asked whether it holds a whole PNG.
 *
 * Table-driven over what a published file can be. `truncated` is what a worker
 * killed mid-write leaves behind: a perfect header and no `IEND`,
 * indistinguishable from a whole render by size alone. `endsOnly` is the one
 * that got past the check when it read only the two ends — a signature with the
 * closing chunk stapled straight onto it, both ends perfect and no image
 * between them.
 *
 * `noIdat` and `badSig` are here because a conjunct no row fails *alone* is a
 * conjunct no row pins. `endsOnly` and `notapng` each break several clauses at
 * once, so the size floor and the signature/IHDR match were both deletable with
 * this table still green. `noIdat` is the smallest file with two perfect ends
 * and nothing in between — a header and an `IEND`, below the floor and whole by
 * every other clause. `badSig` is the mirror: correct size, dimensions and
 * close, and one wrong byte in the signature.
 *
 * One wrong byte pins one byte, which is why `badSig` is not the whole of that
 * claim. The test below corrupts each of the sixteen in turn.
 *
 * The ends are taken from a real encoder's output the way the teardown takes
 * them, so a typo in the constants cannot agree with a matching typo here —
 * which is also why `noIdat` staples the real file's own tail on rather than a
 * hand-written `IEND`.
 */
test("a published render is asked whether it holds a whole PNG, not merely a name", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const html = [...Buffer.from("<!DOCTYPE html><title>not a png</title><p>nothing to see</p>")];
  const found = auditBytes([
    ends("desktop--whole.png", real),
    ends("desktop--empty.png", []),
    ends("desktop--truncated.png", [...real.slice(0, PNG_HEAD), ...new Array(80).fill(0x00)]),
    ends("desktop--notapng.png", html),
    ends("desktop--endsonly.png", [...Buffer.from("iVBORw0KGgpJRU5ErkJggg==", "base64")]),
    ends("desktop--noidat.png", [...real.slice(0, PNG_HEAD), ...real.slice(-PNG_TAIL)]),
    ends("desktop--badsig.png", [real[0] ^ 0xff, ...real.slice(1)]),
  ]);
  assert.deepStrictEqual(found, {
    empty: ["desktop--empty.png"],
    malformed: [
      "desktop--badsig.png",
      "desktop--endsonly.png",
      "desktop--noidat.png",
      "desktop--notapng.png",
      "desktop--truncated.png",
    ],
  });
});

/**
 * A header that is well-formed and describes a picture of no size is not a
 * render either, and it is the one defect the chunk's *shape* cannot catch —
 * every length and every type is right.
 *
 * Both dimensions, separately. Zeroing only the width left the height clause
 * unpinned: deleting `uint32(open, 20) > 0` kept all 43 tests green, this one
 * included, under the very name that claims to reject a picture of no size. A
 * `4x0` render is an ordinary broken-encoder output, and it was the half of the
 * rule nothing asked about.
 *
 * The rows are resealed, and without that neither clause is pinned at all. A
 * dimension lives inside `IHDR`, so zeroing one breaks the chunk's own checksum
 * and round nineteen's walk refuses the file before either comparison is
 * reached: deleting both clauses left all 443 tests green, this test included,
 * because it was the walk answering under this test's name. Repaired, each row
 * is a structurally perfect PNG describing a picture of no size — the one break
 * every length and every checksum agrees with, which is why it needs a clause
 * of its own.
 */
test("a header describing a picture of no size is not a whole PNG", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const zeroed = (at) => {
    const bytes = [...real];
    bytes.splice(at, 4, 0x00, 0x00, 0x00, 0x00);
    return resealed(bytes);
  };
  const found = [["desktop--nowidth.png", 16], ["desktop--noheight.png", 20]]
    .map(([name, at]) => auditBytes([ends(name, zeroed(at))]).malformed);
  assert.deepStrictEqual(found, [["desktop--nowidth.png"], ["desktop--noheight.png"]]);
});

/**
 * Every byte the header is compared against, corrupted on its own.
 *
 * `badSig` flips byte zero, and one row that fails on one byte pins one byte.
 * The comparison is sixteen — the eight-byte signature and the eight that open
 * `IHDR`, its fixed length of 13 and its type — and the other fifteen were
 * marks: narrowing `matches(shot.open, [...PNG_OPEN, ...PNG_IHDR])` to
 * `matches(shot.open, [PNG_OPEN[0]])` left all 43 tests green, `badSig`
 * included. A file whose signature is right and whose first chunk claims a
 * length of 14, or is not `IHDR` at all, would have been read as a whole render.
 *
 * A conjunct is pinned by a row that fails on it alone, so a sixteen-byte
 * conjunct needs sixteen rows. Each flips one byte of a real encoder's output
 * and nothing else: the size, both dimensions and the closing chunk stay exactly
 * as they were.
 *
 * That makes the first eight rows isolating and the last eight not, and the
 * difference is round nineteen's doing rather than this test's. Bytes 8–15 are
 * inside `IHDR`, so flipping one breaks the chunk's own checksum and the walk
 * refuses the file before the comparison is reached. Every row here is still
 * true — such a file is malformed — but only the signature's eight can be *the
 * reason*, and narrowing the comparison to `matches(shot.open, PNG_OPEN)` left
 * all 443 tests green with this test among them. The two tests below pin the
 * other half, on files that walk.
 */
test("every byte of the header a whole PNG opens with is one the audit reads", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const header = real.slice(0, 16);
  const flipped = (at) => real.map((byte, index) => (index === at ? byte ^ 0xff : byte));
  const found = header.map((_, at) => auditBytes([ends(`desktop--byte${at}.png`, flipped(at))]).malformed);

  assert.deepStrictEqual(found, header.map((_, at) => [`desktop--byte${at}.png`]));
});

/**
 * The bytes the audit compares against are the ones a real PNG carries, and a
 * real one passes.
 */
test("the bytes a whole PNG opens and closes with are the ones a real one carries", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const found = auditBytes([ends("desktop--real.png", real)]);
  assert.deepStrictEqual([found, real.length > PNG_HEAD + PNG_TAIL], [{ empty: [], malformed: [] }, true]);
});

/**
 * A first chunk that walks, checksums and closes correctly, and is still not a
 * header.
 *
 * The `IHDR` half of the header comparison was a mark. Narrowing
 * `matches(shot.open, [...PNG_OPEN, ...PNG_IHDR])` to `matches(shot.open,
 * PNG_OPEN)` left all 443 tests green, because every row that broke those eight
 * bytes broke the chunk's own checksum with them and the walk answered first.
 *
 * So these rows walk. Four flip one byte of the chunk's *type* and repair every
 * checksum afterwards; the fifth is assembled the way an encoder assembles a
 * file, with a first chunk carrying fourteen bytes of data instead of thirteen,
 * followed by the real file's own `IDAT` and `IEND` untouched. Each is
 * structurally perfect — every length agrees, every checksum agrees, it holds
 * image data and it closes — and its first chunk is still not `IHDR` with the
 * length the format fixes. The comparison is the only clause left to say so.
 *
 * The type is flipped byte by byte and the length is not, and that asymmetry is
 * a limit rather than an oversight. A file may claim any type in four bytes and
 * still be walked, but it can only claim a *length* it actually carries: pinning
 * byte 8 on its own would need a fixture of sixteen megabytes. The walk catches
 * those, and this row pins that the length is compared at all.
 */
test("a first chunk that walks and is not a header of length 13 is not a whole PNG", () => {
  const rows = notHeader();
  const found = rows.map(([name, bytes]) => auditBytes([ends(`desktop--${name}.png`, bytes)]).malformed);

  assert.deepStrictEqual(found, rows.map(([name]) => [`desktop--${name}.png`]));
});

/**
 * Each of those rows is a whole render in every respect but the one it breaks:
 * it walks to its end holding image data, it closes with the real file's own
 * `IEND`, its header describes a picture with a size, and it clears the floor.
 *
 * Without this, a row that also broke the walk would pass the test above while
 * pinning nothing — which is precisely how the sixteen-byte rows stopped
 * pinning, one round earlier and one layer out.
 */
test("a file whose first chunk is not a header is whole in every other way", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const read = notHeader().map(([, bytes]) => [
    walkChunks(bytes)?.includes("IDAT"),
    bytes.slice(-PNG_TAIL),
    [uint32(bytes, 16) > 0, uint32(bytes, 20) > 0, bytes.length >= PNG_HEAD + 12 + PNG_TAIL],
  ]);

  assert.deepStrictEqual(read, notHeader().map(() => [true, real.slice(-PNG_TAIL), [true, true, true]]));
});

/**
 * A stream that walks to its end, holds a picture, and never closes.
 *
 * `matches(shot.close, PNG_IEND)` was the third mark: deleting it left all 443
 * tests green. Every row that had lost its closing chunk had lost the walk with
 * it — `truncated` stops in the middle of a chunk, `endsOnly` has no chunks at
 * all, `noIdat` holds no picture — so the walk was always the reason and the
 * comparison never was.
 *
 * Dropping the whole of `IEND` leaves a file that walks: `IHDR` and `IDAT`, both
 * intact, ending exactly where the file ends. It holds image data, its header is
 * a header, and it clears the floor by a byte. It simply never says it is
 * finished, which is what a writer killed between chunks leaves behind. The
 * walked stream is asserted beside the verdict, so the row is known to have
 * failed on the closing chunk rather than on the walk.
 */
test("a stream that holds a picture and never closes is not a whole PNG", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const unclosed = real.slice(0, real.length - PNG_TAIL);
  const found = auditBytes([ends("desktop--unclosed.png", unclosed)]);

  assert.deepStrictEqual(
    [found, walkChunks(unclosed)],
    [{ empty: [], malformed: ["desktop--unclosed.png"] }, ["IHDR", "IDAT"]],
  );
});

/**
 * The middle of the file, which the ends could never speak for.
 *
 * `whole()` read 33 bytes at the front and 12 at the back, and a render whose
 * only `IDAT` chunk was renamed to `JUNK` — same length, same size, both ends
 * untouched — passed every clause while Chromium refused it outright: *"the
 * source image could not be decoded."* Five hundred undecodable figures could
 * have been published and certified complete.
 *
 * Three rows, because the middle can lie in three ways. `renamed` keeps every
 * length, every checksum and both ends and carries no image data — the defect
 * the ends can never see. `corrupt` flips one byte *inside* a chunk's data,
 * which no length and no end disagrees with; only the checksum does. `overrun`
 * gives a chunk a length that runs past the end of the file, which is what a
 * writer interrupted mid-chunk leaves behind.
 *
 * The CRCs are repaired after the rename, on purpose: a renamed chunk breaks its
 * own checksum too, and a row that fails for two reasons pins neither. That row
 * is a structurally perfect PNG that holds no picture. The other two are left
 * unsealed for the same reason in reverse — repairing them would remove the very
 * clause they exist to fail.
 *
 * `PNG_HEAD` is where the second chunk begins, because it is the signature plus
 * the whole of `IHDR`. The fixture's own layout is `IHDR`, `IDAT`, `IEND`, so
 * that offset is the image data this test edits.
 */
test("a render whose middle is not a picture is not a whole PNG", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const at = PNG_HEAD;
  const renamed = resealed(real.map((byte, index) => (index >= at + 4 && index < at + 8 ? "JUNK".charCodeAt(index - at - 4) : byte)));
  const corrupt = real.map((byte, index) => (index === at + 9 ? byte ^ 0xff : byte));
  const overrun = real.map((byte, index) => (index === at + 3 ? 0xc8 : byte));
  const found = [["renamed", renamed], ["corrupt", corrupt], ["overrun", overrun]]
    .map(([name, bytes]) => auditBytes([ends(`desktop--${name}.png`, bytes)]).malformed);

  assert.deepStrictEqual(found, [["desktop--renamed.png"], ["desktop--corrupt.png"], ["desktop--overrun.png"]]);
});

/**
 * Each of those three rows leaves both ends and the size exactly as a real
 * render has them, so the walk is the only clause that can be the reason.
 *
 * Without this, a fixture that also broke the header would pass the test above
 * while pinning a conjunct that was already pinned — the precise defect the
 * sixteen-byte header rows were added to close, one layer further in.
 */
test("a render broken only in its middle is whole at both of its ends", () => {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const at = PNG_HEAD;
  const rows = [
    resealed(real.map((byte, index) => (index >= at + 4 && index < at + 8 ? "JUNK".charCodeAt(index - at - 4) : byte))),
    real.map((byte, index) => (index === at + 9 ? byte ^ 0xff : byte)),
    real.map((byte, index) => (index === at + 3 ? 0xc8 : byte)),
  ].map((bytes) => [bytes.length, bytes.slice(0, PNG_HEAD), bytes.slice(-PNG_TAIL)]);

  assert.deepStrictEqual(rows, rows.map(() => [real.length, real.slice(0, PNG_HEAD), real.slice(-PNG_TAIL)]));
});

/**
 * A real render's stream is walked end to end and named for what it holds, so
 * the rows above fail for the reason claimed rather than because the walk
 * rejects everything handed to it.
 */
test("the chunk stream of a real PNG is walked to its end", () => {
  assert.deepStrictEqual(walkChunks([...Buffer.from(REAL_PNG, "base64")]), ["IHDR", "IDAT", "IEND"]);
});

/** A four-byte big-endian field, as PNG spells every number. */
function uint32(bytes, at) {
  return ((bytes[at] << 24) | (bytes[at + 1] << 16) | (bytes[at + 2] << 8) | bytes[at + 3]) >>> 0;
}

/**
 * Re-checksums every chunk of a file whose bytes have been edited, so a fixture
 * breaks only the clause it is written to break.
 *
 * Computed with `zlib.crc32` rather than with the audit's own CRC: a fixture
 * that borrowed the implementation under test would agree with any error in it,
 * which is the same reason the fixtures above are cut from a real encoder's
 * output rather than hand-written.
 */
function resealed(bytes) {
  const sealed = [...bytes];
  for (let at = 8; at + 12 <= sealed.length;) {
    const end = at + 12 + uint32(sealed, at);
    const sum = crc32(Buffer.from(sealed.slice(at + 4, end - 4)));
    sealed.splice(end - 4, 4, (sum >>> 24) & 0xff, (sum >>> 16) & 0xff, (sum >>> 8) & 0xff, sum & 0xff);
    at = end;
  }
  return sealed;
}

/** A four-byte big-endian field, written as PNG writes every number. */
function be32(value) {
  return [(value >>> 24) & 0xff, (value >>> 16) & 0xff, (value >>> 8) & 0xff, value & 0xff];
}

/**
 * One PNG chunk as an encoder writes it: its length, its type, its data and a
 * checksum over the last two. Checksummed with `zlib.crc32` for the same reason
 * `resealed` is — a fixture that borrowed the audit's own CRC would agree with
 * any error in it.
 */
function chunk(type, data) {
  const body = [...Buffer.from(type, "ascii"), ...data];
  return [...be32(data.length), ...body, ...be32(crc32(Buffer.from(body)))];
}

/**
 * Files whose first chunk is not `IHDR` carrying the thirteen bytes the format
 * fixes, and which are whole in every other respect.
 *
 * The four type rows are the real file with one byte of the chunk's type
 * flipped and every checksum repaired after. The length row is rebuilt rather
 * than edited: a length field is what says where the next chunk begins, so
 * changing it in place moves every chunk after it and the walk — not the header
 * comparison — becomes the reason. Assembled instead, with a fourteen-byte
 * first chunk and the real file's own `IDAT` and `IEND` following it, the file
 * walks to its end and only its declared length is wrong.
 */
function notHeader() {
  const real = [...Buffer.from(REAL_PNG, "base64")];
  const typed = [12, 13, 14, 15].map((at) => [
    `type${at}`,
    resealed(real.map((byte, index) => (index === at ? byte ^ 0xff : byte))),
  ]);
  return [...typed, ["length14", [...real.slice(0, 8), ...chunk("IHDR", [...real.slice(16, 29), 0x00]), ...real.slice(33)]]];
}
