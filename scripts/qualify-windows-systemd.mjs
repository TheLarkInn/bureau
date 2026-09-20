import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { copyFile, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { PassThrough } from "node:stream";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import { requireValue } from "./maintenance-contract.mjs";
import { readBoundedJson } from "./maintenance-files.mjs";
import { transaction, unitArguments } from "../deployment/supervision/transaction.mjs";
import { ENV, emptyService, processIdentity, properties, serviceState } from "../deployment/supervision/system.mjs";
import { SCHEMA } from "../deployment/supervision/protocol.mjs";
import { interpreterApproval, binaryDigest } from "../deployment/supervision/provenance.mjs";

const execute = promisify(execFile);
const source = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const commit = "1".repeat(40);
const owner = "2".repeat(32);
const root = process.argv.at(-1);
requireValue(process.platform === "linux" && process.getuid() === 0
  && process.env.GITHUB_ACTIONS === "true" && process.env.RUNNER_ENVIRONMENT === "github-hosted"
  && /^\/run\/bureau-supervision-fixture-\d+-\d+$/u.test(root ?? ""),
"live qualification is restricted to a disposable GitHub-hosted runner");

async function run(command, args) {
  return execute(command, args, { env: ENV, timeout: 25_000, killSignal: "SIGKILL", maxBuffer: 65_536, encoding: "utf8" });
}

async function cleanup() {
  let record;
  try { record = await readBoundedJson(join(root, "owned.json")); }
  catch (error) { if (error.code === "ENOENT") return; throw error; }
  requireValue(Array.isArray(record) && record.length <= 16, "invalid fixture ownership list");
  for (const { engine, guard } of record) {
    requireValue(/^bureau-supervision-test-[a-f0-9-]+-engine\.service$/u.test(engine)
      && guard === engine.replace("-engine.", "-guard."), "refusing unrelated fixture cleanup");
    for (const unit of [engine, guard]) {
      const state = await serviceState(unit);
      if (["active", "activating", "deactivating"].includes(state.ActiveState)) {
        await run("/usr/bin/systemctl", ["stop", unit]);
      }
      await emptyService(unit);
    }
    await rm(`/run/systemd/system/${engine}`, { force: true });
    await rm(`/run/systemd/system/${engine}.d/20-reporter-credential.conf`, { force: true });
    await rm(`/run/systemd/system/${engine}.d/windows-supervision.conf`, { force: true });
    await rm(`/run/systemd/system/${engine}.d`, { recursive: true, force: true });
  }
  await run("/usr/bin/systemctl", ["daemon-reload"]);
  await rm(root, { recursive: true, force: true });
}

async function copySources() {
  await mkdir(join(root, "source", "deployment", "supervision"), { recursive: true, mode: 0o755 });
  await mkdir(join(root, "source", "scripts"), { mode: 0o755 });
  await mkdir(join(root, "source", "deployment", "windows"), { mode: 0o755 });
  await copyFile(join(source, "deployment/windows/native-budget.json"), join(root, "source/deployment/windows/native-budget.json"));
  for (const name of ["protocol", "system", "provenance", "profile", "budget", "admission", "lease", "claim", "heartbeat", "transaction"]) {
    const path = `deployment/supervision/${name}.mjs`;
    await copyFile(join(source, path), join(root, "source", path));
  }
  await copyFile(join(source, "deployment/supervision/peer.py"), join(root, "source/deployment/supervision/peer.py"));
  for (const name of ["maintenance-contract", "maintenance-files", "maintenance-mount", "supervision-systemd-child"]) {
    const path = `scripts/${name}.mjs`;
    await copyFile(join(source, path), join(root, "source", path));
  }
}

async function entryLock() {
  const lock = join(root, "owner.lock");
  const childScript = join(root, "lock-test.mjs");
  const entry = join(root, "entry-test.sh");
  await writeFile(childScript, "process.stdin.resume(); process.stdout.write('locked\\n');\n"
    + "process.stdin.on('end', () => setTimeout(() => {}, 250));\n");
  const original = await readFile(join(source, "deployment/windows-entry.sh"), "utf8");
  await writeFile(entry, original.replace("/run/bureau-windows-owner.lock", lock)
    .replace("node /opt/bureau/source/deployment/supervision/transaction.mjs", `${process.execPath} ${childScript}`));
  const child = spawn("/bin/bash", [entry, commit], { env: ENV, stdio: ["pipe", "pipe", "pipe"] });
  const finished = new Promise((resolve, reject) => {
    child.once("error", reject).once("exit", (code) => code === 0 ? resolve() : reject(new Error("owned entry failed")));
  });
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("entry did not acquire ownership")), 4000);
      child.stdout.once("data", (bytes) => { clearTimeout(timer); assert.equal(bytes.toString(), "locked\n"); resolve(); });
    });
    const overlap = () => assert.rejects(run("/bin/bash", [entry, commit]),
      (error) => error.code === 1 && error.stdout === "");
    await overlap();
    child.stdin.end();
    await overlap();
    await finished;
    await run("/usr/bin/flock", ["--nonblock", lock, "/bin/true"]);
    console.log("ok - actual native entry refuses overlap through the owned drain interval");
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
}

