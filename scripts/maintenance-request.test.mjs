import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { validateEvidence } from "./maintenance-contract.mjs";
import { EXPECTED_REQUEST, normalizeRequest, readPublisherRequest } from "./maintenance-request.mjs";
import { fixture } from "./maintenance-test-support.mjs";

const HELPER = fileURLToPath(new URL("./maintenance-publish.mjs", import.meta.url));

function stepInputs() {
  const { value } = fixture("chaos", []);
  return {
    maintenance_source: value.source, maintenance_observed_at: "2026-09-17T12:00:00Z",
    maintenance_evidence: value,
  };
}

// Exactly what the engine serializes for the step (crates/bureau/src/contract.rs StepRequest).
function stepRequest(inputs = stepInputs()) {
  return {
    schema: "v2", run_id: "r1", step: "report-clean", worktree: "/w",
    item: { forge: "github", external_id: "TheLarkInn/bureau#131" }, trust: "trusted", inputs, artifacts: {},
  };
}

// Exactly what bureau-io.get_step_context returns (crates/bureau/src/mcp/tools.rs `context`).
function toolResult(request = stepRequest()) {
  return { content: [{ type: "text", text: JSON.stringify(request) }], isError: false };
}

test("every accepted shape normalizes to the same validated inputs", () => {
  const inputs = stepInputs();
  const shapes = [
    ["the v2 step request", stepRequest(inputs)],
    ["the get_step_context result", toolResult(stepRequest(inputs))],
    ["the inputs object at top level", inputs],
    ["an inputs projection", { inputs }],
  ];
  const normalized = shapes.map(([name, shape]) => [name, normalizeRequest(shape, "clear")]);
  assert.deepEqual(normalized, shapes.map(([name]) => [name, { schema: "v2", inputs }]));
});

test("handoff keeps its draft receipt and observation time from every shape", () => {
  const inputs = { ...stepInputs(), maintenance_draft: { disposition: "open" } };
  const shapes = [stepRequest(inputs), toolResult(stepRequest(inputs)), inputs];
  const kept = shapes.map((shape) => normalizeRequest(shape, "handoff").inputs);
  assert.deepEqual(kept, [inputs, inputs, inputs]);
});

const REFUSALS = [
  ["missing evidence", {}, "clear", "missing inputs: maintenance_evidence"],
  ["an unrelated object", { run_id: "r1" }, "draft", "missing inputs: maintenance_evidence"],
  ["handoff without draft", stepInputs(), "handoff", "missing inputs: maintenance_draft"],
  ["a foreign schema", { schema: "x" }, "handoff", "publisher requires a v2 request"],
  ["a v1 request", { ...stepRequest(), schema: "v1" }, "clear", "publisher requires a v2 request"],
  ["a v2 request without inputs", { schema: "v2" }, "clear", "inputs must be an object, got undefined"],
  ["an array", [stepRequest()], "clear", "got array"],
  ["null", null, "clear", "got null"],
  ["a JSON string", JSON.stringify(stepRequest()), "clear", "got string"],
  ["ambiguous keys", { ...stepRequest(), maintenance_evidence: {} }, "clear",
    "maintenance_* keys appear both inside and outside inputs"],
  ["an error tool result", { ...toolResult(), isError: true }, "clear",
    "a get_step_context result must be a successful result with exactly one text item"],
  ["two text items", { content: [...toolResult().content, ...toolResult().content] }, "clear",
    "a get_step_context result must be a successful result with exactly one text item"],
  ["a nested tool result", toolResult(toolResult()), "clear", "missing inputs: maintenance_evidence"],
];

test("unrecognized shapes name the expected request and what is missing", () => {
  const errors = REFUSALS.map(([name, shape, mode]) => {
    try {
      normalizeRequest(shape, mode);
      return [name, "accepted"];
    } catch (error) {
      return [name, error.message];
    }
  });
  assert.deepEqual(errors, REFUSALS.map(([name, , , detail]) => [name, `${EXPECTED_REQUEST}; ${detail}`]));
});

test("the exact refusal text names both accepted shapes", () => {
  assert.equal(EXPECTED_REQUEST, "request must be the v2 step request "
    + "{schema:\"v2\", inputs:{maintenance_evidence,...}} or the get_step_context result");
});

test("non-JSON input and unknown operations fail before anything else", async () => {
  const text = { content: [{ type: "text", text: "not json" }] };
  const unread = () => assert.fail("stdin must not be read for an unknown operation");
  const outcomes = await Promise.allSettled([
    readPublisherRequest("clear", async () => JSON.parse("{")),
    readPublisherRequest("clear", async () => text),
    readPublisherRequest("publish", unread),
  ]);
  assert.deepEqual(outcomes.map((outcome) => outcome.reason.message.replace(/: .*JSON.*$/u, ": <parse>")), [
    `${EXPECTED_REQUEST}; stdin is not JSON: <parse>`,
    `${EXPECTED_REQUEST}; the get_step_context text is not JSON: <parse>`,
    "publisher operation must be draft, handoff, or clear",
  ]);
});

test("normalization leaves evidence validation unchanged", () => {
  const incomplete = { ...stepInputs().maintenance_evidence, complete: false };
  const tampered = { ...stepInputs().maintenance_evidence, schema: "other" };
  for (const evidence of [incomplete, tampered, null]) {
    const shapes = [stepRequest({ maintenance_evidence: evidence }), { maintenance_evidence: evidence }];
    for (const shape of shapes) {
      assert.throws(() => validateEvidence(normalizeRequest(shape, "clear").inputs.maintenance_evidence),
        /^Error: missing or incomplete deterministic evidence$/u);
    }
  }
});

test("the helper reports an unrecognized request as a blocked v2 result", () => {
  const run = spawnSync(process.execPath, [HELPER, "clear"], {
    input: JSON.stringify({ run_id: "r1" }), encoding: "utf8", timeout: 10_000,
    env: { PATH: process.env.PATH },
  });
  assert.deepEqual([run.status, JSON.parse(run.stdout)], [1, {
    schema: "v2", outcome: "blocked", outputs: {}, artifacts: [], trust: "derived",
    message: `${EXPECTED_REQUEST}; missing inputs: maintenance_evidence`,
  }]);
});
