import { lstat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { readBoundedJson } from "../../scripts/maintenance-files.mjs";
import { clock, publish } from "./lease.mjs";
import { admissionServer } from "./admission.mjs";
import { SCHEMA, LIMITS, frames, heartbeat, identity, nonce, refusalReason, sameIdentity, writeFrame } from "./protocol.mjs";
import { ENGINE, ENV, GUARD, RUNTIME, command, installation, processIdentity, properties, serviceState, systemRoot }
  from "./system.mjs";

export async function supervise({ input, output, commit, effects, limits = LIMITS }) {
  const reader = frames(input);
  const lostOutput = () => reader.close();
  output.on("error", lostOutput);
  const startedAt = effects.now();
  let owner;
  let known;
  let root;
  let started = false;
  const observe = async () => {
    const current = await effects.observe();
    requireValue(["starting", "running"].includes(current.state), "engine left its owned lifetime");
    if (current.state === "running") {
      identity(current.identity, root);
      requireValue(!known || sameIdentity(known, current.identity), "engine identity changed");
      known = current.identity;
    } else {
      requireValue(!known && effects.now() - startedAt < limits.startup, "engine startup expired or identity disappeared");
    }
    return current;
  };
  try {
    const prepared = await effects.prepare();
    root = await effects.root();
    const guard = identity(prepared, root);
    for (let sequence = 0; ; sequence += 1) {
      reader.check();
      const state = started ? await observe() : { state: "starting", identity: null };
      const challenge = { schema: SCHEMA, type: "challenge", nonce: nonce(), sequence,
        guard: guard.invocation, ...state };
      const response = reader.read(limits.response);
      const [value] = await Promise.all([response, writeFrame(output, challenge)]);
      owner = heartbeat(value, challenge, owner, commit);
      reader.check();
      if (started) await observe();
      await effects.fresh({ guard, owner, commit, sequence, at: effects.now() });
      await effects.notify(started ? "WATCHDOG=1" : "READY=1");
      started = true;
      await effects.wait(limits.interval);
    }
  } finally {
    reader.close();
    output.off("error", lostOutput);
    await effects.close?.();
  }
}

export function nativeEffects({ engine = ENGINE, guard = GUARD, runtime = RUNTIME,
  commit, executable = "/opt/bureau/bin/bureau", prepare = () => installation(commit),
  notifySocket = process.env.NOTIFY_SOCKET, invocation = process.env.INVOCATION_ID } = {}) {
  let guardian;
  let main;
  let group;
  let admissions;
  const ownGuard = async () => {
    const state = await serviceState(guard);
    const observed = (await processIdentity(state)).identity;
    requireValue(observed.invocation === invocation && observed.pid === process.pid
      && (!guardian || sameIdentity(guardian, observed)), "heartbeat invocation changed");
    return observed;
  };
  return {
    now: clock, wait: sleep, root: systemRoot,
    async prepare() {
      await prepare();
      guardian = await ownGuard();
      const { User: user } = await properties(engine, ["User"]);
      requireValue(typeof user === "string" && /^[a-z_][a-z0-9_-]*$/u.test(user), "service user is not explicit");
      const uid = Number(await command("/usr/bin/id", ["-u", "--", user]));
      requireValue(Number.isSafeInteger(uid) && uid > 0, "pre-start must be unprivileged");
      admissions = await admissionServer({ runtime, engine, guard, commit, uid, executable: process.execPath });
      return guardian;
    },
    close: () => admissions?.close(),
    fresh: (lease) => { admissions.check(); return publish(runtime, "lease.json", lease); },
    async notify(message) {
      requireValue(typeof notifySocket === "string" && notifySocket.startsWith("/"), "systemd notification socket missing");
      await command("/usr/bin/systemd-notify", [`--pid=${process.pid}`, message],
        { ...ENV, NOTIFY_SOCKET: notifySocket });
    },
    async observe() {
      admissions.check();
      await ownGuard();
      const profile = await properties(engine, ["RefuseManualStart", "Restart", "NeedDaemonReload"]);
      requireValue(profile.RefuseManualStart === "yes" && profile.Restart === "no"
        && profile.NeedDaemonReload === "no", "supervision profile changed");
      const state = await serviceState(engine);
      requireValue(["activating", "active"].includes(state.ActiveState), "engine stopped or startup failed");
      if (state.ActiveState === "activating") {
        requireValue(!main, "engine began a replacement invocation");
        return { state: "starting", identity: null };
      }
      requireValue(state.SubState === "running", "engine is not running");
      const claimed = await readBoundedJson(join(runtime, "claim.json"));
      requireValue(claimed.invocation === state.InvocationID && claimed.guard === guardian.invocation,
        "engine has no matching one-use startup grant");
      const observed = await processIdentity(state);
      const metadata = await lstat(`/sys/fs/cgroup${observed.identity.cgroup}`, { bigint: true });
      const key = `${metadata.dev}:${metadata.ino}`;
      requireValue(metadata.isDirectory() && !metadata.isSymbolicLink() && (!group || group === key)
        && (!main || sameIdentity(main, observed.identity)), "engine process or cgroup identity changed");
      main = observed.identity;
      group = key;
      const repeated = await serviceState(engine);
      requireValue(repeated.ActiveState === "active" && repeated.SubState === "running"
        && repeated.InvocationID === main.invocation && Number(repeated.MainPID) === main.pid
        && repeated.ControlGroup === main.cgroup, "engine changed during identity observation");
      if (observed.executable !== executable) {
        requireValue(observed.executable === "/usr/bin/bash" || observed.executable === "/usr/bin/flock",
          "unexpected startup executable");
        return { state: "starting", identity: null };
      }
      return { state: "running", identity: main };
    },
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const commit = process.argv[2];
    await supervise({ input: process.stdin, output: process.stdout, commit, effects: nativeEffects({ commit }) });
  } catch (error) {
    console.error(`Windows ownership supervision refused: ${refusalReason(error?.message)}`);
    process.exitCode = 1;
  }
}
