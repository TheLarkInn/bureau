import assert from "node:assert/strict";
import test from "node:test";

import { readBoundedFile, readBoundedJson } from "./maintenance-files.mjs";

function fakeFile(text, reportedSize = Buffer.byteLength(text)) {
  const data = Buffer.from(text);
  const reads = [];
  let closed = false;
  return {
    reads, isClosed: () => closed,
    open: async () => ({
      stat: async () => ({ size: reportedSize, isFile: () => true }),
      async read(buffer, offset, length, position) {
        reads.push(length);
        const bytesRead = Math.min(length, data.length - position);
        data.copy(buffer, offset, position, position + bytesRead);
        return { bytesRead };
      },
      close: async () => { closed = true; },
    }),
  };
}

test("policy reads cap allocated bytes and close handles on success or oversize", async () => {
  const file = fakeFile('{"a":1}');
  assert.deepEqual(await readBoundedJson("fixture", 7, file.open), { a: 1 });
  assert.equal(file.isClosed(), true);
  const huge = fakeFile("{}", 10 ** 12);
  await assert.rejects(readBoundedFile("fixture", 8192, huge.open), /byte limit/u);
  assert.deepEqual([huge.reads.length, huge.isClosed()], [0, true]);
});

test("growth between stat and read cannot bypass a byte limit", async () => {
  const file = fakeFile("0123456789", 2);
  await assert.rejects(readBoundedFile("fixture", 4, file.open), /grew beyond/u);
  assert.deepEqual(file.reads, [5]);
  assert.equal(file.isClosed(), true);
});

test("limits count UTF-8 bytes, not UTF-16 code units", async () => {
  const file = fakeFile('"\u20ac\u20ac"');
  await assert.rejects(readBoundedJson("fixture", 7, file.open), /byte limit/u);
  const valid = fakeFile('"\u20ac\u20ac"');
  assert.equal(await readBoundedJson("fixture", 8, valid.open), "\u20ac\u20ac");
});
