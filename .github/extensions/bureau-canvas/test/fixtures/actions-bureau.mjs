import { readFile } from "node:fs/promises";
import { join } from "node:path";
import * as YAML from "../../lib/vendor/yaml.mjs";

const [, , command, dir, jsonFlag] = process.argv;

if (command !== "validate" || jsonFlag !== "--json") {
  process.stderr.write("expected: validate <dir> --json\n");
  process.exit(64);
}

const pipeline = await readPipeline(dir);
const state = await readState(dir);
const payload = pipeline ? payloadFromPipeline(dir, pipeline) : statePayload(dir, state);
process.stdout.write(`${JSON.stringify(payload)}\n`);
process.exit(payload.ok ? 0 : 1);

async function readState(dir) {
  const text = await readFile(join(dir, "actions-state.json"), "utf8").catch(() => "{}");
  return JSON.parse(text);
}

async function readPipeline(dir) {
  const text = await readFile(join(dir, "pipelines", "agent-eligible-pipeline.yaml"), "utf8").catch(() => null);
  return text ? YAML.parse(text) : null;
}

function statePayload(dir, state) {
  return state.valid === false ? invalidPayload(dir) : validPayload(dir, state.agent ?? "/bureau:implementer");
}

function payloadFromPipeline(dir, pipeline) {
  const errors = pipelineErrors(pipeline);
  return errors.length === 0 ? validPayload(dir, "/bureau:implementer", pipeline) : { ok: false, dir, errors, config: null };
}

function pipelineErrors(pipeline) {
  return [
    ...fieldErrors(pipeline),
    ...targetErrors(pipeline),
  ];
}

function fieldErrors(pipeline) {
  return (pipeline.steps ?? []).flatMap((item) =>
    item.name === "verify" && item.type === "deterministic" && item.role
      ? [{ path: "pipelines/agent-eligible-pipeline.yaml", message: "pipeline `agent-eligible-pipeline` step `verify`: `role` does not apply to deterministic steps" }]
      : [],
  );
}

function targetErrors(pipeline) {
  const names = new Set([...(pipeline.steps ?? []).map((item) => item.name), "done", "abort", "escalate"]);
  return (pipeline.steps ?? []).flatMap((item) =>
    ["next", "on_failure", "on_blocked", "on_no_work"].flatMap((field) =>
      item[field] && !names.has(item[field])
        ? [{ path: "pipelines/agent-eligible-pipeline.yaml", message: `pipeline \`agent-eligible-pipeline\` step \`${item.name}\`: unknown ${field} target \`${item[field]}\`` }]
        : [],
    ),
  );
}

function validPayload(dir, agent, pipeline = defaultPipeline()) {
  return {
    ok: true,
    dir,
    errors: [],
    config: {
      repos: {
        bureau: { url: "https://github.com/TheLarkInn/bureau.git", forge: "github", access: "push" },
      },
      roles: {
        implementer: { name: "implementer", agent, adapter: "copilot", permissions: ["repo:read"] },
        unused: { name: "unused", agent: "/bureau:unused", adapter: "copilot", permissions: [] },
      },
      assignments: {
        eligible: {
          name: "eligible",
          work: { forge: "github", source: "TheLarkInn/bureau", filter: "is:open" },
          repos: ["bureau"],
          pipeline: "agent-eligible-pipeline",
          role: "implementer",
        },
      },
      pipelines: {
        "agent-eligible-pipeline": pipeline,
      },
    },
  };
}

function defaultPipeline() {
  return {
    name: "agent-eligible-pipeline",
    steps: [
      step("start", "deterministic", { run: "cargo test --offline", next: "decide" }),
      step("decide", "decision", { over: "start", on: outcomes() }),
      step("verify", "deterministic", { run: "cargo test --offline", inputs_from: ["start"], next: "done" }),
    ],
  };
}

function invalidPayload(dir) {
  return {
    ok: false,
    dir,
    errors: [{ path: "pipelines/agent-eligible-pipeline.yaml", message: "pipeline `agent-eligible-pipeline`: changed" }],
    config: null,
  };
}

function step(name, type, fields) {
  return {
    name,
    type,
    on: {},
    steps: [],
    inputs_from: [],
    max_attempts: 1,
    ...fields,
  };
}

function outcomes() {
  return { success: "verify", failure: "abort", blocked: "abort", "no-work": "done" };
}
