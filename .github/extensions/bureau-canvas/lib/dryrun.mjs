import { constants } from "node:fs";
import { access, cp, mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { delimiter, dirname, extname, join, parse, resolve } from "node:path";
import { homedir } from "node:os";

import { parse as parseYaml, stringify as stringifyYaml } from "./vendor/yaml.mjs";
import { wslBridged } from "./findings.mjs";

const FAKE_ROLE = "dryrun-fake";
const DEFAULT_ITEM = "dryrun-item";
const OUTCOME_TERMINALS = {
  success: "terminal:done",
  "no-work": "terminal:done",
  failure: "terminal:abort",
  blocked: "terminal:escalate",
};

export async function dryRun({ dir, pipeline, item = DEFAULT_ITEM, fixtures = {}, onEvent, ...options }) {
  const cwd = options.cwd ?? process.cwd();
  const env = options.env ?? process.env;
  const configDir = resolve(cwd, dir);
  const bureau = await locateBureau(configDir, { cwd, env, binary: options.binary });
  if (!bureau) {
    return { ok: false, state: "binary-missing", runId: null, steps: [], terminal: null };
  }

  const scratch = await prepareScratch(configDir, pipeline, fixtures, bureau, { cwd, env, ...options });
  const validation = await runValidate(bureau, scratch.configDir, { cwd, env });
  if (validation.code !== 0) {
    return invalidResult(scratch, validation);
  }

  const run = await runPipeline(bureau, pipeline, item, scratch, { cwd, env, onEvent, pollMs: options.pollMs });
  return { ok: run.code === 0, state: "finished", ...run, scratch, validation };
}

async function prepareScratch(configDir, pipeline, fixtures, bureau, options) {
  const root = await newScratchRoot(options);
  const scratch = scratchPaths(root);
  await mkdir(scratch.root, { recursive: true });
  await cp(configDir, scratch.configDir, { recursive: true, force: true });
  await mkdir(scratch.fixturesDir, { recursive: true });
  await mkdir(scratch.runsDir, { recursive: true });
  await mkdir(dirname(scratch.statePath), { recursive: true });
  await mkdir(scratch.configCacheDir, { recursive: true });
  await mkdir(scratch.checkoutCacheDir, { recursive: true });

  await rewriteConfig(scratch.configDir, scratch.fixturesDir, fixtures, bureau);
  await writeSettings(scratch.settingsPath, configPathForBureau(bureau, scratch.configDir));
  return scratch;
}

async function newScratchRoot(options) {
  const parent = options.artifactsRoot ?? defaultArtifactsRoot(options.env);
  const suffix = `${Date.now()}-${process.pid}-${Math.random().toString(16).slice(2)}`;
  return join(parent, `dryrun-${suffix}`);
}

function defaultArtifactsRoot(env) {
  const home = env.COPILOT_HOME ?? join(homedir(), ".copilot");
  return join(home, "extensions", "bureau-canvas", "artifacts");
}

function scratchPaths(root) {
  return {
    root,
    configDir: join(root, "config"),
    fixturesDir: join(root, "fixtures"),
    runsDir: join(root, "runs"),
    statePath: join(root, "state", "state.sqlite"),
    configCacheDir: join(root, "config-cache"),
    checkoutCacheDir: join(root, "checkout-cache"),
    settingsPath: join(root, "settings.yaml"),
  };
}

async function rewriteConfig(configDir, fixturesDir, fixtures, bureau) {
  const roles = await readNamedYaml(join(configDir, "roles"));
  await writeFakeRole(configDir);
  for (const file of await yamlFiles(join(configDir, "pipelines"))) {
    const data = parseYaml(await readFile(file, "utf8"));
    const pipelineName = data.name ?? parse(file).name;
    for (const step of data.steps ?? []) {
      if (step.type === "agent") {
        rewriteAgentStep(step, roles, await writeFixture(fixturesDir, pipelineName, step, fixtures, bureau));
      }
    }
    await writeFile(file, stringifyYaml(data, { lineWidth: 0 }));
  }
}

function rewriteAgentStep(step, roles, fixture) {
  const role = roles.get(step.role);
  if (step.trust == null && role?.min_trust != null) {
    step.trust = role.min_trust;
  }
  step.role = FAKE_ROLE;
  step.fixture = fixture;
}

async function writeFakeRole(configDir) {
  const role = {
    name: FAKE_ROLE,
    agent: "dry-run",
    adapter: "fake",
    permissions: [],
    min_trust: "untrusted",
  };
  await writeFile(join(configDir, "roles", `${FAKE_ROLE}.yaml`), stringifyYaml(role, { lineWidth: 0 }));
}

async function readNamedYaml(dir) {
  const values = new Map();
  for (const file of await yamlFiles(dir)) {
    const value = parseYaml(await readFile(file, "utf8"));
    values.set(value.name ?? parse(file).name, value);
  }
  return values;
}

async function yamlFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && [".yaml", ".yml"].includes(extname(entry.name)))
    .map((entry) => join(dir, entry.name))
    .sort();
}

