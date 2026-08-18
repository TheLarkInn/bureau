import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, extname, isAbsolute, join, parse } from "node:path";

import { parse as parseYaml } from "../../lib/vendor/yaml.mjs";

const [, , command, ...args] = process.argv;

if (command === "validate") {
  await validate(args);
} else if (command === "run") {
  await run(args);
} else {
  process.stderr.write(`unexpected command: ${command}\n`);
  process.exit(64);
}

async function validate(args) {
  const [dir, jsonFlag] = args;
  if (jsonFlag !== "--json") {
    process.stderr.write("expected: validate <dir> --json\n");
    process.exit(64);
  }
  const config = await loadConfig(dir);
  const errors = fixtureErrors(dir, config);
  process.stdout.write(`${JSON.stringify({ ok: errors.length === 0, dir, errors, config })}\n`);
  process.exit(errors.length === 0 ? 0 : 1);
}

async function run(args) {
  const pipeline = args[0];
  const options = optionsOf(args.slice(1));
  const configDir = await settingsRemote(options.settings);
  const config = await loadConfig(configDir);
  const target = config.pipelines[pipeline];
  const runId = `dryrun-${Date.now()}`;
  const dir = join(options.runs, runId);
  await mkdir(dir, { recursive: true });
  const append = appender(join(dir, "events.jsonl"));
  await append("run_started", { run_id: runId, assignment: "dryrun", item: options.item });
  await emitSteps(append, target.steps ?? [], options.item);
  process.stdout.write(`${runId} success cost=$0.00 dry run\n`);
}

async function emitSteps(append, steps, item) {
  if (item === "dryrun-log-over-config") {
    await mismatchedLog(append, steps);
    return;
  }
  const first = steps[0];
  const outcome = first ? await outcomeOf(first) : "success";
  if (outcome !== "success") {
    await append("step_started", { step: first.name });
    await append("step_finished", { step: first.name, outcome });
    await append("run_finished", { outcome: "blocked", message: "step failed and escalated", cost_usd: 0 });
    process.exit(1);
  }
  for (const step of steps) {
    await append("step_started", { step: step.name });
    await append("step_finished", { step: step.name, outcome: await outcomeOf(step) });
  }
  await append("run_finished", { outcome: "success", message: "dry run", cost_usd: 0 });
}

async function mismatchedLog(append, steps) {
  await append("step_started", { step: steps[0].name });
  await append("step_finished", { step: steps[0].name, outcome: "failure" });
  await append("step_started", { step: steps[1].name });
  await append("step_finished", { step: steps[1].name, outcome: "success" });
  await append("run_finished", { outcome: "success", message: "log wins", cost_usd: 0 });
}

function appender(path) {
  let seq = 0;
  return async (kind, data) => {
    const event = { seq, at_ms: Date.now(), kind, data };
    seq += 1;
    await writeFile(path, `${JSON.stringify(event)}\n`, { flag: "a" });
  };
}

async function outcomeOf(step) {
  if (step.type !== "agent") {
    return "success";
  }
  const transcript = JSON.parse(await readFile(step.fixture, "utf8"));
  const stdout = transcript.chunks.find((chunk) => chunk.stream === "stdout")?.data ?? "{}";
  return JSON.parse(stdout).outcome ?? "success";
}

function optionsOf(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    options[args[index].slice(2).replaceAll("-", "_")] = args[index + 1];
  }
  return {
    item: options.item,
    settings: options.settings,
    runs: options.runs,
  };
}

async function settingsRemote(path) {
  return parseYaml(await readFile(path, "utf8")).config.remote;
}

async function loadConfig(dir) {
  return {
    repos: await loadMap(join(dir, "repos.yaml"), "repos"),
    roles: await loadNamed(join(dir, "roles")),
    assignments: await loadNamed(join(dir, "assignments")),
    pipelines: await loadNamed(join(dir, "pipelines")),
  };
}

async function loadMap(path, key) {
  return parseYaml(await readFile(path, "utf8"))[key] ?? {};
}

async function loadNamed(dir) {
  const values = {};
  for (const file of await yamlFiles(dir)) {
    const value = parseYaml(await readFile(file, "utf8"));
    values[value.name ?? parse(file).name] = value;
  }
  return values;
}

async function yamlFiles(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && [".yaml", ".yml"].includes(extname(entry.name)))
    .map((entry) => join(dir, entry.name));
}

function fixtureErrors(dir, config) {
  const errors = [];
  for (const [name, pipeline] of Object.entries(config.pipelines)) {
    for (const step of pipeline.steps ?? []) {
      checkFixture(errors, dir, name, step, config.roles[step.role]);
    }
  }
  return errors;
}

function checkFixture(errors, dir, pipeline, step, role) {
  if (step.fixture == null) {
    return;
  }
  if (role?.adapter !== "fake") {
    errors.push(error(dir, pipeline, step.name, "`fixture` requires a role with the `fake` adapter"));
  }
  if (!isAbsolute(step.fixture)) {
    errors.push(error(dir, pipeline, step.name, "`fixture` must be an absolute path"));
  }
}

function error(dir, pipeline, step, message) {
  return {
    path: join("pipelines", `${pipeline}.yaml`),
    message: `pipeline \`${pipeline}\` step \`${step}\`: ${message}`,
  };
}
