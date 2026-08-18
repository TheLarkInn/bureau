import { findings as loadFindings } from "./findings.mjs";
import { configView, pipelineView } from "./view.mjs";

const SUBJECT_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string", description: "Config directory to read." },
    pipeline: { type: "string", description: "Pipeline to describe or select." },
  },
};

const FOCUS_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    kind: {
      type: "string",
      enum: ["config", "repo", "role", "assignment", "pipeline", "step", "terminal", "file"],
    },
    name: { type: "string" },
    pipeline: { type: "string" },
    step: { type: "string" },
    dir: { type: "string" },
  },
  required: ["kind"],
};

export const actions = [
  {
    name: "describe",
    description: "Describe the current Bureau config or one pipeline, including findings.",
    inputSchema: SUBJECT_SCHEMA,
    handler: describe,
  },
  {
    name: "focus",
    description: "Focus a config item in the open Bureau canvas without reloading it.",
    inputSchema: FOCUS_SCHEMA,
    handler: focus,
  },
  {
    name: "reload",
    description: "Reload Bureau config from disk and optionally switch the selected pipeline.",
    inputSchema: SUBJECT_SCHEMA,
    handler: reload,
  },
];

export async function describe(ctx, deps = {}) {
  const subject = subjectFor(ctx, deps);
  const result = await loader(deps)(subject.dir);
  return described(result, subject);
}

export async function focus(ctx, deps = {}) {
  const subject = subjectFor(ctx, deps);
  const focusItem = focusItemFor(ctx.input ?? {});
  const payload = { focus: focusItem, subject };
  rememberSubject(ctx, deps, subject);
  await publisher(deps)(ctx.instanceId, "focus", payload);
  return payload;
}

export async function reload(ctx, deps = {}) {
  const subject = subjectFor(ctx, deps);
  const result = await loader(deps)(subject.dir);
  const payload = described(result, subject);
  rememberSubject(ctx, deps, subject);
  await publisher(deps)(ctx.instanceId, "state", payload);
  return payload;
}

function described(result, subject) {
  const scope = subject.pipeline ? "pipeline" : "config";
  return {
    scope,
    subject,
    ok: result.ok,
    state: result.state,
    dir: result.dir,
    errors: result.errors ?? [],
    findings: result.findings ?? [],
    view: scope === "pipeline" ? pipelineView(result, subject.pipeline) : configView(result),
  };
}

function subjectFor(ctx, deps) {
  const saved = deps.getSubject?.(ctx.instanceId) ?? {};
  const input = ctx.input ?? {};
  const pipeline = input.pipeline ?? pipelineName(input) ?? saved.pipeline ?? null;
  return {
    dir: input.dir ?? saved.dir ?? ".bureau",
    ...(pipeline ? { pipeline } : {}),
  };
}

function focusItemFor(input) {
  const item = { kind: input.kind };
  copyIfPresent(item, "name", input.name);
  copyIfPresent(item, "pipeline", input.pipeline);
  copyIfPresent(item, "step", input.step ?? stepName(input));
  return item;
}

function pipelineName(input) {
  return input.kind === "pipeline" ? input.name : undefined;
}

function stepName(input) {
  return input.kind === "step" ? input.name : undefined;
}

function copyIfPresent(target, name, value) {
  if (value != null) {
    target[name] = value;
  }
}

function rememberSubject(ctx, deps, subject) {
  deps.setSubject?.(ctx.instanceId, subject);
}

function loader(deps) {
  return deps.loadFindings ?? loadFindings;
}

function publisher(deps) {
  return deps.publish ?? (() => undefined);
}
