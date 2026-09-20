import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { constants } from "node:fs";
import { access } from "node:fs/promises";
import { promisify } from "node:util";

import { requireValue } from "./maintenance-contract.mjs";
import { readBoundedJson } from "./maintenance-files.mjs";
import { requestAdmission } from "../deployment/supervision/admission.mjs";
import { nativeEffects, supervise } from "../deployment/supervision/heartbeat.mjs";

const [role, engine, guard, runtime, mode, commit, executable, forgedPid] = process.argv.slice(2);
requireValue(/^bureau-supervision-test-[a-f0-9-]+-engine\.service$/u.test(engine ?? "")
  && guard === engine.replace("-engine.", "-guard.")
  && /^\/run\/bureau-supervision-test-[a-f0-9-]+$/u.test(runtime ?? ""),
"fixture must use unique test-owned units and runtime");

try {
  if (role === "forge") {
    requireValue(process.getuid() !== 0, "forged peer must use the service user");
    await assert.rejects(requestAdmission(`${runtime}/admission.sock`,
      { invocation: process.env.INVOCATION_ID, pid: Number(forgedPid) }));
    console.log("separate unprivileged forged peer refused");
  } else if (role === "claim") {
    requireValue(process.getuid() !== 0 && process.env.BUREAU_SYNTHETIC_REPORTER === undefined,
      "pre-start must remain unprivileged with a cleared reporter environment");
    await assert.rejects(access(runtime, constants.W_OK), { code: "EACCES" });
    await assert.rejects(readBoundedJson(`${runtime}/lease.json`), { code: "EACCES" });
    if (mode === "forged-peer") {
      const { stdout } = await promisify(execFile)(process.execPath,
        [process.argv[1], "forge", ...process.argv.slice(3), String(process.pid)], {
          env: { PATH: "/usr/bin:/bin", INVOCATION_ID: process.env.INVOCATION_ID },
          timeout: 4000, killSignal: "SIGKILL", maxBuffer: 4096,
        });
      requireValue(stdout.trim() === "separate unprivileged forged peer refused", "forged-peer proof missing");
    }
    await requestAdmission(`${runtime}/admission.sock`, { invocation: process.env.INVOCATION_ID, pid: process.pid });
  } else {
    requireValue(role === "heartbeat", "unknown fixture role");
    const effects = nativeEffects({ engine, guard, runtime, commit, executable,
      prepare: async () => { requireValue(mode !== "partial", "synthetic pre-readiness refusal"); } });
    await supervise({ input: process.stdin, output: process.stdout, commit, effects });
  }
} catch (error) {
  console.error(`isolated supervision fixture: ${error.message}`);
  process.exitCode = 1;
}
