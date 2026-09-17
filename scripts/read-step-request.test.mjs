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

    test("an explicit limit counts bytes across chunks before accumulating the request", async () => {
      const original = process.stdin;
      const text = '{"value":"\u20ac"}';
      Object.defineProperty(process, "stdin", {
        value: Readable.from([text.slice(0, 9), text.slice(9)]), configurable: true,
      });
      try {
        await assert.rejects(readStepRequest({ maximumBytes: text.length }), /byte limit/u);
      } finally {
        Object.defineProperty(process, "stdin", { value: original, configurable: true });
      }
    });

    test("an exact byte limit is accepted and invalid limits fail before reading", async () => {
      const original = process.stdin;
      const text = '{"value":"\u20ac"}';
      Object.defineProperty(process, "stdin", { value: Readable.from([text]), configurable: true });
      try {
        await assert.rejects(readStepRequest({ maximumBytes: 0 }), /positive integer/u);
        assert.deepEqual(await readStepRequest({ maximumBytes: Buffer.byteLength(text) }), { value: "\u20ac" });
      } finally {
        Object.defineProperty(process, "stdin", { value: original, configurable: true });
      }
    });
  }
});
