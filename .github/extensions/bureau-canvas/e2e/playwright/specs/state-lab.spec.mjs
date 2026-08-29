// The state lab, checked the way a reviewer uses it.
//
// The lab is a deliverable, not scaffolding: if it stops rendering states or
// stops agreeing with the registry, the matrix is no longer reviewable by a
// human. So the suite opens it, drives it, and asserts that what it reports
// about a state matches what the registry says about that state.

import { ENTRY_TRANSITIONS, rootReason, STATES, summary, TRANSITIONS } from "../../../web/statelab/registry.mjs";
import { CONSTRAINTS, harnessNotes } from "../../../web/statelab/constraints.mjs";
import { servableInFrame } from "../../../web/statelab/intercept.mjs";
import { VIEWPORTS } from "../../../web/statelab/selectors.mjs";
import { expect, test } from "../matrix-fixtures.mjs";

/** A state the lab can drive itself, interception included. */
const DRIVABLE = STATES.filter((state) => servableInFrame(state.intercept));

async function openLab(page, host) {
  const errors = [];
  page.on("pageerror", (error) => errors.push(`pageerror: ${error}`));
  page.on("console", (message) => {
    if (message.type() === "error") {
      errors.push(`console: ${message.text()}`);
    }
  });
  await page.goto(new URL("statelab.html", host.url).href);
  await page.locator(".state-item").first().waitFor();
  return errors;
}

test("the lab lists every state in the registry and boots clean", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const listed = await page.locator(".state-item .state-id").allTextContents();

  expect(errors).toEqual([]);
  expect(listed.sort()).toEqual(STATES.map((state) => state.id).sort());
  // This host serves the bundled sample, so the fixtures land on the payload
  // they were written against and the lab has nothing to warn about. The note
  // exists for a contributor opening the lab against their own `.bureau/`,
  // where the same state id renders different content.
  await expect(page.locator("#base-note")).toBeHidden();
});

/**
 * Listing a state and being able to open it are two different claims, and only
 * the first one was ever asserted.
 *
 * The suite drove about nine hand-picked states, so "the lab lists every state"
 * was checked and "the lab can open one" was checked nine times. Four probes
 * wrote an `expect` literal with no `hides` key — matrix states get theirs from
 * `expectations()`, probes do not — and the panel's unguarded `for…of` over it
 * threw into the catch that renders the panel as a red note. Four of the 271
 * states could not be opened at all on the surface whose whole purpose is
 * opening them, and every test passed.
 *
 * The registry now normalises the shape and `statelab.test.mjs` pins that
 * offline for all 271, which is what makes the class impossible. This walk is
 * the browser half: it opens every state the lab can drive and requires each one
 * to produce its expectation list, so "the lab lists every state" is joined by
 * the claim that was missing — that it can open every state it lists.
 *
 * Split into chunks so the run parallelises, and tagged `@matrix` so the cost
 * lands in the exhaustive job rather than in the fast suite that gates
 * `lint.sh`. The two states not walked are the ones needing the renderer module
 * blocked, which the lab cannot install from inside its own frame; it blanks the
 * stage and says so, and `state-lab.spec.mjs` asserts that separately.
 *
 * Console errors are deliberately not asserted here: a state may declare
 * `allowErrors`, and judging those against a state's own allowances is the
 * matrix's job. The claim under test is the narrower one that was missing —
 * that the panel is drawn at all.
 */
const WALK_CHUNKS = 8;

for (let chunk = 0; chunk < WALK_CHUNKS; chunk += 1) {
  const group = DRIVABLE.filter((_state, index) => index % WALK_CHUNKS === chunk);

  test(`@matrix the lab opens every state it lists (${chunk + 1}/${WALK_CHUNKS})`, async ({ page, host }) => {
    test.setTimeout(240_000);
    await openLab(page, host);
    const unopened = [];
    for (const state of group) {
      // Selected by filtering and clicking, not by driving the hash. The lab
      // registers its `hashchange` listener only after the first walk resolves,
      // and the list a spec waits for is drawn before that — so a hash written
      // in between is dropped, and the boot walk then stamps its own id back
      // over it. The click path is the one a reviewer uses in any case.
      await page.locator("#search").fill(state.id);
      await page.locator(".state-item").filter({ has: page.getByText(state.id, { exact: true }) }).first().click();
      await expect(page.locator("#detail h2")).toHaveText(state.id, { timeout: 20_000 });
      const drew = await page.locator("#detail .expectations").waitFor({ timeout: 20_000 }).then(() => true, () => false);
      if (!drew) {
        unopened.push(`${state.id}: ${await page.locator("#detail .panel p").first().textContent()}`);
      }
    }

    // Non-empty by construction, and every state lands in exactly one chunk, so
    // a slicing mistake that walked nothing would fail here rather than pass.
    expect([unopened, group.length > 0]).toEqual([[], true]);
  });
}