async function provision(mode, owned) {
  const name = `bureau-supervision-test-${randomBytes(12).toString("hex")}`;
  const engine = `${name}-engine.service`;
  const guard = `${name}-guard.service`;
  const runtime = `/run/${name}`;
  const script = join(root, "source", "scripts", "supervision-systemd-child.mjs");
  const reporter = join(root, `${name}.env`);
  owned.push({ engine, guard });
  await writeFile(join(root, "owned.json"), JSON.stringify(owned), { mode: 0o600 });
  const childArgs = [engine, guard, runtime, mode, commit, await realpath("/bin/sh")];
  const claimCommand = `${process.execPath} ${script} claim ${childArgs.join(" ")}`;
  const base = `[Unit]\nDescription=Isolated Bureau lifetime fixture\n[Service]\nType=exec\nUser=nobody\n`
    + `ExecStart=${mode === "start-failure" ? "/bin/false" : "/bin/sh -c 'test \"$BUREAU_SYNTHETIC_REPORTER\" = fixture-only || exit 99; trap \"\" TERM; /bin/sleep 300 & wait'"}\n`
    + "Restart=on-failure\nTimeoutStopSec=70min\nMemoryMax=128M\nMemorySwapMax=0\nCPUQuota=25%\nTasksMax=32\n"
    + "RuntimeMaxSec=60s\nPrivateNetwork=yes\nNoNewPrivileges=yes\nProtectSystem=strict\n"
    + "ProtectHome=yes\nProtectControlGroups=yes\nStandardOutput=null\nStandardError=null\n";
  await writeFile(`/run/systemd/system/${engine}`, base);
  await mkdir(`/run/systemd/system/${engine}.d`);
  await writeFile(reporter, "BUREAU_SYNTHETIC_REPORTER=fixture-only\n", { mode: 0o600 });
  const reporterDrop = (await readFile(join(source, "deployment/20-reporter-credential.conf"), "utf8"))
    .replace("/var/lib/bureau-maintenance/credentials/github-reporter.env", reporter);
  await writeFile(`/run/systemd/system/${engine}.d/20-reporter-credential.conf`, reporterDrop);
  const profile = (await readFile(join(source, "deployment/windows-supervision.conf"), "utf8"))
    .replaceAll("bureau-windows-heartbeat.service", guard)
    .replace("node /opt/bureau/source/deployment/supervision/claim.mjs", claimCommand);
  await writeFile(`/run/systemd/system/${engine}.d/windows-supervision.conf`, profile);
  await run("/usr/bin/systemctl", ["daemon-reload"]);
  const effective = await properties(engine, ["EnvironmentFiles", "DropInPaths"]);
  assert.equal(effective.EnvironmentFiles, `${reporter} (ignore_errors=no)`);
  assert.equal(effective.DropInPaths,
    `/run/systemd/system/${engine}.d/20-reporter-credential.conf /run/systemd/system/${engine}.d/windows-supervision.conf`);
  return { engine, guard, runtime, script, args: ["heartbeat", ...childArgs], group: "nogroup" };
}

