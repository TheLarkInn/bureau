import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { blocksDelete, referrers } from "../lib/preflight.mjs";
import { valueOf } from "../web/statelab/dimensions.mjs";

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
 * The one declared dimension value no state renders, held to what it claims.
 *
 * `field:delete-blocked` is kept in the registry so the screen is excluded by a
 * named rule rather than missing, and `delete-is-offered-only-where-nothing-
 * refers` says why: `DeleteControl` mounts on an assignment card and on the
 * orphan strip, and neither can answer with referrers. The consequence is that
 * the controls and copy that value declares are rendered by no state, so
 * nothing in the browser suite can fail for them.
 *
 * That rule's `why` pointed at this file, which owned the blocking *answer* —
 * `blocksDelete` above — and not the *screen* the value describes. The two are
 * different claims, and only the first was held. So the screen is held here
 * too, against the source that draws it: the sentence is read out of the
 * registry rather than restated, so the declared copy and the shipped copy
 * cannot drift apart quietly, and the confirm is held to being withheld on
 * `blocking`, which is what `withheld(S.deleteConfirm)` promises.
 */
test("the screen a blocked delete draws is the one the registry declares", async () => {
  const source = (await readFile(new URL("../web/app.mjs", import.meta.url), "utf8")).replace(/\s+/gu, " ");
  const declared = valueOf("field", "delete-blocked");

  assert.deepEqual(
    {
      copy: declared.copy.every((line) => source.includes(JSON.stringify(line))),
      withholdsConfirm: /disabled: preflight\.blocking/u.test(source),
      blocks: blocksDelete([{ severity: "referrer" }]),
    },
    { copy: true, withholdsConfirm: true, blocks: true },
  );
});
