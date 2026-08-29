import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { blocksDelete, referrers } from "../lib/preflight.mjs";
import { valueOf } from "../web/statelab/dimensions.mjs";
import { BLOCKED_PREFLIGHT } from "../web/statelab/intercept.mjs";
import { SELECTORS as S, withheld } from "../web/statelab/selectors.mjs";

const committed = JSON.parse(await readFile(new URL("./fixtures/committed-payload.json", import.meta.url), "utf8"));

test("deleting a role names the assignment and the step that run it", () => {
  const found = referrers(committed, "role", "implementer");

  assert.deepEqual(found.map((item) => [item.kind, item.name, item.severity]), [
    ["assignment", "agent-eligible", "referrer"],
    ["step", "agent-eligible-pipeline/implement", "referrer"],
  ]);
});

test("deleting the primary repo is called out as the landing repo", () => {
  const found = referrers(committed, "repo", "bureau");

  assert.deepEqual(
    { severities: found.map((item) => item.severity), message: found[0]?.message },
    { severities: ["primary-repo"], message: "assignment `agent-eligible` lands its branch on `bureau`" },
  );
});

test("deleting an unreferenced entity reports nothing", () => {
  const payload = structuredClone(committed);
  payload.config.pipelines.unused = { name: "unused", steps: [] };
  payload.config.roles.spare = { name: "spare", agent: "/x:y", adapter: "copilot", permissions: [], min_trust: "derived" };

  assert.deepEqual(
    { pipeline: referrers(payload, "pipeline", "unused"), role: referrers(payload, "role", "spare") },
    { pipeline: [], role: [] },
  );
});

test("deleting a step reports its referrers and what it strands", () => {
  const found = referrers(committed, "step", "verify", { pipeline: "agent-eligible-pipeline" });

  assert.deepEqual(
    {
      severities: [...new Set(found.map((item) => item.severity))].sort(),
      // `review` takes `verify` as input and is only reached through it.
      strands: found.filter((item) => item.severity === "orphaned").map((item) => item.name),
    },
    { severities: ["orphaned", "referrer"], strands: ["review"] },
  );
});

test("deleting the entry step is its own severity, naming the successor", () => {
  const found = referrers(committed, "step", "implement", { pipeline: "agent-eligible-pipeline" });
  const entry = found.find((item) => item.severity === "entry-step");

  assert.deepEqual(
    { severity: entry?.severity, mentionsSuccessor: entry?.message.includes("verify") },
    { severity: "entry-step", mentionsSuccessor: true },
  );
});

test("preflight is advisory and orphaning alone does not block", () => {
  const orphansOnly = [{ severity: "orphaned" }];
  const referenced = [{ severity: "referrer" }];

  assert.deepEqual(
    { orphansOnly: blocksDelete(orphansOnly), referenced: blocksDelete(referenced), marked: referrers(committed, "role", "implementer")[0].source },
    { orphansOnly: false, referenced: true, marked: "preflight" },
  );
});

/**
 * The one declared dimension value no combination of dimensions renders, held
 * to what it claims — at the two ends a render cannot hold for itself.
 *
 * `field:delete-blocked` is kept in the registry so the screen is excluded by a
 * named rule rather than missing, and `delete-is-offered-only-where-nothing-
 * refers` says why: `DeleteControl` mounts on an assignment card and on the
 * orphan strip, and neither can answer with referrers.
 *
 * This test used to own the screen as well, by reading `web/app.mjs` for the
 * spelling of its two lines. That was a mark, and the measurement is the
 * argument: gating the sentence behind `preflight.blocking && false` left the
 * literal in the source and this file green, and changing the Confirm's
 * `disabled` to `preflight.blocking && false || busy` did the same. A file that
 * still contains a line says nothing about whether the line reached a screen.
 *
 * So the screen moved to `probe--delete-preflight-blocked`, which renders it
 * from these very lists. Two ends remain that a render cannot hold for itself,
 * and they are what is left here.
 *
 * `declaresCopy` is the first: the probe spreads `copy`, so an empty `copy`
 * spreads into an empty set of claims and the render agrees with a promise that
 * asks nothing. Emptying it left this file green at 7/7 and the whole offline
 * suite green at 470; a blank string does the same while looking populated, so
 * both are refused.
 *
 * `staged` is the second: the probe renders the host's answer, and a harness is
 * free to invent one. This pins it to the answer `lib/preflight.mjs` actually
 * produces for the role that sample references, so a change in what the host
 * reports fails here rather than leaving a probe rendering a shape the product
 * stopped producing — and `blocking` is checked against `blocksDelete` rather
 * than asserted, because that is the rule the screen exists to obey.
 *
 * `declaresControls` is pinned to the list rather than to its length, and the
 * distinction is the whole point. The probe *spreads* these selectors, so the
 * registry decides what the render asks for — which is the same shape as the
 * `suppress` fault two rounds ago, where a declaration that quietly asked for
 * less could not be caught by a render that agreed with it. Dropping
 * `withheld(S.deleteConfirm)` and leaving `[S.preflight]` left the offline suite
 * green at 471 *and* both of the probe's renders green, with the most important
 * claim on the screen — that the Confirm is withheld — no longer asked by
 * anything. Length would not have moved. The list is deliberately brittle for
 * the same reason the count pins above are: a selector added or removed here is
 * meant to stop and be looked at.
 */
test("the blocked delete declares a real sentence, and is staged from the host's own answer", () => {
  const declared = valueOf("field", "delete-blocked");
  const staged = BLOCKED_PREFLIGHT.result;

  assert.deepEqual(
    {
      declaresCopy: declared.copy.length > 0 && declared.copy.every((line) => typeof line === "string" && line.trim() !== ""),
      declaresControls: declared.shows,
      referrers: staged.referrers,
      blocking: staged.blocking === blocksDelete(staged.referrers),
      blocks: blocksDelete([{ severity: "referrer" }]),
    },
    {
      declaresCopy: true,
      declaresControls: [S.preflight, withheld(S.deleteConfirm)],
      referrers: referrers(committed, "role", "implementer"),
      blocking: true,
      blocks: true,
    },
  );
});