/**
 * The header a reviewer reads first, held to every number in it.
 *
 * This asserted one of `summary()`'s fifteen metrics, and only as a substring of
 * all of them joined — so it did not even establish which metric carried the
 * number — beside a bare `count() > 0` for the constraint list. Rewriting
 * `renderSummary` to report `0` for the other fourteen left the test green while
 * the lab told a reviewer there were no dimensions, no rules and no transitions.
 *
 * Read as label→value pairs and compared whole, so a metric that goes missing,
 * gains a label, or reports the wrong number is a failure; and the constraint
 * list is pinned to the registry's own length rather than to being non-empty.
 */
test("the lab reports every one of the registry's counts", async ({ page, host }) => {
  await openLab(page, host);
  const shown = await page.locator("#summary .metric").evaluateAll((nodes) => Object.fromEntries(
    nodes.map((node) => [node.querySelector("span").textContent, node.querySelector("strong").textContent]),
  ));
  const expected = Object.fromEntries(Object.entries(summary())
    .map(([key, value]) => [key.replace(/([A-Z])/gu, " $1").toLowerCase(), String(value)]));

  expect(shown).toEqual(expected);
  expect(await page.locator("#constraints details").count()).toBe(CONSTRAINTS.length);
});

test("selecting a state drives the production page and passes its own checks", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const target = DRIVABLE.find((state) => state.id.includes("field:limits") && state.id.includes("fieldState:dirty"));

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await page.locator("#detail h2", { hasText: target.id }).waitFor();
  await page.locator("#detail .expectations").waitFor();
  await expect(page.locator("#detail .expectations li.bad")).toHaveCount(0);
  await expect(page.locator("#detail .expectations li.ok").first()).toBeVisible();
  // The rows are not the panel. Its closing note carries every finding no row
  // spoke for, and asserting only `li.bad` left that note unchecked: a state
  // failing on a kind the rows do not cover was reported by the lab as clean
  // while the matrix failed it, which is the one disagreement a review surface
  // may not have.
  await expect(page.locator("#detail .panel:has(.expectations) .note--err")).toHaveCount(0);
  // The rendered surface is the real page, inside the lab's frame.
  await expect(page.frameLocator("#stage-frame").locator(".limits-editor")).toBeVisible();
  expect(errors).toEqual([]);
});

test("a replay state with a run selected passes its checks in the lab too", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const target = DRIVABLE.find((state) => state.id.includes("mode:replay+run:finished"));

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await page.locator("#detail .expectations").waitFor();
  await expect(page.locator("#detail .expectations li.bad")).toHaveCount(0);
  await expect(page.locator("#detail .panel:has(.expectations) .note--err")).toHaveCount(0);
  await expect(page.frameLocator("#stage-frame").locator(".replay-timeline")).toBeVisible();
  expect(errors).toEqual([]);
});

test("the compact control resizes the stage and re-runs the entry path", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const target = DRIVABLE.find((state) => state.id.endsWith("card:expanded"));
  const stage = page.locator("#stage");

  await page.locator(".state-item", { hasText: target.id }).first().click();
  await expect(stage).toHaveJSProperty("offsetWidth", VIEWPORTS.desktop.width);

  await page.locator('[data-viewport="compact"]').click();
  // The stage's own width, not the note beside it. The note is a label this
  // control writes; asserting it would have held with the two lines that
  // actually resize the stage deleted, which is the whole of what the button
  // does — and the render underneath would have stayed desktop-wide while the
  // lab claimed a compact viewport.
  await expect(stage).toHaveJSProperty("offsetWidth", VIEWPORTS.compact.width);
  await expect(page.locator("#viewport-note")).toContainText(`${VIEWPORTS.compact.width}`);
  await page.locator("#detail .expectations").waitFor();
  // The state the reviewer selected, not whichever one was still on screen. The
  // button re-runs `current`, and while that was set when a walk *started*
  // rather than when it was requested, a click landing during a walk re-ran the
  // previous state and relabelled the panel with it. Nothing caught it because
  // walks used to finish faster than a second click could arrive.
  await expect(page.locator("#detail h2")).toHaveText(target.id);
  await expect(page.frameLocator("#stage-frame").locator(".assignment-detail")).toBeVisible();
  expect(errors).toEqual([]);
});

