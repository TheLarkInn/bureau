import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { blocksDelete, referrers } from "../lib/preflight.mjs";

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
