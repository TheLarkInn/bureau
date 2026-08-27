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

import { auditNames, auditSettled, auditTwins, auditUnaudited, expectedShots, isDrift, partitionFindings, shotName } from "../e2e/playwright/gallery-audit.mjs";
import { notices } from "../e2e/playwright/global-teardown.mjs";
import { applyMarks, escape, figureTag, indexPage, markTag, NOTICE_ANCHOR, rowsFor, SETTLED_MARK } from "../e2e/playwright/gallery-index.mjs";
import { STATES as REGISTRY_STATES } from "../web/statelab/registry.mjs";
import { VIEWPORTS as REAL_VIEWPORTS } from "../web/statelab/selectors.mjs";

const VIEWPORTS = [{ id: "desktop" }, { id: "compact" }];
const STATES = [{ id: "surface:config+card:expanded" }, { id: "probe--draft-bar" }];

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
 */
test("a mark with nothing to attach to is reported rather than dropped", () => {
  const drifted = `<main class="grid">${figureTag(A).replace(">", ' class="shot">')}</figure></main>`;

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
 */
test("the attribute a figure is stamped with is the one the page's styling draws", () => {
  const page = indexPage(rowsFor(STATES, VIEWPORTS, (state, viewport) => shotName(state.id, viewport.id)), STATES, VIEWPORTS);
  const target = shotName(STATES[0].id, VIEWPORTS[0].id);
  const literal = target.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

  const marked = applyMarks(page, "", [target]);
  const stamped = new RegExp(`<figure data-shot="${literal}"\\s+([^>]+)>`, "u").exec(marked.html)?.[1];

  assert.deepEqual(
    [typeof stamped, page.includes(`figure[${stamped}] img`), page.includes(`figure[${stamped}] figcaption::after`)],
    ["string", true, true],
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
 * The binding between the two, which is what makes the property above reach the
 * gallery: a marker that went back to rebuilding still passes today and fails
 * the moment `figureTag` gains anything, which is exactly when it must.
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
 * The attributes of one element, read as attributes rather than as substrings.
 *
 * A row is asked what its `<figure>` and `<img>` actually carry, because
 * `row.includes('src="./…"')` answers yes to a *suffix* of some other
 * attribute's value. Adding `data-original-src="./${escape(shot)}"` beside a
 * `src` written raw satisfied that check while the real `src` broke open on the
 * hostile quote — the substring was found, in the wrong attribute.
 */
function attributesOf(row, tag) {
  const opens = row.indexOf(`<${tag}`);
  const text = row.slice(opens, row.indexOf(">", opens) + 1);
  return Object.fromEntries([...text.matchAll(/([\w-]+)="([^"]*)"/gu)].map(([, name, value]) => [name, value]));
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

  assert.deepEqual(
    [
      escape('a "b" <c> & d'),
      ["<script>", "<em>", "<b>", "<i>", "<u>"].filter((tag) => row.includes(tag)),
      attributesOf(row, "figure")["data-shot"],
      attributesOf(row, "img").src,
      row.includes(`data-shot="${shot}"`),
    ],
    ["a &quot;b&quot; &lt;c&gt; &amp; d", [], escape(shot), `./${escape(shot)}`, false],
  );
});