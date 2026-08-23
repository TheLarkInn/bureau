import assert from "node:assert/strict";
import test from "node:test";

import { Readable } from "node:stream";

import { readStepRequest } from "./read-step-request.mjs";

test("reads a chunked step request from stdin", async () => {
  const original = process.stdin;
  const input = Readable.from(['{"inputs":', '{"source_commit":"abc"}}']);
  Object.defineProperty(process, "stdin", { value: input, configurable: true });
  try {
    assert.deepEqual(
      await readStepRequest(),
      { inputs: { source_commit: "abc" } },
    );
  } finally {
    Object.defineProperty(process, "stdin", {
      value: original,
      configurable: true,
    });
  }
});