test("a state the lab cannot install blanks the stage rather than showing the last render", async ({ page, host }) => {
  await openLab(page, host);
  const drivable = DRIVABLE.find((state) => state.id.endsWith("card:expanded"));
  const blocked = STATES.find((state) => state.intercept && !servableInFrame(state.intercept));

  // Drive a state the lab can produce first, so there is a real render on the
  // stage for the next selection to inherit. Returning early used to leave it
  // there beside the intercepted state's description — the lab presenting a
  // screen it never produced as that state's render, which is the one thing a
  // review surface may not do.
  await page.locator(".state-item", { hasText: drivable.id }).first().click();
  await expect(page.frameLocator("#stage-frame").locator(".assignment-detail")).toBeVisible();

  await page.locator(".state-item", { hasText: blocked.id }).first().click();
  await expect(page.locator("#detail .note--warn")).toContainText(blocked.intercept);
  await expect(page.locator("#stage-frame")).toHaveAttribute("src", "about:blank");
});

test("the lab renders a save state itself, under the same refusal the suite asserts", async ({ page, host }) => {
  const errors = await openLab(page, host);
  // `fail-intent` is installed inside the frame before the page's modules run,
  // so a refused save is a screen a reviewer can look at rather than a note
  // saying the browser suite has it. This is the claim that a third of the
  // registry stopped being unreachable in the lab.
  const refused = STATES.find((state) => state.intercept === "fail-intent");

  await page.locator(".state-item", { hasText: refused.id }).first().click();
  await page.locator("#detail .expectations").waitFor();

  await expect(page.locator("#stage-frame")).not.toHaveAttribute("src", "about:blank");
  await expect(page.locator("#detail .expectations li.bad")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("the lab explains why an excluded combination is not a state", async ({ page, host }) => {
  await openLab(page, host);
  const rules = page.locator("#constraints details");

  await expect(rules.first()).toContainText("pruned here");
  await rules.first().locator("summary").click();
  await expect(rules.first().locator("pre.example")).toBeVisible();
});

/**
 * The per-rule tallies are order-dependent by construction, so they cannot
 * answer "why is *this* combination not a state?". The picker can, and this is
 * the assertion that keeps it wired to `violations()` rather than to a list of
 * states someone remembered to update.
 */
test("the picker judges any combination a reviewer assembles", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const verdict = page.locator("#picker-verdict");

  await expect(verdict).toHaveAttribute("data-verdict", "reachable");

  await page.locator('#picker select[aria-label="card"]').selectOption("expanded");
  await expect(verdict).toHaveAttribute("data-verdict", "excluded");
  await expect(verdict).toContainText("boot-has-no-regions");
  await expect(verdict.locator("details")).not.toHaveCount(0);

  await page.locator('#picker select[aria-label="card"]').selectOption("n/a");
  await expect(verdict).toHaveAttribute("data-verdict", "reachable");
  expect(errors).toEqual([]);
});

/**
 * The harness note as a reviewer actually receives it, in both places the lab
 * prints one.
 *
 * The offline suite holds `harnessNotes` — that the sentence it returns names
 * the limit, names the standing state, and claims no more. That is a different
 * claim from *the lab printing it*, and the gap between the two is the whole
 * defect the last round removed: the sentence was written out at each call
 * site, so the surface said "The same screen is rendered by …" while nothing
 * that tested a function ever saw it. Making the sentence data closed the
 * drift between the two copies; it did not make either copy answerable. Either
 * one can be hard-coded back with the offline test still green.
 *
 * So both are read from the rendered DOM, and the overclaim is refused across
 * the whole surface rather than at the two places it is known to have lived —
 * a reviewer told two screens match has been given a reason not to look at the
 * difference, and looking is the entire job of this page.
 */
test("the lab prints the registry's harness note, and the overclaim appears nowhere", async ({ page, host }) => {
  const errors = await openLab(page, host);
  const rule = CONSTRAINTS.find((item) => item.kind === "harness");
  const [limit, instead] = harnessNotes(rule);

  const listed = page.locator("#constraints details", { hasText: rule.title });
  await listed.locator("summary").click();
  await expect(listed).toContainText(limit);
  await expect(listed).toContainText(instead);

  // The picker's copy, driven onto a combination this harness rule rejects.
  const combination = { surface: "config", data: "validated", section: "two-cards", card: "expanded", field: "delete" };
  for (const [axis, value] of Object.entries(combination)) {
    await page.locator(`#picker select[aria-label="${axis}"]`).selectOption(value);
  }
  const rejected = page.locator("#picker-verdict details", { hasText: rule.id });
  await expect(rejected).toContainText(limit);
  await expect(rejected).toContainText(instead);

  await expect(page.locator("body")).not.toContainText(/the same screen is rendered/iu);
  expect(errors).toEqual([]);
});

/**
 * A state nothing reaches first used to say only "a root of the DAG" — the one
 * sentence true of every root, so a reviewer could not tell the screen the
 * canvas opens on from a tuple whose parent quietly failed to be a state.
 *
 * Both cases are walked, because they were not equivalent: the note was gated
 * on the transition list being empty, so a root that *opens* something — a
 * landing with three ways out — rendered its outbound edges and no note at
 * all, which is the state a reviewer is most likely to be reading.
 *
 * The reason shown has to be the reason the registry assigned, not merely some
 * prose, so the category is read back off the panel and compared with
 * `rootReason` for that same state.
 */
for (const [shape, pick] of [
  ["with no edges at all", (state) => !TRANSITIONS.some((edge) => edge.from === state.id || edge.to === state.id)],
  ["that still opens other screens", (state) => TRANSITIONS.some((edge) => edge.from === state.id)],
]) {
  test(`a root ${shape} says which kind of root it is`, async ({ page, host }) => {
    const errors = await openLab(page, host);
    const entered = new Set(ENTRY_TRANSITIONS.map((edge) => edge.to));
    const root = DRIVABLE.find((state) => !entered.has(state.id) && pick(state));

    await page.locator(".state-item", { hasText: root.id }).first().click();
    await page.locator("#detail h2", { hasText: root.id }).waitFor();
    const note = page.locator("#detail .root");
    await expect(note).toHaveAttribute("data-root-reason", rootReason(root).id);
    await expect(note).toContainText(rootReason(root).title);
    // A root that return edges arrive at has to reconcile its own note with
    // the "N in" header directly above it, or the panel reads as a
    // contradiction. That sentence is the only thing that does so.
    const arriving = TRANSITIONS.filter((edge) => edge.to === root.id).length;
    if (arriving) {
      await expect(note).toContainText(`${arriving} edge(s) arriving here`);
    }
    expect(errors).toEqual([]);
  });
}

/**
 * The other half: a state that *is* entered must not claim to be a root. The
 * pair is what makes either assertion mean anything — a lab that printed the
 * note unconditionally would pass the two above and fail this one.
 */
test("a state something reaches first claims no root reason", async ({ page, host }) => {
  await openLab(page, host);
  const entered = DRIVABLE.find((state) => ENTRY_TRANSITIONS.some((edge) => edge.to === state.id));

  await page.locator(".state-item", { hasText: entered.id }).first().click();
  await page.locator("#detail h2", { hasText: entered.id }).waitFor();
  await expect(page.locator("#detail .root")).toHaveCount(0);
  await expect(page.locator("#detail .edges .linkish").first()).toBeVisible();
});

test("the lab links the transition DAG in both directions", async ({ page, host }) => {
  await openLab(page, host);
  const child = DRIVABLE.find((state) => state.id.includes("field:limits+fieldState:dirty"));

  await page.locator(".state-item", { hasText: child.id }).first().click();
  await page.locator("#detail .expectations").waitFor();
  const inbound = page.locator("#detail .edges .linkish").first();
  await expect(inbound).toContainText("←");
  await inbound.click();
  await expect(page.locator("#detail h2")).not.toHaveText(child.id);
});