async function writeFixture(fixturesDir, pipeline, step, fixtures, bureau) {
  const transcript = transcriptFor(step, fixtures);
  const file = join(fixturesDir, `dryrun-${pipeline}-${step.name}.json`);
  await writeFile(file, `${JSON.stringify(transcript, null, 2)}\n`);
  return configPathForBureau(bureau, file);
}

function transcriptFor(step, fixtures) {
  const chosen = fixtures[step.name] ?? fixtures.default ?? resultFor(step.name, "success");
  if (chosen?.chunks && Number.isInteger(chosen.exit_code)) {
    return chosen;
  }
  return transcriptFromResult({ ...resultFor(step.name, "success"), ...chosen });
}

function resultFor(step, outcome) {
  return {
    schema: "v2",
    outcome,
    outputs: {},
    artifacts: [],
    trust: "derived",
    message: `dry run ${step}`,
  };
}

function transcriptFromResult(result) {
  return {
    schema: "v2",
    chunks: [{ delay_ms: 0, stream: "stdout", data: JSON.stringify(result) }],
    exit_code: 0,
    usage: { source: "dry-run", input_tokens: null, output_tokens: null, cost_usd: null },
  };
}

async function writeSettings(path, configDir) {
  const settings = {
    config: { kind: "separate_repository", remote: configDir, reference: "HEAD" },
    credentials: {},
    plugin: { install_user_global: false },
    migration: { source: null },
  };
  await writeFile(path, stringifyYaml(settings, { lineWidth: 0 }));
}

function configPathForBureau(bureau, path) {
  return bureau.translate ? bureau.translate(path) : path;
}

async function runValidate(bureau, dir, options) {
  const args = [...bureau.args, "validate", configPathForBureau(bureau, dir), "--json"];
  return collectRun(spawn(bureau.command, args, { cwd: options.cwd, env: options.env, windowsHide: true }));
}

async function runPipeline(bureau, pipeline, item, scratch, options) {
  const args = [
    ...bureau.args,
    "run",
    pipeline,
    "--item",
    item,
    "--settings",
    configPathForBureau(bureau, scratch.settingsPath),
    "--config-cache",
    configPathForBureau(bureau, scratch.configCacheDir),
    "--runs",
    configPathForBureau(bureau, scratch.runsDir),
    "--state",
    configPathForBureau(bureau, scratch.statePath),
    "--cache",
    configPathForBureau(bureau, scratch.checkoutCacheDir),
  ];
  const child = spawn(bureau.command, args, { cwd: options.cwd, env: options.env, windowsHide: true });
  const collected = collectRun(child);
  let done = false;
  const finished = collected.then((run) => {
    done = true;
    return run;
  });
  const progress = await tailRunLog(scratch.runsDir, options.onEvent, () => done, options.pollMs);
  const run = await finished;
  return { ...run, runId: progress.runId, steps: progress.steps, terminal: progress.terminal };
}

async function tailRunLog(runsDir, onEvent, done, pollMs = 20) {
  const state = { seen: new Map(), runId: null, steps: [], terminal: null };
  while (!done()) {
    await scanRunLogs(runsDir, state, onEvent);
    await sleep(pollMs);
  }
  await scanRunLogs(runsDir, state, onEvent);
  return state;
}

async function scanRunLogs(runsDir, state, onEvent) {
  for (const dir of await runDirs(runsDir)) {
    const events = await readEvents(join(dir, "events.jsonl"));
    const last = state.seen.get(dir) ?? -1;
    for (const event of events.filter((candidate) => candidate.seq > last)) {
      state.seen.set(dir, event.seq);
      await acceptEvent(event, state, onEvent);
    }
  }
}

async function runDirs(runsDir) {
  const entries = await readdir(runsDir, { withFileTypes: true }).catch(() => []);
  return entries.filter((entry) => entry.isDirectory()).map((entry) => join(runsDir, entry.name));
}

