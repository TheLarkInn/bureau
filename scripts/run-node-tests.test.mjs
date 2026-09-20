import assert from "node:assert/strict";
import { test } from "node:test";
import { nodeTestFailure, runNodeTests } from "./run-node-tests.mjs";

const tap = [
  "TAP version 13", "ok 1 - example", "1..1",
  "# tests 1", "# pass 1", "# fail 0", "# cancelled 0", "# skipped 0", "# todo 0",
].join("\n");
const completed = { status: 0, signal: null, stdout: tap, stderr: "" };

test("Node gates reject failed processes even when TAP claims success", () => {
  const cases = [
    { status: 73 },
    { signal: "SIGKILL" },
    { error: new Error("output buffer exceeded") },
    { error: new Error() },
    { status: null },
  ];
  for (const failure of cases) {
    assert.ok(nodeTestFailure({ ...completed, ...failure }));
  }
});

test("Node gates require complete successful TAP, not merely the first pass", () => {
  const cases = [
    "",
    "TAP version 13\nok 1 - partial output",
    tap.replace("1..1", ""),
    tap.replace("# tests 1", "# tests 0"),
    tap.replace("# fail 0", "# fail 1"),
    tap.replace("# cancelled 0", "# cancelled 1"),
    tap.replace("# skipped 0", ""),
    tap.replace("# todo 0", ""),
    `${tap}\nnot ok 2 - failure hidden by an exit-zero runner`,
  ];
  for (const stdout of cases) {
    assert.ok(nodeTestFailure({ ...completed, stdout }));
  }
});

test("Node gates preserve platform skips while the site explicitly rejects them", () => {
  const skipped = { ...completed, stdout: tap.replace("# skipped 0", "# skipped 1") };
  const pending = { ...completed, stdout: tap.replace("# todo 0", "# todo 1") };
  assert.deepEqual([
    nodeTestFailure(completed), nodeTestFailure(completed, false),
    nodeTestFailure(skipped), nodeTestFailure(skipped, false),
    nodeTestFailure(pending, false),
  ], [null, null, null, "tests were skipped or left pending", "tests were skipped or left pending"]);
});

test("the shared runner bounds concurrency, time, and captured output", () => {
  let invocation;
  const sink = { write() {} };
  runNodeTests(["example.test.mjs"], {
    spawn(...args) { invocation = args; return completed; }, stdout: sink, stderr: sink,
  });
  assert.deepEqual(invocation[1],
    ["--test", "--test-concurrency=1", "--test-reporter=tap", "example.test.mjs"]);
  assert.deepEqual([invocation[2].timeout, invocation[2].maxBuffer], [900_000, 8 * 1024 * 1024]);
});

test("caller filenames cannot override the runner's process bounds", () => {
  for (const files of [[], ["--test-concurrency=31"], [null]]) {
    assert.throws(() => runNodeTests(files), /supply test files/u);
  }
});

test("the shared runner propagates interrupted output as a failed gate", () => {
  const sink = { write() {} };
  assert.throws(() => runNodeTests(["example.test.mjs"], {
    label: "probe", stdout: sink, stderr: sink,
    spawn: () => ({ ...completed, status: 73, stdout: "ok 1 - partial output\n" }),
  }), /probe: test process failed \(exit 73/u);
});
