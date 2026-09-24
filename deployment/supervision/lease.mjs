import { link, rename, unlink, writeFile } from "node:fs/promises";
import { uptime } from "node:os";
import { join } from "node:path";

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { readBoundedJson } from "../../scripts/maintenance-files.mjs";
import { COMMIT, ID, LIMITS, exactKeys, identity, nonce, sameIdentity } from "./protocol.mjs";
import { processIdentity, serviceState, systemRoot } from "./system.mjs";

export const clock = () => Math.floor(uptime() * 1000);

export function checkedLease(value, now, commit, root) {
  exactKeys(value, ["guard", "owner", "commit", "sequence", "at"]);
  identity(value.guard, root);
  requireValue(typeof value.owner === "string" && ID.test(value.owner)
    && typeof value.commit === "string" && COMMIT.test(value.commit) && value.commit === commit
    && Number.isSafeInteger(value.sequence) && value.sequence >= 0
    && Number.isSafeInteger(now) && now >= 0 && Number.isSafeInteger(value.at) && value.at >= 0
    && value.at <= now && now - value.at < LIMITS.response,
  "startup lease is missing, mismatched or stale");
  return value;
}

export async function publish(runtime, name, value, once = false) {
  const temporary = join(runtime, `${name}.${nonce()}.tmp`);
  await writeFile(temporary, `${JSON.stringify(value)}\n`, { flag: "wx", mode: 0o600 });
  try {
    if (once) await link(temporary, join(runtime, name));
    else await rename(temporary, join(runtime, name));
  } finally {
    if (once) await unlink(temporary);
  }
}

export async function claim({ runtime, engine, guard, commit, invocation, pid, executable = process.execPath }, {
  now = clock, show = serviceState, inspect = processIdentity, read = readBoundedJson, save = publish,
  live = () => true, root = systemRoot,
} = {}) {
  requireValue(live() && typeof invocation === "string" && ID.test(invocation)
    && Number.isSafeInteger(pid) && pid > 1, "startup has no live systemd identity");
  const manager = await root();
  const lease = checkedLease(await read(join(runtime, "lease.json")), now(), commit, manager);
  const guardian = await show(guard);
  requireValue(guardian.ActiveState === "active" && guardian.SubState === "running"
    && sameIdentity((await inspect(guardian)).identity, lease.guard), "startup guardian identity changed");
  const service = await show(engine);
  requireValue(service.ActiveState === "activating" && service.SubState === "start-pre"
    && service.InvocationID === invocation && service.ControlPID === String(pid),
  "startup is not the owned service transaction");
  const control = await inspect({ ...service, MainPID: service.ControlPID });
  requireValue(control.identity.invocation === invocation && control.identity.pid === pid
    && control.executable === executable, "pre-start process is not the qualified interpreter");
  checkedLease(lease, now(), commit, manager);
  requireValue(live(), "startup ownership ended during claim");
  // A successful link consumes the only admission for this heartbeat lifetime.
  // Failure is not rolled back, so even a failed ExecStart cannot be replaced.
  await save(runtime, "claim.json", { invocation, guard: lease.guard.invocation, owner: lease.owner }, true);
}
