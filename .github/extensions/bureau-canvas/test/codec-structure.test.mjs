// Structural edits: adding and removing whole steps, map entries and sequence
// items, and building a document from scratch (issue #44).
//
// The bar is diff size. The config repo is the authorization model, so a save
// that rewrites lines it did not need to touch makes the review PR useless.

import assert from "node:assert/strict";
import { readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { createDocument, createdStyle, parse, render } from "../lib/codec.mjs";

const root = fileURLToPath(new URL("../../../../", import.meta.url));
const pipelinePath = ".bureau/pipelines/agent-eligible-pipeline.yaml";
const reposPath = ".bureau/repos.yaml";
const crlfPath = fileURLToPath(new URL("./fixtures/codec-structure-crlf.yaml", import.meta.url));

/** Multiset difference, so an insertion does not read as every later line changing. */
function lineDelta(before, after) {
  const remove = (lines, taken) => lines.filter((line) => !taken.delete(line));
  const left = before.split("\n");
  const right = after.split("\n");
  return {
    added: remove(right, new Map(left.map((line, index) => [line, index])) && countOf(left)),
    removed: remove(left, countOf(right)),
  };
}

function countOf(lines) {
  const counts = new Map();
  for (const line of lines) {
    counts.set(line, (counts.get(line) ?? 0) + 1);
  }
  return {
    delete(line) {
      const count = counts.get(line) ?? 0;
      if (count > 0) {
        counts.set(line, count - 1);
        return true;
      }
      return false;
    },
  };
}

async function readPipeline() {
  const text = await readFile(join(root, pipelinePath), "utf8");
  const parsed = parse(text, { path: pipelinePath });
  return { text, parsed, view: structuredClone(parsed.view) };
}

test("adding a step adds only that step's lines", async () => {
  const { text, parsed, view } = await readPipeline();
  view.steps.push({
    id: "publish",
    name: "publish",
    type: "step",
    kind: "deterministic",
    order: view.steps.length,
    fields: { inputsFrom: [], maxAttempts: 1, run: "scripts/publish.sh" },
  });
  view.edges.push({
    id: "control:publish:success->terminal:done",
    source: "publish",
    target: "terminal:done",
    relation: "control",
    outcome: "success",
  });

  const delta = lineDelta(text, render(view, parsed.doc, parsed.style));

  assert.deepEqual(delta, {
    added: ["- name: publish", "  type: deterministic", "  run: scripts/publish.sh", "  next: done"],
    removed: [],
  });
});

test("removing a step removes only that step's lines", async () => {
  const { text, parsed, view } = await readPipeline();
  view.steps = view.steps.filter((step) => step.name !== "review");
  view.edges = view.edges.filter((edge) => edge.source !== "review" && edge.target !== "review");

  const delta = lineDelta(text, render(view, parsed.doc, parsed.style));

  assert.deepEqual({ added: delta.added, removedHas: delta.removed.includes("- name: review") }, { added: [], removedHas: true });
});

test("adding and removing a repo leaves other entries untouched", async () => {
  const text = await readFile(join(root, reposPath), "utf8");
  const parsed = parse(text, { path: reposPath });
  const added = structuredClone(parsed.view);
  added.value.repos.docs = { url: "https://example.invalid/docs.git", forge: "github", access: "read", credential: "docs-token" };
  const withRepo = render(added, parsed.doc, parsed.style);

  const reparsed = parse(withRepo, { path: reposPath });
  const removed = structuredClone(reparsed.view);
  delete removed.value.repos.docs;
  const withoutRepo = render(removed, reparsed.doc, reparsed.style);

  assert.deepEqual(
    {
      addedLines: lineDelta(text, withRepo),
      // Removing it again returns the file to exactly what it was.
      roundTrip: withoutRepo === text,
    },
    {
      addedLines: {
        added: ["  docs:", "    url: https://example.invalid/docs.git", "    forge: github", "    access: read", "    credential: docs-token"],
        removed: [],
      },
      roundTrip: true,
    },
  );
});

test("removing the last sequence item leaves an empty collection", async () => {
  const { parsed, view } = await readPipeline();
  view.steps = [];
  view.edges = [];

  const rendered = render(view, parsed.doc, parsed.style);

  assert.match(rendered, /^steps: \[\]$/mu);
});

test("a created document matches the committed style and round-trips", async () => {
  const text = createDocument({
    name: "reviewer",
    agent: "/bureau:reviewer",
    adapter: "copilot",
    permissions: ["repo:read", "model:invoke"],
    min_trust: "derived",
  });
  const parsed = parse(text, { path: "roles/reviewer.yaml" });

  assert.deepEqual(
    {
      text,
      roundTrip: render(structuredClone(parsed.view), parsed.doc, parsed.style) === text,
    },
    {
      // Sequence dashes flush under their key, as serde_yaml_ng writes them.
      text: "name: reviewer\nagent: /bureau:reviewer\nadapter: copilot\npermissions:\n- repo:read\n- model:invoke\nmin_trust: derived\n",
      roundTrip: true,
    },
  );
});

test("structural edits preserve CRLF", async () => {
  const source = (await readFile(join(root, pipelinePath), "utf8")).replaceAll("\r\n", "\n").replaceAll("\n", "\r\n");
  await writeFile(crlfPath, source);
  try {
    const onDisk = await readFile(crlfPath, "utf8");
    const parsed = parse(onDisk, { path: pipelinePath });
    const view = structuredClone(parsed.view);
    view.steps = view.steps.filter((step) => step.name !== "review");
    view.edges = view.edges.filter((edge) => edge.source !== "review" && edge.target !== "review");
    const rendered = render(view, parsed.doc, parsed.style);

    assert.deepEqual(
      { crlf: rendered.includes("\r\n"), strayLf: /[^\r]\n/u.test(rendered) },
      { crlf: true, strayLf: false },
    );
  } finally {
    await rm(crlfPath, { force: true });
  }
});
