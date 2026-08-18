import { readFile } from "node:fs/promises";
import { join } from "node:path";

const [, , command, dir, jsonFlag] = process.argv;

if (command !== "validate" || jsonFlag !== "--json") {
  process.stderr.write("expected: validate <dir> --json\n");
  process.exit(64);
}

const state = await readState(dir);
const payload = state.valid === false ? invalidPayload(dir) : validPayload(dir, state.agent ?? "/bureau:implementer");
process.stdout.write(`${JSON.stringify(payload)}\n`);
process.exit(payload.ok ? 0 : 1);

async function readState(dir) {
  const text = await readFile(join(dir, "actions-state.json"), "utf8").catch(() => "{}");
  return JSON.parse(text);
}

function validPayload(dir, agent) {
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
        "agent-eligible-pipeline": {
          name: "agent-eligible-pipeline",
          steps: [
            step("start", "deterministic", { run: "cargo test --offline", next: "decide" }),
            step("decide", "decision", { over: "start", on: outcomes() }),
            step("verify", "deterministic", { run: "cargo test --offline", inputs_from: ["start"], next: "done" }),
          ],
        },
      },
    },
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