async function readEvents(path) {
  const text = await readFile(path, "utf8").catch(() => "");
  const lines = text.endsWith("\n") || text.endsWith("\r") ? text.split(/\r?\n/u) : text.split(/\r?\n/u).slice(0, -1);
  return lines.filter(Boolean).flatMap(parseEvent);
}

function parseEvent(line) {
  try {
    return [JSON.parse(line)];
  } catch {
    return [];
  }
}

async function acceptEvent(event, state, onEvent) {
  if (event.kind === "run_started") {
    state.runId = event.data?.run_id ?? state.runId;
    await publish(onEvent, { type: "run_started", runId: state.runId });
  }
  if (event.kind === "step_started") {
    await stepStarted(event.data?.step, state, onEvent);
  }
  if (event.kind === "group_started") {
    await stepStarted(event.data?.group, state, onEvent);
  }
  if (event.kind === "group_member_started") {
    await stepStarted(event.data?.member, state, onEvent);
  }
  if (event.kind === "step_finished") {
    await publish(onEvent, { type: "step_finished", id: event.data?.step, step: event.data?.step, outcome: event.data?.outcome });
  }
  if (event.kind === "run_finished") {
    await terminalReached(event.data, state, onEvent);
  }
}

async function stepStarted(step, state, onEvent) {
  if (!step) {
    return;
  }
  state.steps.push(step);
  await publish(onEvent, { type: "step_started", id: step, step });
}

async function terminalReached(data, state, onEvent) {
  const id = OUTCOME_TERMINALS[data?.outcome] ?? null;
  state.terminal = id;
  await publish(onEvent, { type: "terminal", id, terminal: id?.slice("terminal:".length) ?? null, outcome: data?.outcome });
}

async function publish(onEvent, event) {
  if (onEvent) {
    await onEvent(event);
  }
}

function invalidResult(scratch, validation) {
  return { ok: false, state: "invalid", runId: null, steps: [], terminal: null, scratch, validation };
}

function collectRun(child) {
  return new Promise((resolveRun) => {
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8").on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => resolveRun({ code: null, stdout, stderr, error }));
    child.on("close", (code) => resolveRun({ code, stdout, stderr }));
  });
}

async function locateBureau(dir, options) {
  const explicit = options.binary ?? options.env.BUREAU_CANVAS_BUREAU;
  if (explicit) {
    return (await commandFor(explicit)) ?? null;
  }
  const found = (await findOnPath("bureau", options.env)) ?? (await findInWorkspace(dir, options.cwd));
  return found ? wslBridged(found) : null;
}

async function findOnPath(name, env) {
  for (const directory of pathDirectories(env)) {
    for (const candidate of commandNames(name, env)) {
      const command = await commandFor(join(directory, candidate));
      if (command) {
        return command;
      }
    }
  }
  return null;
}

async function findInWorkspace(dir, cwd) {
  for (const root of uniqueAncestors([dir, cwd])) {
    for (const name of commandNames("bureau", process.env)) {
      const command = await commandFor(join(root, "target", "debug", name));
      if (command) {
        return command;
      }
    }
  }
  return null;
}

function pathDirectories(env) {
  return String(env.PATH ?? env.Path ?? "").split(delimiter).filter(Boolean);
}

function commandNames(name, env) {
  if (process.platform !== "win32") {
    return [name, `${name}.mjs`];
  }
  const extensions = String(env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD")
    .split(";")
    .filter(Boolean)
    .map((value) => value.toLowerCase());
  return [name, `${name}.mjs`, ...extensions.map((extension) => `${name}${extension}`)];
}

function uniqueAncestors(paths) {
  const seen = new Set();
  return paths.flatMap((path) => {
    const roots = ancestors(path);
    return roots.filter((root) => {
      const known = seen.has(root);
      seen.add(root);
      return !known;
    });
  });
}

function ancestors(path) {
  const roots = [];
  for (let current = resolve(path); current !== dirname(current); current = dirname(current)) {
    roots.push(current);
  }
  roots.push(dirname(roots.at(-1) ?? path));
  return roots;
}

async function commandFor(path) {
  if (!(await canRun(path, extname(path) === ".mjs"))) {
    return null;
  }
  return extname(path) === ".mjs" ? { command: process.execPath, args: [path] } : { command: path, args: [] };
}

async function canRun(path, isModule) {
  try {
    const mode = process.platform === "win32" || isModule ? constants.F_OK : constants.F_OK | constants.X_OK;
    await access(path, mode);
    return true;
  } catch {
    return false;
  }
}

function sleep(ms) {
  return new Promise((resolveSleep) => {
    setTimeout(resolveSleep, ms);
  });
}
