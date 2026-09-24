import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { chmod, lstat, mkdir, mkdtemp, readdir, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { PRUNE_HOLD_MS } from "./maintenance-deadline.mjs";
import { BOUNDS, GiB, directoryIdentity } from "./maintenance-resources.mjs";
import { commandLock, lockedPrune, pruneTarget } from "./maintenance-target.mjs";

const linux = process.platform === "linux";
const hasFlock = linux && (() => {
  try {
    execFileSync("flock", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();

async function fixture() {
  const base = await realpath(await mkdtemp(join(tmpdir(), "bureau-target-")));
  const root = join(base, "cargo");
  await mkdir(join(root, "debug", "deps"), { recursive: true });
  await chmod(root, 0o700);
  await writeFile(join(root, "debug", "deps", "libbureau.rlib"), Buffer.alloc(4096));
  await writeFile(join(root, "CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n");
  const outside = join(base, "state");
  await mkdir(outside);
  await writeFile(join(outside, "keep"), "state");
  return { base, root, outside };
}

test("bounds leave headroom under the measurement and policy ceilings", () => {
  assert.ok(BOUNDS.cargoPruneBytes < 8 * GiB && BOUNDS.cargoPruneEntries < BOUNDS.maxEntries);
});

test("an over-bound target is emptied in place and an under-bound target is kept", { skip: !linux }, async () => {
  const cases = [
    { bounds: {}, pruned: false },
    { bounds: { bytes: 1024 }, pruned: true },
    { bounds: { entries: 3 }, pruned: true },
  ];
  const observed = [];
  for (const { bounds } of cases) {
    const { base, root } = await fixture();
    const identity = await directoryIdentity(root);
    const result = await pruneTarget(root, bounds);
    observed.push({ pruned: result.pruned, kept: (await readdir(root)).length > 0,
      same: await directoryIdentity(root) === identity });
    await rm(base, { recursive: true });
  }
  assert.deepEqual(observed, cases.map(({ pruned }) => ({ pruned, kept: !pruned, same: true })));
});

test("cleanup removes symlinks without following them", { skip: !linux }, async () => {
  const { base, root, outside } = await fixture();
  await symlink(outside, join(root, "debug", "escape"));
  await symlink(join(outside, "keep"), join(root, "link"));
  const result = await pruneTarget(root, { bytes: 1024 });
  const survived = [await readdir(root), await readdir(outside)];
  await rm(base, { recursive: true });
  assert.deepEqual([result.pruned, ...survived], [true, [], ["keep"]]);
});

test("a symlinked, foreign, shared or missing root is refused untouched", { skip: !linux }, async () => {
  const { base, root } = await fixture();
  const alias = join(base, "alias");
  await symlink(root, alias);
  const shared = join(base, "shared");
  await mkdir(shared);
  await writeFile(join(shared, "file"), "x");
  await chmod(shared, 0o777);
  const cases = [
    [alias, {}, /canonical owned cache path/u],
    [`${root}/`, {}, /canonical owned cache path/u],
    [root, { owner: process.getuid() + 1 }, /owned by the maintenance account/u],
    [shared, {}, /owned by the maintenance account/u],
    [join(base, "missing"), {}, /ENOENT/u],
  ];
  const failures = [];
  for (const [path, options, pattern] of cases) {
    await pruneTarget(path, { bytes: 0, ...options }).then(() => failures.push(path),
      (error) => pattern.test(error.message) || failures.push(`${path}: ${error.message}`));
  }
  const untouched = [(await readdir(root)).length, (await readdir(shared)).length, (await lstat(alias)).isSymbolicLink()];
  await rm(base, { recursive: true });
  assert.deepEqual([failures, untouched], [[], [2, 1, true]]);
});

test("pruning runs under the shared command lock and reports its measurement", { skip: !hasFlock }, async () => {
  const { base, root } = await fixture();
  const lock = commandLock({ cargo_target: root });
  const kept = await lockedPrune(root, lock, { waitSeconds: 5 });
  const holder = spawn("flock", [lock, "-c", "echo held; exec sleep 30"],
    { detached: true, stdio: ["ignore", "pipe", "ignore"] });
  await once(holder.stdout, "data");
  const blocked = await lockedPrune(root, lock, { waitSeconds: 0 }).then(() => "ran", (error) => error.message);
  process.kill(-holder.pid, "SIGKILL");
  holder.stdout.destroy();
  const entries = (await readdir(root)).length;
  for (let index = 0; index < BOUNDS.cargoPruneEntries; index += 1) {
    await writeFile(join(root, "debug", `stale-${index}`), "");
  }
  const pruned = await lockedPrune(root, lock, { waitSeconds: 5 });
  const left = (await readdir(root)).length;
  await rm(base, { recursive: true });
  assert.deepEqual([kept.pruned, entries, pruned.pruned, left, lock, blocked],
    [false, 2, true, 0, join(base, "command.lock"),
      "Cargo target cleanup skipped: maintenance command lock busy for 0 s; the check did not run"]);
});

test("cleanup lock waits are explicit, bounded by the prune hold and report contention", async () => {
  const observed = [];
  const conflict = Object.assign(new Error("Command failed"), { code: 75, stderr: "" });
  for (const waitSeconds of [0, 7]) {
    await lockedPrune("/cache/cargo", "/cache/command.lock", { waitSeconds,
      run: async (file, args, options) => {
        observed.push([file, args.slice(0, 5), options.timeout]);
        throw conflict;
      } }).catch((error) => observed.push(error.message));
  }
  await lockedPrune("/cache/cargo", "/cache/command.lock", { run: async () => ({ stdout: "{}" }) })
    .catch((error) => observed.push(error.message));
  assert.deepEqual(observed, [
    ["flock", ["--wait", "0", "--conflict-exit-code", "75", "/cache/command.lock"], PRUNE_HOLD_MS],
    "Cargo target cleanup skipped: maintenance command lock busy for 0 s; the check did not run",
    ["flock", ["--wait", "7", "--conflict-exit-code", "75", "/cache/command.lock"], 7000 + PRUNE_HOLD_MS],
    "Cargo target cleanup skipped: maintenance command lock busy for 7 s; the check did not run",
    "invalid Cargo target cleanup lock wait",
  ]);
});

test("cleanup failures are fatal and malformed results are rejected", async () => {
  const failed = Object.assign(new Error("Command failed"), { stderr: "owned directory identity changed\n" });
  const outcomes = [];
  for (const run of [async () => { throw failed; }, async () => ({ stdout: "{}" }),
    async () => ({ stdout: JSON.stringify({ pruned: true, bytes: -1 }) })]) {
    await lockedPrune("/cache/cargo", "/cache/command.lock", { run, waitSeconds: 1 }).then(() => outcomes.push("accepted"),
      (error) => outcomes.push(error.message));
  }
  assert.deepEqual(outcomes, ["Cargo target cleanup failed: owned directory identity changed",
    "Cargo target cleanup returned no measurement", "Cargo target cleanup returned no measurement"]);
});
