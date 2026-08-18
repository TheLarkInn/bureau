import { stat } from "node:fs/promises";
import { basename, resolve } from "node:path";
import { trustFindings } from "./trust.mjs";

const SOURCE = "advisory";
const TERMINALS = new Set(["done", "abort", "escalate"]);

export async function advisories(payload, options = {}) {
  const config = payload?.config;
  if (!config) {
    return [];
  }
  return [
    ...(await agentAdvisories(config, payload.dir, options)),
    ...(await scriptAdvisories(config, payload.dir, options)),
    ...checkAdvisories(config),
  ];
}

export function pluginPicker() {
  return {
    enumerable: false,
    items: [],
    reason: "bureau-plugin has no read-only enumeration surface for resolved plugins and agents.",
  };
}

async function agentAdvisories(config, dir, options) {
  if (!options.resolveAgent) {
    return [];
  }
  const found = [];
  for (const [name, role] of Object.entries(config.roles ?? {})) {
    const resolved = await options.resolveAgent(role.agent, { dir, role: name });
    if (!resolved?.ok) {
      found.push(advisory("agent-resolution", rolePath(name), resolved?.message ?? `role \`${name}\`: agent \`${role.agent}\` does not resolve`, {
        kind: "role",
        role: name,
      }));
    }
  }
  return found;
}

async function scriptAdvisories(config, dir, options) {
  const root = options.repoRoot ?? repoRoot(dir);
  const found = [];
  for (const [pipeline, value] of Object.entries(config.pipelines ?? {})) {
    for (const step of value.steps ?? []) {
      const script = scriptToken(step.run);
      if (step.type === "deterministic" && script && !(await exists(resolve(root, script)))) {
        found.push(advisory("script-advisory", pipelinePath(pipeline), `pipeline \`${pipeline}\` step \`${step.name}\`: run script \`${script}\` does not exist`, {
          kind: "step",
          pipeline,
          step: step.name,
        }));
      }
    }
  }
  return found;
}

function checkAdvisories(config) {
  const found = [];
  for (const [pipeline, value] of Object.entries(config.pipelines ?? {})) {
    const byName = new Map((value.steps ?? []).map((step) => [step.name, step]));
    for (const step of value.steps ?? []) {
      if (needsMachineCheck(step, config.roles) && !hasDownstreamDeterministic(step, byName)) {
        found.push(advisory("check-advisory", pipelinePath(pipeline), `pipeline \`${pipeline}\` step \`${step.name}\`: no downstream deterministic check was found`, {
          kind: "step",
          pipeline,
          step: step.name,
        }));
      }
    }
  }
  return found;
}

function needsMachineCheck(step, roles) {
  return step.type === "agent" && roleWriteCapable(roles?.[step.role]);
}

function roleWriteCapable(role) {
  return Boolean(role) && trustFindings(writeProbe(role)).length > 0;
}

function writeProbe(role) {
  return {
    config: {
      roles: { probe: { ...role, name: "probe" } },
      pipelines: {
        probe: {
          name: "probe",
          steps: [
            { name: "source", type: "deterministic", run: "true", next: "target" },
            { name: "target", type: "agent", role: "probe", trust: "untrusted", inputs_from: ["source"], next: "done" },
          ],
        },
      },
    },
  };
}

function hasDownstreamDeterministic(step, byName) {
  const pending = controlTargets(step);
  const seen = new Set();
  while (pending.length > 0) {
    const name = pending.shift();
    if (TERMINALS.has(name) || seen.has(name)) {
      continue;
    }
    seen.add(name);
    const next = byName.get(name);
    if (!next) {
      continue;
    }
    if (next.type === "deterministic") {
      return true;
    }
    pending.push(...controlTargets(next));
  }
  return false;
}

function controlTargets(step) {
  return [step.next, step.on_failure, step.on_blocked, step.on_no_work, ...Object.values(step.on ?? {})].filter(Boolean);
}

function scriptToken(run) {
  return tokens(run).find((token) => !token.startsWith("-") && !/^[A-Za-z0-9_-]+$/u.test(token)) ?? null;
}

function tokens(run) {
  return [...String(run ?? "").matchAll(/"([^"]+)"|'([^']+)'|(\S+)/gu)].map((match) => match.slice(1).find(Boolean));
}

async function exists(path) {
  return stat(path).then(
    (info) => info.isFile(),
    () => false,
  );
}

function repoRoot(dir) {
  return basename(resolve(dir)) === ".bureau" ? resolve(dir, "..") : resolve(dir);
}

function advisory(marker, path, message, target) {
  return { source: SOURCE, marker, path, message, target };
}

function rolePath(name) {
  return `roles/${name}.yaml`;
}

function pipelinePath(name) {
  return `pipelines/${name}.yaml`;
}
