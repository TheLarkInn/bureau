import assert from "node:assert/strict";
import { test } from "node:test";

import { readFactoryControls } from "../lib/factory-controls.mjs";
import { parseFactoryControls } from "../web/live/factory-controls.mjs";

const CONTROL = { session_id: "sdk-session", event_seq: 9, allowed: true, reason: null };
const payload = (control = CONTROL) => ({ run_id: "bureau-run", local_factory_resume: control });
const output = (control = CONTROL) => ({ code: 0, stdout: JSON.stringify({
  state: { run_id: "bureau-run" }, local_factory_resume: control,
}), stderr: "" });

test("reads native eligibility through the explicit read-only state projection", async () => {
  const calls = [];
  const actual = await readFactoryControls("bureau-run", "/private/runs", {
    exec: (args) => { calls.push(args); return output(); },
  });
  assert.deepEqual(actual, payload());
  assert.deepEqual(calls, [["show", "bureau-run", "--json", "--runs", "/private/runs"]]);
});

test("absent or incompatible CLI never becomes a raw-log approval fallback", async () => {
  const cases = [
    [null, /binary unavailable/u],
    [{ code: 2, stdout: "", stderr: "local --json requires --events" }, /could not verify/u],
    [{ code: 0, stdout: "[]", stderr: "" }, /different or missing run/u],
    [{ code: 0, stdout: "invalid", stderr: "" }, /invalid state JSON/u],
  ];
  for (const [result, pattern] of cases) {
    await assert.rejects(readFactoryControls("bureau-run", "/private/runs", { exec: () => result }), pattern);
  }
});

test("valid refusals and inactive factory state retain their exact meanings", () => {
  const refused = { ...CONTROL, allowed: false, reason: "Factory admission is indeterminate." };
  assert.deepEqual(parseFactoryControls(payload(refused), "bureau-run"), refused);
  assert.equal(parseFactoryControls(payload(null), "bureau-run"), null);
});

test("malformed, wrong-run and contradictory control projections are rejected", () => {
  const cases = [
    {}, { ...payload(), run_id: "another-run" },
    payload({ ...CONTROL, allowed: "true" }),
    payload({ ...CONTROL, event_seq: Number.MAX_SAFE_INTEGER + 1 }),
    payload({ ...CONTROL, event_seq: null }),
    payload({ ...CONTROL, allowed: false }),
    payload({ ...CONTROL, reason: "Cannot resume." }),
  ];
  for (const candidate of cases) {
    assert.throws(() => parseFactoryControls(candidate, "bureau-run"), /Bureau returned/u);
  }
});