async function guardSignal(context, challenge, signal) {
  const observed = (await processIdentity(await serviceState(context.guard))).identity;
  requireValue(observed.invocation === challenge.guard, "test guardian identity changed");
  process.kill(observed.pid, signal);
}

async function exercise(mode, owned) {
  const context = await provision(mode, owned);
  await assert.rejects(run("/usr/bin/systemctl", ["start", context.engine]));
  const input = new PassThrough();
  const output = new PassThrough();
  let running = 0;
  let drained = false;
  let observed;
  let actionFailure;
  let action = Promise.resolve();
  output.on("data", (chunk) => {
    const value = JSON.parse(chunk);
    if (value.type === "stopped") { drained = value.drained === true; return; }
    action = action.then(async () => {
      if (value.state === "running") {
        observed ??= value.identity;
        running += 1;
      }
      if (mode === "no-owner") { input.end(); return; }
      if (running > 0 && ["eof", "healthy", "forged-peer"].includes(mode)) {
        if (mode === "healthy") {
          await assert.rejects(run("/usr/bin/systemctl", ["restart", context.engine]));
          assert.deepEqual((await processIdentity(await serviceState(context.engine))).identity, observed);
        }
        input.end();
        return;
      }
      if (running > 0 && mode === "frozen-writer") return;
      if (running > 0 && mode === "oversized") { input.write(`${"x".repeat(4096)}\n`); return; }
      input.write(`${JSON.stringify({ schema: SCHEMA, type: "heartbeat", nonce: value.nonce,
        sequence: value.sequence, guard: value.guard, owner, commit, ageMs: 0 })}\n`);
      if (running > 0 && mode === "native-loss") await guardSignal(context, value, "SIGKILL");
      if (running > 0 && mode === "native-freeze") await guardSignal(context, value, "SIGSTOP");
    }).catch((error) => { actionFailure = error; input.end(); });
  });
  const start = performance.now();
  const result = transaction({ input, output, commit, engine: context.engine, guard: context.guard,
    arguments: unitArguments(context), prepare: async () => {} });
  await assert.rejects(result, /ownership ended/u);
  await action;
  if (actionFailure) throw actionFailure;
  assert.equal(drained, true);
  assert.equal(running > 0, !["partial", "no-owner", "start-failure"].includes(mode));
  await emptyService(context.engine);
  await emptyService(context.guard);
  assert.ok(performance.now() - start < 30_000, "emergency shutdown exceeded the fixture deadline");
  await sleep(200);
  assert.equal((await serviceState(context.engine)).MainPID, "0", "refusal restarted the engine");
  console.log(`ok - real systemd ${mode}: dependency, one owner and cgroup drain`);
}

if (process.argv.includes("--cleanup")) {
  await cleanup();
} else {
  const owned = [];
  await mkdir(root, { mode: 0o755 });
  try {
    await copySources();
    const interpreter = { node_path: process.execPath, node_sha256: "a".repeat(64) };
    interpreterApproval(interpreter);
    assert.throws(() => interpreterApproval({ ...interpreter, node_path: "/usr/bin/unqualified-node" }));
    await assert.rejects(binaryDigest(join(root, "missing-node")), { code: "ENOENT" });
    await entryLock();
    for (const mode of ["no-owner", "partial", "start-failure", "healthy", "forged-peer", "eof", "oversized",
      "frozen-writer", "native-loss", "native-freeze"]) await exercise(mode, owned);
    console.log("10 real systemd lifetime cases passed; no Bureau engine, model or forge was invoked");
  } finally {
    await cleanup();
  }
}
