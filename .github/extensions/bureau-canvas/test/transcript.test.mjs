// The step-log parser (web/live/transcript.js). Fixtures are the shapes real
// runs produce: an agent's Copilot CLI transcript, a deterministic step's v2
// contract line, and a failed step's raw stderr.

import assert from "node:assert/strict";
import { test } from "node:test";

const { blocks, parseTranscript, runOutput, stepOutput } = await import("../web/live/transcript.js");

const TRANSCRIPT = [
  "● get_step_context (MCP: bureau-io)",
  '  └ {"schema":"v2","run_id":"r1"}',
  "",
  "I'm grounding the review in the",
  "product and architecture contracts.",
  "",
  "● Read DESIGN.md",
  "  │ .github/extensions/bureau-canvas/DESIGN.md",
  "  └ L1:300 (253 lines read)",
  "/ Search (glob)",
  '  │ "crates/**/*.rs"',
  "  └ 294 files found",
].join("\n");

const CONTRACT = '{"schema":"v2","outcome":"success","outputs":{"detector_exit":2},'
  + '"artifacts":[{"name":"detector.json","path":"target/detector.json"}],'
  + '"trust":"derived","message":"design detector completed"}';

const STACK = "Error: listing repository issues returned HTTP 504\n    at allIssues (file:///x.mjs:94:13)\n";

function event(step, data, atMs) {
  return { kind: "output", at_ms: atMs, data: { step, stream: "combined", data } };
}

test("reads an agent transcript as tool calls, prose and nothing else", () => {
  const parsed = blocks(TRANSCRIPT);

  assert.deepStrictEqual(parsed, [
    { kind: "tool", title: "get_step_context (MCP: bureau-io)", detail: [], result: '{"schema":"v2","run_id":"r1"}' },
    { kind: "note", text: "I'm grounding the review in the product and architecture contracts." },
    { kind: "tool", title: "Read DESIGN.md", detail: [".github/extensions/bureau-canvas/DESIGN.md"], result: "L1:300 (253 lines read)" },
    { kind: "tool", title: "Search (glob)", detail: ['"crates/**/*.rs"'], result: "294 files found" },
  ]);
});

test("reads a deterministic step's contract line as its outcome", () => {
  assert.deepStrictEqual(blocks(CONTRACT), [{
    kind: "result",
    outcome: "success",
    message: "design detector completed",
    outputs: { detector_exit: 2 },
    artifacts: [{ name: "detector.json", path: "target/detector.json" }],
    trust: "derived",
  }]);
});

test("keeps output that is not a transcript verbatim, and splits a swallowed tool call", () => {
  const cases = [
    // A stack trace must not be reflowed into prose.
    [STACK, [{ kind: "output", text: STACK.replace(/\s+$/u, "") }]],
    [" \n ", []],
    // The capture interleaves a warning with the start of a tool call.
    ["Warning: model opus is not ● Read a.md\n  └ 3 lines read", [
      { kind: "warning", text: "model opus is not" },
      { kind: "tool", title: "Read a.md", detail: [], result: "3 lines read" },
    ]],
  ];

  assert.deepStrictEqual(cases.map(([text]) => blocks(text)), cases.map(([, want]) => want));
});

test("collects one step's output in order, bounded by a replay position", () => {
  const events = [
    event("implement", "first ", 10),
    event("verify", "other ", 20),
    event("implement", "second ", 30),
    event("implement", "later", 90),
    { kind: "output", at_ms: 40, data: { stream: "run", data: "escalated" } },
  ];

  assert.deepStrictEqual(
    [stepOutput(events, "implement", 50), stepOutput(events, "implement"), runOutput(events, 50)],
    ["first second ", "first second later", "escalated"],
  );
});

test("a blank line separates paragraphs instead of joining them", () => {
  const parsed = parseTranscript("one\n\ntwo");

  assert.deepStrictEqual(parsed.map((block) => block.text), ["one", "", "two"]);
});
