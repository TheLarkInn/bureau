import assert from "node:assert/strict";
import { readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { parse, render } from "../lib/codec.mjs";

const root = fileURLToPath(new URL("../../../../", import.meta.url));
const crlfPath = fileURLToPath(new URL("./fixtures/codec-generated-crlf.yaml", import.meta.url));
const yamlFiles = [
  ".bureau/repos.yaml",
  ".bureau/roles/implementer.yaml",
  ".bureau/roles/reviewer.yaml",
  ".bureau/assignments/agent-eligible.yaml",
  ".bureau/pipelines/agent-eligible-pipeline.yaml",
  ".github/extensions/bureau-canvas/test/fixtures/codec-reference-pipeline.yaml",
  ".github/extensions/bureau-canvas/test/fixtures/codec-absent-outcome.yaml",
];

test("round-trips committed and reference YAML byte for byte with LF", async () => {
  for (const path of yamlFiles) {
    const text = await readFile(join(root, path), "utf8");
    assert.equal(roundTrip(text, path), text, path);
  }
});

test("round-trips real CRLF files byte for byte", async () => {
  for (const path of yamlFiles) {
    const text = (await readFile(join(root, path), "utf8")).replaceAll("\r\n", "\n").replaceAll("\n", "\r\n");
    await writeFile(crlfPath, text);
    try {
      const onDisk = await readFile(crlfPath, "utf8");
      assert.equal(roundTrip(onDisk, path), onDisk, path);
    } finally {
      await rm(crlfPath, { force: true });
    }
  }
});

test("one edge rewire changes exactly one line", async () => {
  const path = ".bureau/pipelines/agent-eligible-pipeline.yaml";
  const text = await readFile(join(root, path), "utf8");
  const parsed = parse(text, { path });
  const view = structuredClone(parsed.view);
  const edge = view.edges.find((candidate) => candidate.source === "verify" && candidate.outcome === "failure");
  edge.target = "terminal:done";
  const rendered = render(view, parsed.doc, parsed.style);

  assert.deepEqual(changedLines(text, rendered), [[72, "  on_failure: escalate", "  on_failure: done"]]);
});

test("comments and flow style survive", async () => {
  const path = ".github/extensions/bureau-canvas/test/fixtures/codec-reference-pipeline.yaml";
  const rendered = roundTrip(await readFile(join(root, path), "utf8"), path);

  assert.deepEqual(
    [
      rendered.includes("# Hand-authored sparse pipeline fixture."),
      rendered.includes("inputs_from: [reproduce]"),
      rendered.includes("on: {success: done, failure: propose, blocked: escalate, no-work: done}"),
    ],
    [true, true, true],
  );
});

test("absent outcomes stay absent rather than becoming abort edges", async () => {
  const path = ".github/extensions/bureau-canvas/test/fixtures/codec-absent-outcome.yaml";
  const parsed = parse(await readFile(join(root, path), "utf8"), { path });

  assert.deepEqual(
    {
      failureEdge: parsed.view.edges.some((edge) => edge.outcome === "failure" && edge.target === "terminal:abort"),
      renderedHasFailure: render(parsed.view, parsed.doc, parsed.style).includes("on_failure"),
    },
    { failureEdge: false, renderedHasFailure: false },
  );
});

test("existing data inputs are edited structurally and omitted when emptied", async () => {
  const path = ".github/extensions/bureau-canvas/test/fixtures/codec-reference-pipeline.yaml";
  const parsed = parse(await readFile(join(root, path), "utf8"), { path });
  const view = structuredClone(parsed.view);
  view.steps.find((step) => step.name === "propose").fields.inputsFrom = [];
  const reparsed = parse(render(view, parsed.doc, parsed.style), { path });

  assert.deepEqual(reparsed.view.steps.find((step) => step.name === "propose").fields.inputsFrom, []);
});

test("existing concurrent fields persist without replacing the step", () => {
  const path = "pipelines/concurrent.yaml";
  const text = "name: concurrent\nsteps:\n- name: a\n  type: deterministic\n  run: 'true'\n- name: b\n  type: deterministic\n  run: 'true'\n- name: group\n  type: concurrent\n  steps: [a, b]\n  completion: all\n";
  const parsed = parse(text, { path });
  const view = structuredClone(parsed.view);
  const fields = view.steps.find((step) => step.name === "group").fields;
  Object.assign(fields, { members: ["a"], completion: "stop_on_failure", maxConcurrent: 4 });
  const group = parse(render(view, parsed.doc, parsed.style), { path }).view.steps.find((step) => step.name === "group");

  assert.deepEqual(
    { members: group.fields.members, completion: group.fields.completion, maxConcurrent: group.fields.maxConcurrent },
    { members: ["a"], completion: "stop_on_failure", maxConcurrent: 4 },
  );
});

test("path stem and declared name mismatch is reported", async () => {
  const text = await readFile(join(root, ".github/extensions/bureau-canvas/test/fixtures/codec-reference-pipeline.yaml"), "utf8");

  assert.deepEqual(parse(text, { path: "pipelines/wrong-name.yaml" }).view.file, {
    path: "pipelines/wrong-name.yaml",
    key: "wrong-name",
    declaredName: "codec-reference-pipeline",
    nameMismatch: true,
  });
});

function roundTrip(text, path) {
  const parsed = parse(text, { path });
  return render(parsed.view, parsed.doc, parsed.style);
}

function changedLines(before, after) {
  const left = before.split("\n");
  const right = after.split("\n");
  const changes = [];
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    if (left[index] !== right[index]) {
      changes.push([index + 1, left[index], right[index]]);
    }
  }
  return changes;
}
