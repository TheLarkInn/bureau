import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { PassThrough } from "node:stream";
import test from "node:test";

import { transaction } from "../deployment/supervision/transaction.mjs";
import { ENV } from "../deployment/supervision/system.mjs";
import { SCHEMA } from "../deployment/supervision/protocol.mjs";
import { COMMIT, ENGINE, GUARD } from "./supervision-test-support.mjs";

const challenge = { schema: SCHEMA, type: "challenge", nonce: "5".repeat(32),
  guard: GUARD.invocation, sequence: 0, state: "running", identity: ENGINE };

async function runChild(t, script, receive = () => {}, manager = "") {
  const input = new PassThrough();
  const output = new PassThrough();
  const events = [];
  output.on("data", (data) => { events.push(JSON.parse(data)); receive(events.at(-1), input); });
  let launched = 0;
  const running = transaction({ input, output, commit: COMMIT, prepare: async () => {}, root: async () => manager,
    empty: async () => { events.push("empty"); }, finish: async (engine, owned) => { events.push({ drained: owned }); },
    launch(file, args, options) {
      assert.equal(file, "/usr/bin/systemd-run");
      assert.deepEqual(options.env, ENV);
      launched += 1;
      const child = spawn(process.execPath, ["--input-type=module", "-e", script], options);
      t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill(); });
      return child;
    } });
  return { running, input, events, launched: () => launched };
}

test("actual native transaction relays foreground IO and confirms drain before returning refusal", async (t) => {
  const script = `process.stdin.resume(); process.stdout.write(${JSON.stringify(`${JSON.stringify(challenge)}\n`)});
    process.stdin.on('end', () => { process.exitCode = 1; });`;
  const child = await runChild(t, script, (frame, input) => { if (frame.type === "challenge") input.end(); });
  await assert.rejects(child.running, /ownership ended/u);
  assert.deepEqual(child.events.slice(-3), [{ drained: ENGINE }, "empty", { schema: SCHEMA, type: "stopped", drained: true }]);
  assert.equal(child.launched(), 1);
});

test("no-identity successful client exit cannot manufacture successful admission", async (t) => {
  const child = await runChild(t, "process.exitCode = 0;");
  await assert.rejects(child.running, /ownership ended/u);
  assert.equal(child.events.some((event) => event.type === "challenge"), false);
});

test("healthy owned client exit returns only after the actual transaction drain path", async (t) => {
  const child = await runChild(t, `process.stdout.write(${JSON.stringify(`${JSON.stringify(challenge)}\n`)});`);
  await child.running;
  assert.deepEqual(child.events.at(-1), { schema: SCHEMA, type: "stopped", drained: true });
});

test("actual foreground output handling rejects malformed, oversize and identity-changing output", async (t) => {
  for (const text of ["invalid\n", `${"x".repeat(4096)}\n`,
    `${JSON.stringify({ ...challenge, identity: null })}\n`]) {
    const child = await runChild(t, `process.stdout.write(${JSON.stringify(text)}); process.exitCode = 1;`);
    await assert.rejects(child.running, /ownership ended/u);
    assert.equal(child.events.some((event) => event.type === "challenge"), false);
  }
});

test("partial startup error still executes the real transaction drain", async (t) => {
  const child = await runChild(t, "process.stderr.write('synthetic\\nstartup refusal\\n'); process.exitCode = 1;");
  await assert.rejects(child.running, { message: /ownership ended/u, native: "synthetic startup refusal" });
  assert.equal(child.events.some((event) => event.type === "stopped" && event.drained), true);
});

test("bounded native diagnostics cannot turn a flooding client into successful ownership", async (t) => {
  const script = `process.stderr.write('x'.repeat(70000));
    process.stdout.write(${JSON.stringify(`${JSON.stringify(challenge)}\n`)});`;
  const child = await runChild(t, script);
  await assert.rejects(child.running, (error) => /ownership ended/u.test(error.message)
    && error.native === "x".repeat(512));
});

test("native identity must sit under the manager root observed before launch", async (t) => {
  const wsl = "/wsl-user/distro-4668/systemd";
  const frame = { ...challenge, identity: { ...ENGINE, cgroup: `${wsl}${ENGINE.cgroup}` } };
  const script = `process.stdout.write(${JSON.stringify(`${JSON.stringify(frame)}\n`)});`;
  const accepted = await runChild(t, script, () => {}, wsl);
  await accepted.running;
  for (const manager of ["", "/wsl-user/distro-3850/systemd"]) {
    const refused = await runChild(t, script, () => {}, manager);
    await assert.rejects(refused.running, /ownership ended/u);
    assert.equal(refused.events.some((event) => event.type === "challenge"), false);
  }
});

test("pre-admission refusal and native overlap never launch a transaction", async () => {
  for (const failure of ["prepare", "root", "empty"]) {
    let launched = false;
    await assert.rejects(transaction({ input: new PassThrough(), output: new PassThrough(), commit: COMMIT,
      prepare: async () => {}, empty: async () => {}, root: async () => "",
      [failure]: async () => { throw new Error("refused"); },
      launch: () => { launched = true; },
    }), /refused/u);
    assert.equal(launched, false);
  }
});

test("drain refusal is never converted into a stopped acknowledgement", async (t) => {
  const output = new PassThrough();
  let bytes = 0;
  output.on("data", (data) => { bytes += data.length; });
  await assert.rejects(transaction({ input: new PassThrough(), output, commit: COMMIT,
    prepare: async () => {}, empty: async () => {}, root: async () => "",
    finish: async () => { throw new Error("changed identity during shutdown"); },
    launch(file, args, options) {
      const child = spawn(process.execPath, ["-e", "process.exitCode = 1"], options);
      t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill(); });
      return child;
    },
  }), /changed identity/u);
  assert.equal(bytes, 0);
});
