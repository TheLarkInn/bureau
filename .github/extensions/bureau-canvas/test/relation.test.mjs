// relationView: the read-only config relation graph (Q16).

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { relationView } from "../lib/view.mjs";

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
