import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { clock } from "./lease.mjs";
import { NATIVE_BUDGET } from "./budget.mjs";
import { FRAME_BYTES, SCHEMA, LIMITS, sameIdentity, identity, writeFrame } from "./protocol.mjs";
import { ENGINE, ENV, GUARD, ROOT, RUNTIME, drainProcessIdentity, emptyService, installation, serviceState } from "./system.mjs";

export function unitArguments({ engine = ENGINE, guard = GUARD, runtime = RUNTIME,
  script = `${ROOT}/deployment/supervision/heartbeat.mjs`, node = process.execPath, group = "bureau", args = [] } = {}) {
  return ["--quiet", "--collect", "--wait", "--pipe", `--unit=${guard}`, "--service-type=notify",
    ...[
      `Wants=${engine}`, `Before=${engine}`, "NotifyAccess=all", "WatchdogSec=12s", "WatchdogSignal=SIGKILL",
      "TimeoutStartSec=20s", "TimeoutStopSec=2s", "TimeoutAbortSec=1s", "KillMode=control-group",
      "Restart=no", "OOMPolicy=stop", `Group=${group}`, `RuntimeDirectory=${runtime.slice("/run/".length)}`,
      "RuntimeDirectoryMode=0750", "UMask=0077",
      `MemoryMax=${NATIVE_BUDGET.guardianMemoryMaxBytes}`, "MemorySwapMax=0", "CPUQuota=10%", "TasksMax=16", "LimitCORE=0",
      "NoNewPrivileges=yes", "ProtectSystem=strict", "ProtectHome=yes", "PrivateTmp=yes",
      "PrivateNetwork=yes", "RestrictAddressFamilies=AF_UNIX", "ProtectControlGroups=yes",
    ].map((value) => `--property=${value}`), "/usr/bin/env", "-i", "PATH=/usr/bin:/bin",
    "INVOCATION_ID=${INVOCATION_ID}", "NOTIFY_SOCKET=${NOTIFY_SOCKET}", node, script, ...args];
}

export async function drain(engine, owned, {
  show = serviceState, empty = emptyService, inspect = drainProcessIdentity,
  now = clock, wait = sleep, timeout = LIMITS.drain,
} = {}) {
  const deadline = now() + timeout;
  for (;;) {
    const state = await show(engine);
    requireValue(!owned || !state.InvocationID || state.InvocationID === owned.invocation,
      "engine invocation changed during drain; no name-based stop is authorized");
    requireValue(!owned || !state.ControlGroup || state.ControlGroup === owned.cgroup,
      "engine cgroup changed during drain");
    if (owned && Number(state.MainPID) > 0) {
      const observed = await inspect(state, owned);
      requireValue(observed.status === "absent"
        || (["running", "exited"].includes(observed.status) && sameIdentity(observed.identity, owned)),
      "engine process changed during drain");
    }
    if (["inactive", "failed"].includes(state.ActiveState) && state.MainPID === "0") {
      const final = await empty(engine, owned);
      requireValue(!owned || ((!final.InvocationID || final.InvocationID === owned.invocation)
        && (!final.ControlGroup || final.ControlGroup === owned.cgroup)), "engine changed during final drain observation");
      return;
    }
    requireValue(now() < deadline, "owned cgroup drain was not confirmed");
    await wait(100);
  }
}

export async function transaction({ input, output, commit, engine = ENGINE, guard = GUARD,
  arguments: args = unitArguments({ args: [commit] }), prepare = installation, launch = spawn,
  finish = drain, empty = emptyService }) {
  await prepare(commit);
  await empty(engine);
  await empty(guard);
  const child = launch("/usr/bin/systemd-run", args, { env: ENV, stdio: ["pipe", "pipe", "pipe"] });
  let problem;
  let owned;
  let buffered = "";
  let diagnostics = 0;
  let sequence = 0;
  let deadline;
  const boundClient = () => {
    deadline ??= setTimeout(() => {
      problem ??= new Error("native foreground client did not exit");
      child.kill("SIGKILL");
    }, 25_000);
  };
  const refuse = () => {
    problem ??= new Error("native foreground transaction failed");
    child.stdin.end();
    boundClient();
  };
  const stopped = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
  stopped.catch(refuse);
  child.stdin.on("error", refuse);
  child.stderr.on("error", refuse);
  output.on("error", refuse);
  child.stderr.on("data", (chunk) => {
    diagnostics += chunk.length;
    if (diagnostics > 65_536) refuse();
  });
  input.on("error", refuse);
  input.pipe(child.stdin);
  const terminate = () => { input.unpipe(child.stdin); input.pause(); child.stdin.end(); boundClient(); };
  input.on("end", terminate);
  process.on("SIGTERM", terminate).on("SIGINT", terminate);
  try {
    for await (const chunk of child.stdout) {
      requireValue(Buffer.byteLength(buffered) + chunk.length <= FRAME_BYTES, "native output exceeded its frame bound");
      buffered += chunk.toString("utf8");
      if (!buffered.endsWith("\n")) continue;
      const value = JSON.parse(buffered);
      requireValue(value.schema === SCHEMA && value.type === "challenge" && value.sequence === sequence,
        "invalid native supervision output");
      sequence += 1;
      if (value.state === "running") {
        identity(value.identity);
        requireValue(!owned || sameIdentity(owned, value.identity), "owned identity changed in native output");
        owned = value.identity;
      } else requireValue(value.state === "starting" && !owned && value.identity === null, "native identity disappeared");
      await writeFrame(output, value);
      buffered = "";
    }
  } catch {
    refuse();
  } finally {
    terminate();
    input.off("error", refuse).off("end", terminate);
    process.off("SIGTERM", terminate).off("SIGINT", terminate);
  }
  try {
    const result = await stopped;
    await finish(engine, owned);
    await empty(guard);
    await writeFrame(output, { schema: SCHEMA, type: "stopped", drained: true });
    requireValue(owned && !problem && !buffered && result.code === 0 && result.signal === null,
      "ownership ended; cgroup drained, explicit re-admission required");
  } finally {
    clearTimeout(deadline);
    output.off("error", refuse);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    await transaction({ input: process.stdin, output: process.stdout, commit: process.argv[2] });
  } catch {
    console.error("Windows supervised transaction refused; inspect native service state before re-admission");
    process.exitCode = 1;
  }
}
