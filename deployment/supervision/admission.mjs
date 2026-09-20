import { spawn } from "node:child_process";
import { createConnection } from "node:net";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { claim } from "./lease.mjs";
import { ID, exactKeys, frames, writeFrame } from "./protocol.mjs";
import { ENV } from "./system.mjs";

export function admissionRequest(value) {
  exactKeys(value, ["invocation", "pid"]);
  requireValue(typeof value.invocation === "string" && ID.test(value.invocation)
    && Number.isSafeInteger(value.pid) && value.pid > 1, "invalid pre-start identity");
  return value;
}

export function authenticatedRequest(packet, uid) {
  exactKeys(packet, ["id", "peerPid", "peerUid", "peerGid", "request"]);
  const request = admissionRequest(packet.request);
  requireValue(Number.isSafeInteger(uid) && uid > 0 && packet.peerUid === uid
    && Number.isSafeInteger(packet.peerPid) && packet.peerPid === request.pid
    && Number.isSafeInteger(packet.peerGid) && packet.peerGid >= 0, "pre-start socket peer does not match");
  return request;
}

export async function admissionServer(context, {
  consume = claim, launch = spawn, path = join(context.runtime, "admission.sock"),
} = {}) {
  const helper = fileURLToPath(new URL("./peer.py", import.meta.url));
  const child = launch("/usr/bin/python3", ["-I", "-S", "-u", helper, path],
    { env: ENV, stdio: ["pipe", "pipe", "pipe"] });
  const reader = frames(child.stdout);
  let failed = false;
  let closing = false;
  let diagnostics = 0;
  const failure = () => { failed = true; child.stdin.destroy(); child.kill("SIGTERM"); };
  const completed = new Promise((resolve) => child.once("close", resolve));
  child.on("error", failure).on("exit", () => { failed = true; });
  child.stdin.on("error", failure);
  child.stderr.on("error", failure).on("data", (chunk) => {
    diagnostics += chunk.length;
    if (diagnostics > 8192) failure();
  });
  const close = async () => {
    closing = true;
    reader.close();
    child.kill("SIGTERM");
    const deadline = setTimeout(() => child.kill("SIGKILL"), 2000);
    try { await completed; } finally { clearTimeout(deadline); }
  };
  try {
    const ready = await reader.read(2000);
    exactKeys(ready, ["ready"]);
    requireValue(ready.ready === true && !failed, "peer-credential helper did not become ready");
  } catch (error) {
    await close();
    throw error;
  }
  const receive = async () => {
    for (let sequence = 1; !closing; sequence += 1) {
      const packet = await reader.read();
      requireValue(packet.id === sequence, "peer-credential helper lost request ordering");
      let admitted = false;
      try {
        const request = authenticatedRequest(packet, context.uid);
        await consume({ ...context, ...request }, { live: () => !failed && !closing });
        admitted = !failed && !closing;
      } catch {
        // A refusal is delivered as EOF to the unprivileged pre-start, never
        // as a grant. The root helper emits only kernel credentials plus IDs.
      }
      await writeFrame(child.stdin, { id: sequence, admitted });
    }
  };
  receive().catch(failure);
  return {
    check() { requireValue(!failed && !closing, "native peer-credential listener failed"); },
    close,
  };
}

export async function requestAdmission(path, value, timeout = 2000) {
  admissionRequest(value);
  const socket = createConnection(path);
  const reader = frames(socket);
  try {
    const response = reader.read(timeout);
    const [result] = await Promise.all([response, writeFrame(socket, value)]);
    exactKeys(result, ["admitted"]);
    requireValue(result.admitted === true, "pre-start admission refused");
  } finally {
    reader.close();
    socket.destroy();
  }
}
