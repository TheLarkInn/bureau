import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { chmod, mkdir, readFile, readdir, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import { readFactoryControls } from "../../lib/factory-controls.mjs";
import { parseFactoryControls } from "../../web/live/factory-controls.mjs";

const ROOT = fileURLToPath(new URL("../../../../../", import.meta.url));
const execute = promisify(execFile);

async function command(executable, args, env, timeout = 30_000) {
  const { stdout, stderr } = await execute(executable, args, {
    cwd: ROOT, env, timeout, maxBuffer: 16 * 1024 * 1024,
  });
  return { code: 0, stdout, stderr };
}

async function writable(directory) {
  await chmod(directory, 0o700);
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) await writable(join(directory, entry.name));
  }
}

async function remove(directory) {
  await writable(directory);
  await rm(directory, { recursive: true, force: true });
}

async function generated() {
  const directory = join(ROOT, "target", `engine-pause-${randomUUID()}`);
  await mkdir(join(ROOT, "target"), { recursive: true });
  await mkdir(directory);
  try {
    await command("cargo", [
      "test", "--offline", "-p", "bureau", "--test", "copilot_factory_engine",
      "corrections::pause_projection::", "--", "--nocapture",
    ], { ...process.env, BUREAU_ENGINE_PAUSE_EVIDENCE_DIR: directory }, 600_000);
    return directory;
  } catch (error) {
    await remove(directory);
    throw error;
  }
}

async function pauseMarker(directory) {
  try {
    return await readFile(join(directory, "PAUSE"), "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

async function projection(directory, scenario) {
  const receipt = JSON.parse(await readFile(join(directory, scenario, "evidence.json"), "utf8"));
  assert.equal(receipt.schema, "bureau-engine-pause-evidence-v1");
  assert.equal(receipt.scenario, scenario);
  const exec = (args) => command(receipt.bureau, args, { BUREAU_HOME: receipt.root });
  const [raw, controls, recorded, marker] = await Promise.all([
    exec(["show", receipt.run_id, "--events", "--json", "--runs", receipt.runs_dir]),
    readFactoryControls(receipt.run_id, receipt.runs_dir, { exec }),
    readFile(receipt.events, "utf8"),
    pauseMarker(join(receipt.runs_dir, receipt.run_id)),
  ]);
  const events = JSON.parse(raw.stdout);
  assert.deepEqual(events, recorded.trim().split("\n").map((line) => JSON.parse(line)));
  return { events, marker, control: parseFactoryControls(controls, receipt.run_id) };
}

export async function enginePauseEvidence() {
  const supplied = process.env.BUREAU_ENGINE_PAUSE_EVIDENCE_DIR;
  const directory = supplied ? resolve(supplied) : await generated();
  return {
    read: (scenario) => projection(directory, scenario),
    cleanup: () => supplied ? Promise.resolve() : remove(directory),
  };
}
