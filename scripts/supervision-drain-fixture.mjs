import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { requireValue } from "./maintenance-contract.mjs";
import { supervise } from "../deployment/supervision/heartbeat.mjs";
import { transaction } from "../deployment/supervision/transaction.mjs";
import { clock } from "../deployment/supervision/lease.mjs";
import { COMMIT } from "../deployment/supervision/protocol.mjs";

const [role, commit, mode = "drained"] = process.argv.slice(2);
requireValue(typeof commit === "string" && COMMIT.test(commit)
  && ["heartbeat", "client"].includes(role) && ["drained", "unconfirmed"].includes(mode),
"supply the bounded offline drain fixture role, commit and mode");
const guard = { invocation: "a".repeat(32), pid: 1001, starttime: "100",
  cgroup: "/system.slice/fixture-guard.service" };
const engine = { invocation: "b".repeat(32), pid: 1002, starttime: "101",
  cgroup: "/system.slice/fixture-engine.service" };

try {
  if (role === "heartbeat") {
    await supervise({ input: process.stdin, output: process.stdout, commit,
      limits: { response: 2000, interval: 20, startup: 5000 },
      effects: {
        now: clock, wait: sleep, prepare: async () => guard, root: async () => "",
        observe: async () => ({ state: "running", identity: engine }),
        fresh: async () => {}, notify: async () => {},
      },
    });
  } else {
    await transaction({ input: process.stdin, output: process.stdout, commit,
      prepare: async () => {}, empty: async () => {}, root: async () => "",
      finish: async () => { requireValue(mode === "drained", "synthetic unconfirmed cgroup"); },
      launch(file, args, options) {
        requireValue(file === "/usr/bin/systemd-run", "unexpected fixture launch");
        return spawn(process.execPath, [fileURLToPath(import.meta.url), "heartbeat", commit, mode], options);
      },
    });
  }
} catch {
  console.error("offline native fixture refused");
  process.exitCode = 1;
}
