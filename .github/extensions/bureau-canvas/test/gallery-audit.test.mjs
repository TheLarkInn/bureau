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

import { auditNames, auditTwins, expectedShots, shotName } from "../e2e/playwright/gallery-audit.mjs";

const VIEWPORTS = [{ id: "desktop" }, { id: "compact" }];
const STATES = [{ id: "surface:config+card:expanded" }, { id: "probe--draft-bar" }];

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
    [auditTwins(same, []).map((finding) => finding.kind), auditTwins(same, [twin])],
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

  const findings = auditTwins(parted, [twin]);

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

  assert.deepEqual(auditTwins(same, [twin]).map((finding) => finding.detail.includes("compact--")), [true]);
});

/**
 * One state's two renders are the same DOM at two widths, on purpose — the
 * signature is geometry-free, so that is what "the layout differs" looks like
 * to it. Comparing across viewports therefore reports every state that does not
 * reflow as a coincidence, which is most of them.
 */
test("one state rendered at both viewports is not a twin of itself", () => {
  const both = { [A]: "digest-1", [shotName("probe--refusal-dismissed", "compact")]: "digest-1" };

  assert.deepEqual(auditTwins(both, []), []);
});
