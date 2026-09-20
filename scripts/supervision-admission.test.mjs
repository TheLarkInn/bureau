import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createServer } from "node:net";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";

import { admissionRequest, admissionServer, authenticatedRequest, requestAdmission } from "../deployment/supervision/admission.mjs";
import { ENV } from "../deployment/supervision/system.mjs";
import { ENGINE } from "./supervision-test-support.mjs";

const request = () => ({ invocation: ENGINE.invocation, pid: 321 });
const packet = () => ({ id: 1, peerPid: 321, peerUid: 1001, peerGid: 1001, request: request() });

test("kernel peer evidence, not public ControlPID JSON, authorizes the pre-start", () => {
  assert.deepEqual(authenticatedRequest(packet(), 1001), request());
  for (const patch of [{ peerPid: 322 }, { peerUid: 1002 }, { peerPid: "321" },
    { peerUid: [1001] }, { peerGid: null }]) {
    assert.throws(() => authenticatedRequest({ ...packet(), ...patch }, 1001), /peer/u);
  }
  for (const patch of [{ invocation: [ENGINE.invocation] }, { pid: "321" }, { pid: 0 }]) {
    assert.throws(() => admissionRequest({ ...request(), ...patch }), /identity/u);
  }
});

test("actual admission bridge refuses forged peer metadata without consuming a grant", async (t) => {
  let consumed = 0;
  const fixture = `
    process.stdout.write('{"ready":true}\\n');
    setTimeout(() => process.stdout.write(${JSON.stringify(`${JSON.stringify({ ...packet(), peerPid: 999 })}\n`)}), 30);
    process.stdin.on('data', data => {
      const value = JSON.parse(data);
      if (value.admitted !== false || value.id !== 1) process.exit(9);
    });
  `;
  const server = await admissionServer({ runtime: "unused", uid: 1001 }, {
    consume: async () => { consumed += 1; },
    launch(file, args, options) {
      assert.deepEqual([file, args.slice(0, 3), options.env], ["/usr/bin/python3", ["-I", "-S", "-u"], ENV]);
      const child = spawn(process.execPath, ["-e", fixture], options);
      t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill(); });
      return child;
    },
  });
  await new Promise((resolve) => setTimeout(resolve, 80));
  server.check();
  await server.close();
  assert.equal(consumed, 0);
});

test("real admission client uses one bounded request/response and handles a missing listener", async (t) => {
  const name = `bureau-admission-${randomBytes(8).toString("hex")}`;
  const path = process.platform === "win32" ? `\\\\.\\pipe\\${name}` : join(tmpdir(), `${name}.sock`);
  const server = createServer((socket) => {
    socket.once("data", (bytes) => {
      assert.deepEqual(JSON.parse(bytes), request());
      socket.end('{"admitted":true}\n');
    });
  });
  await new Promise((resolve) => server.listen(path, resolve));
  t.after(() => server.close());
  await requestAdmission(path, request());
  await new Promise((resolve) => server.close(resolve));
  await assert.rejects(requestAdmission(path, request()));
});

test("missing platform peer helper fails before readiness", async () => {
  await assert.rejects(admissionServer({ runtime: "unused", uid: 1001 }, {
    launch(file, args, options) { return spawn(join(tmpdir(), "missing-bureau-peer-helper"), [], options); },
  }), /closed|deadline/u);
});
