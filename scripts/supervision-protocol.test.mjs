import assert from "node:assert/strict";
import { PassThrough, Writable } from "node:stream";
import test from "node:test";

import { FRAME_BYTES, frames, heartbeat, identity } from "../deployment/supervision/protocol.mjs";
import { COMMIT, ENGINE, GUARD, OWNER, harness, reply } from "./supervision-test-support.mjs";
import { supervise } from "../deployment/supervision/heartbeat.mjs";

test("actual heartbeat loop admits only after stdin ownership, then supervises until EOF", async () => {
  const seen = [];
  const run = harness((challenge, input, { events }) => {
    seen.push(challenge);
    if (challenge.sequence === 0) assert.deepEqual(events, ["prepare"]);
    if (challenge.sequence === 3) input.end();
    else input.write(`${JSON.stringify(reply(challenge))}\n`);
  });
  await assert.rejects(run.running, /closed/u);
  assert.deepEqual([seen.map((value) => value.state), run.events],
    [["starting", "running", "running", "running"],
      ["prepare", "fresh", "READY=1", "fresh", "WATCHDOG=1", "fresh", "WATCHDOG=1"]]);
});

test("stale, frozen and partial writers expire before admission without ready", async () => {
  for (const write of [() => {}, (input) => input.write('{"schema":')]) {
    const run = harness((challenge, input) => write(input), { limits: { response: 15 } });
    await assert.rejects(run.running, /deadline/u);
    assert.deepEqual(run.events, ["prepare"]);
  }
});

test("malformed, oversized, extra and buffered input is refused by the actual reader", async () => {
  for (const payload of ["broken\n", `${"x".repeat(FRAME_BYTES + 1)}\n`, "{}\n{}\n", "\uFFFD\n"]) {
    const run = harness((challenge, input) => input.write(payload));
    await assert.rejects(run.running, /malformed|oversized|invalid/u);
    assert.deepEqual(run.events, ["prepare"]);
  }
});

test("unsolicited buffered heartbeats cannot keep a frozen owner alive", async () => {
  const input = new PassThrough();
  const reader = frames(input);
  input.write("{}\n");
  await assert.rejects(reader.read(30), /unsolicited/u);
  reader.close();
});

test("every heartbeat binds the nonce, sequence, owner, guard, commit and sample age", () => {
  const challenge = { nonce: "5".repeat(32), sequence: 1, guard: GUARD.invocation };
  assert.equal(heartbeat(reply(challenge), challenge, OWNER, COMMIT), OWNER);
  for (const patch of [{ nonce: "0" }, { sequence: 0 }, { guard: OWNER }, { owner: GUARD.invocation },
    { commit: "0".repeat(40) }, { ageMs: 2001 }, { ageMs: -1 }, { extra: true }]) {
    assert.throws(() => heartbeat(reply(challenge, patch), challenge, OWNER, COMMIT), /invalid|stale/u);
  }
});

test("running identity cannot be absent, unknown, malformed or an escaping cgroup", () => {
  for (const value of [null, {}, { ...ENGINE, pid: 0 }, { ...ENGINE, starttime: "" },
    { ...ENGINE, invocation: "" }, { ...ENGINE, cgroup: "/system.slice/../unrelated.service" }]) {
    assert.throws(() => identity(value), /identity|fields/u);
  }
});

test("JSON arrays and other nonstrings cannot impersonate textual identity fields", () => {
  for (const field of ["invocation", "starttime", "cgroup"]) {
    for (const value of [[ENGINE[field]], {}, null, true, 123]) {
      assert.throws(() => identity({ ...ENGINE, [field]: value }), /identity/u);
    }
  }
});

test("malformed first-heartbeat owner types never reach publication or READY", async () => {
  for (const value of [[OWNER], {}, null, true, 123]) {
    const run = harness((challenge, input) => input.write(`${JSON.stringify(reply(challenge, { owner: value }))}\n`));
    await assert.rejects(run.running, /invalid/u);
    assert.deepEqual(run.events, ["prepare"]);
  }
});

test("fresh heartbeat cannot preserve a changed running identity", async () => {
  for (const patch of [{ invocation: OWNER }, { pid: 790 }, { starttime: "1012" },
    { cgroup: "/system.slice/replacement.service" }]) {
    const run = harness((challenge, input, state) => {
      if (challenge.sequence === 1) state.setState({ state: "running", identity: { ...ENGINE, ...patch } });
      input.write(`${JSON.stringify(reply(challenge))}\n`);
    });
    await assert.rejects(run.running, /identity changed/u);
    assert.deepEqual(run.events, ["prepare", "fresh", "READY=1"]);
  }
});

test("running with unknown identity and running-to-starting both refuse while writer is fresh", async () => {
  for (const value of [{ state: "running", identity: null }, { state: "starting", identity: null }]) {
    const run = harness((challenge, input, state) => {
      if (challenge.sequence === 1) state.setState(value);
      input.write(`${JSON.stringify(reply(challenge))}\n`);
    });
    await assert.rejects(run.running, /fields|identity disappeared/u);
  }
});

test("startup never manufactures an identity and has an absolute deadline", async () => {
  const run = harness((challenge, input, state) => {
    state.setState({ state: "starting", identity: null });
    input.write(`${JSON.stringify(reply(challenge))}\n`);
  }, { limits: { startup: 10 } });
  await assert.rejects(run.running, /startup expired/u);
});

test("partial startup and monitor failures stop the actual loop without later watchdog acknowledgement", async () => {
  for (const name of ["prepare", "fresh", "notify", "observe"]) {
    const run = harness((challenge, input) => input.write(`${JSON.stringify(reply(challenge))}\n`),
      { effects: { [name]: async () => { throw new Error(`failed ${name}`); } } });
    await assert.rejects(run.running, new RegExp(`failed ${name}`, "u"));
    assert.equal(run.events.includes("WATCHDOG=1"), false);
  }
});

test("a lost Windows output reader refuses before readiness without an unhandled stream error", async () => {
  const output = new Writable({ write(chunk, encoding, callback) { callback(new Error("closed reader")); } });
  await assert.rejects(supervise({ input: new PassThrough(), output, commit: COMMIT,
    effects: { now: () => 0, prepare: async () => GUARD },
  }), /closed/u);
});
