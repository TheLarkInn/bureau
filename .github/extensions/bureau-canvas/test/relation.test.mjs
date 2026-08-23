// relationView: the read-only config relation graph (Q16).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { referrers } from "../lib/preflight.mjs";
import { configView, relationView } from "../lib/view.mjs";

const committedUrl = new URL("./fixtures/committed-payload.json", import.meta.url);

async function fixture() {
  return JSON.parse(await readFile(committedUrl, "utf8"));
}

test("relation graph links assignments to pipeline and repos, then pipeline to effective roles", async () => {
  const graph = relationView(await fixture());

  assert.deepEqual(
    {
      nodes: graph.nodes.map((node) => node.id).sort(),
      edges: graph.edges.map((edge) => edge.id).sort(),
    },
    {
      nodes: ["assignment:agent-eligible", "pipeline:agent-eligible-pipeline", "repo:bureau", "role:implementer", "role:reviewer"],
      edges: [
        "pipeline:assignment:agent-eligible->pipeline:agent-eligible-pipeline",
        "repo:assignment:agent-eligible->repo:bureau",
        "role:pipeline:agent-eligible-pipeline->role:implementer",
        "role:pipeline:agent-eligible-pipeline->role:reviewer",
      ],
    },
  );
});

test("relation graph drops edges to entities the config lacks", async () => {
  const payload = await fixture();
  payload.config.roles = {};

  const graph = relationView(payload);

  assert.deepEqual(
    { nodes: graph.nodes.length, edges: graph.edges.map((edge) => edge.relation).sort() },
    { nodes: 3, edges: ["pipeline", "repo"] },
  );
});

test("relation graph of an empty config is empty", () => {
  assert.deepEqual(relationView({ ok: false, config: null }), { nodes: [], edges: [] });
});

/**
 * A role named by a deterministic step is a reference, and all three
 * projections have to say so. They read the same config by three different
 * routes — `usedBy` off the raw steps, the graph off the raw steps, preflight
 * off `pipelineView`'s `fields` — and a filter in any one of them makes the
 * canvas contradict itself: the strip calls a role unreferenced while delete
 * is blocked, or the graph draws it as an edgeless card while the strip does
 * not list it. `bureau validate` rejects such a config; drawing an invalid
 * config is what the findings are for, so the canvas still has to draw it.
 */
test("a role on a deterministic step is referenced by usedBy, the graph and preflight alike", async () => {
  const payload = await fixture();
  payload.config.roles.linter = { ...payload.config.roles.reviewer, name: "linter", agent: "/bureau:linter" };
  payload.config.pipelines["agent-eligible-pipeline"].steps[1].role = "linter";

  const role = configView(payload).roles.find((item) => item.name === "linter");
  const edges = relationView(payload).edges.map((edge) => edge.id);
  const blocked = referrers(payload, "role", "linter").map((item) => item.name);

  assert.deepEqual(
    { usedBy: role.usedBy, edge: edges.includes("role:pipeline:agent-eligible-pipeline->role:linter"), blocked },
    {
      usedBy: ["pipeline:agent-eligible-pipeline/verify"],
      edge: true,
      blocked: ["agent-eligible-pipeline/verify"],
    },
  );
});
