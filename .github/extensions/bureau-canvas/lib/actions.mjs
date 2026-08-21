import { findings as loadFindings } from "./findings.mjs";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import { advisories as loadAdvisories } from "./advisories.mjs";
import { parse, render } from "./codec.mjs";
import { configView, pipelineView } from "./view.mjs";
import { deriveWorkSource } from "./worksource.mjs";

const SUBJECT_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string", description: "Config directory to read." },
    pipeline: { type: "string", description: "Pipeline to describe or select." },
    role: { type: "string", description: "Role to edit." },
    assignment: { type: "string", description: "Assignment to edit." },
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

const SET_FIELD_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    pipeline: { type: "string" },
    role: { type: "string" },
    step: { type: "string" },
    field: { type: "string", enum: ["run", "role", "trust", "over", "max_attempts", "timeout_secs", "agent"] },
    value: { type: ["string", "number", "null"] },
  },
  required: ["field", "value"],
};

const REWIRE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    pipeline: { type: "string" },
    step: { type: "string" },
    outcome: { type: "string", enum: ["success", "failure", "blocked", "no-work"] },
    target: { type: ["string", "null"] },
  },
  required: ["step", "outcome", "target"],
};

const SAVE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    pipeline: { type: "string" },
    role: { type: "string" },
    assignment: { type: "string" },
    force: { type: "boolean" },
  },
};

/**
 * Either a URL to derive the work source from, or the three fields set
 * explicitly — the manual path a derivation this cannot make must fall back
 * to.
 */
const SET_WORK_SOURCE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string", description: "Assignment whose work source changes." },
    url: { type: "string", description: "A board, query, or issues URL to derive forge, source, and filter from." },
    forge: { type: "string", enum: ["github", "ado"] },
    source: { type: "string" },
    filter: { type: "string" },
    approval_label: { type: ["string", "null"] },
  },
  required: ["assignment"],
};

const FIELD_NAMES = new Map([
  ["run", "run"],
  ["role", "role"],
  ["trust", "trust"],
  ["over", "over"],
  ["max_attempts", "maxAttempts"],
  ["timeout_secs", "timeoutSecs"],
  ["agent", "agent"],
]);

const TERMINALS = new Set(["done", "abort", "escalate"]);

const drafts = new Map();

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
  {
    name: "set_field",
    description: "Set an editable field on a pipeline step or role and validate the draft.",
    inputSchema: SET_FIELD_SCHEMA,
    handler: setField,
  },
  {
    name: "set_work_source",
    description: "Set an assignment's work source from a pasted board/issues URL, or from explicit fields, and validate the draft.",
    inputSchema: SET_WORK_SOURCE_SCHEMA,
    handler: setWorkSource,
  },
  {
    name: "rewire",
    description: "Point a step outcome at another step or terminal and validate the draft.",
    inputSchema: REWIRE_SCHEMA,
    handler: rewire,
  },
  {
    name: "save",
    description: "Write validated draft Bureau YAML back to the working tree.",
    inputSchema: SAVE_SCHEMA,
    handler: save,
  },
];

export async function describe(ctx, deps = {}) {
  const subject = subjectFor(ctx, deps);
  const draft = draftFor(ctx, deps, subject);
  if (draft) {
    return draftPayload(draft, subject);
  }
  const result = await loader(deps)(subject.dir);
  return describeResult(result, subject, deps);
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
  clearDraft(ctx, deps);
  const result = await loader(deps)(subject.dir);
  const payload = await describeResult(result, subject, deps);
  rememberSubject(ctx, deps, subject);
  await publisher(deps)(ctx.instanceId, "state", payload);
  return payload;
}

export async function setField(ctx, deps = {}) {
  return mutate(ctx, deps, (draft, input) => {
    if (draft.view.kind === "document" && input.field === "agent") {
      draft.view.value.agent = input.value;
      return;
    }
    const step = stepFor(draft.view, input.step);
    step.fields[FIELD_NAMES.get(input.field)] = input.value;
  });
}

export async function rewire(ctx, deps = {}) {
  return mutate(ctx, deps, (draft, input) => {
    const key = (edge) => edge.source === input.step && edge.outcome === input.outcome && edge.relation === "control";
    draft.view.edges = input.target == null ? draft.view.edges.filter((edge) => !key(edge)) : upsertEdge(draft.view.edges, input, key);
  });
}

/**
 * Sets an assignment's `work` block, either from a pasted forge URL or from
 * explicit fields. The derivation is refused rather than guessed at when the
 * URL is not one it understands, so a wrong filter never reaches the file.
 */
export async function setWorkSource(ctx, deps = {}) {
  return mutate(ctx, deps, (draft, input) => {
    const work = draft.view.value?.work;
    if (!work || typeof work !== "object") {
      throw new Error(`\`${input.assignment}\` has no \`work\` block to set`);
    }
    Object.assign(work, workChanges(input));
  });
}

/** The `work` fields one call changes, from a URL or from explicit values. */
function workChanges(input) {
  const derived = input.url ? derivedWork(input.url) : {};
  const explicit = pickPresent(input, ["forge", "source", "filter"]);
  const changes = { ...derived, ...explicit };
  if ("approval_label" in input) {
    changes.approval_label = input.approval_label;
  }
  if (Object.keys(changes).length === 0) {
    throw new Error("pass a `url` to derive from, or `forge`/`source`/`filter` to set directly");
  }
  return changes;
}

function derivedWork(url) {
  const result = deriveWorkSource(url);
  if (!result.ok) {
    throw new Error(result.reason);
  }
  return { forge: result.forge, source: result.source, filter: result.filter };
}

function pickPresent(input, keys) {
  return Object.fromEntries(keys.filter((key) => input[key] != null).map((key) => [key, input[key]]));
}

export async function save(ctx, deps = {}) {
  const subject = subjectFor(ctx, deps);
  const draft = draftFor(ctx, deps, subject);
  if (!draft) {
    return reload(ctx, deps);
  }
  const validation = await validateDraft(draft, deps);
  if (!validation.ok && !ctx.input?.force) {
    throw new Error(firstMessage(validation));
  }
  await writer(deps)(draft.path, render(draft.view, draft.doc, draft.style));
  clearDraft(ctx, deps);
  const result = await loader(deps)(subject.dir);
  const payload = { ...(await describeResult(result, subject, deps)), saved: true, path: draft.path, forced: Boolean(ctx.input?.force) };
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

async function describeResult(result, subject, deps) {
  return mergeAdvisories(described(result, subject), await advisor(deps)(result, advisoryOptions(deps)));
}

function mergeAdvisories(payload, items) {
  return { ...payload, findings: [...payload.findings, ...items] };
}

async function mutate(ctx, deps, change) {
  const subject = subjectFor(ctx, deps);
  const original = await editableDraft(ctx, deps, subject);
  const draft = cloneDraft(original);
  change(draft, ctx.input ?? {});
  draft.validation = await validateDraft(draft, deps);
  if (!draft.validation.ok) {
    throw new Error(firstMessage(draft.validation));
  }
  storeDraft(ctx, deps, draft);
  rememberSubject(ctx, deps, subject);
  const payload = { ...draftPayload(draft, subject), dirty: true };
  await publisher(deps)(ctx.instanceId, "state", payload);
  return payload;
}

async function editableDraft(ctx, deps, subject) {
  const draft = draftFor(ctx, deps, subject);
  return draft ?? loadDraft(subject, deps);
}

async function loadDraft(subject, deps) {
  const path = await documentPath(subject, deps);
  const text = await reader(deps)(path, "utf8");
  const parsed = parse(text, { path: relative(resolve(subject.dir), path).replaceAll("\\", "/") });
  return { subject, path, ...parsed, validation: null };
}

function draftPayload(draft, subject) {
  const validation = draft.validation ?? { ok: true, state: "draft", dir: subject.dir, errors: [], findings: [] };
  const scope = draft.view.kind === "pipeline" ? "pipeline" : (subject.assignment ? "assignment" : "role");
  return {
    scope,
    subject,
    ok: validation.ok,
    state: validation.state,
    dir: validation.dir ?? subject.dir,
    errors: validation.errors ?? [],
    findings: validation.findings ?? [],
    view: draft.view,
  };
}

function draftFor(ctx, deps, subject) {
  const draft = getDraft(ctx, deps);
  return draft && sameSubject(draft.subject, subject) ? draft : null;
}

function sameSubject(left, right) {
  return resolve(left.dir) === resolve(right.dir)
    && left.pipeline === right.pipeline
    && left.role === right.role
    && left.assignment === right.assignment;
}

function getDraft(ctx, deps) {
  return deps.getDraft?.(ctx.instanceId) ?? drafts.get(ctx.instanceId);
}

function storeDraft(ctx, deps, draft) {
  if (deps.setDraft) {
    deps.setDraft(ctx.instanceId, draft);
  } else {
    drafts.set(ctx.instanceId, draft);
  }
}

function clearDraft(ctx, deps) {
  if (deps.clearDraft) {
    deps.clearDraft(ctx.instanceId);
  } else {
    drafts.delete(ctx.instanceId);
  }
}

function cloneDraft(draft) {
  return { ...draft, view: structuredClone(draft.view) };
}

function stepFor(view, name) {
  return view.steps.find((step) => step.name === name);
}

function upsertEdge(edges, input, key) {
  const target = targetName(input.target);
  const next = edges.filter((edge) => !key(edge));
  next.push({
    id: `control:${input.step}:${input.outcome}->${target}`,
    source: input.step,
    target,
    relation: "control",
    outcome: input.outcome,
  });
  return next;
}

function targetName(target) {
  return TERMINALS.has(target) ? `terminal:${target}` : target;
}

async function validateDraft(draft, deps) {
  if (deps.validateDraft) {
    return deps.validateDraft(draft);
  }
  const { root, config } = await scratchCopy(draft.subject.dir);
  try {
    const destination = join(config, relative(resolve(draft.subject.dir), draft.path));
    await writer(deps)(destination, render(draft.view, draft.doc, draft.style));
    return loadValidation(config, deps);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

async function loadValidation(dir, deps) {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const result = await loader(deps)(dir);
    if (result.state !== "dir-missing") {
      return result;
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 20));
  }
  return loader(deps)(dir);
}

async function scratchCopy(dir) {
  const source = resolve(dir);
  const root = await mkdtemp(join(tmpdir(), "bureau-canvas-validate-"));
  const config = join(root, "config");
  await cp(source, config, { recursive: true });
  return { root, config };
}

async function documentPath(subject, deps) {
  if (subject.role) {
    return namedPath(subject, deps, "roles", subject.role);
  }
  if (subject.assignment) {
    return namedPath(subject, deps, "assignments", subject.assignment);
  }
  return namedPath(subject, deps, "pipelines", subject.pipeline);
}

/** The `.yaml`/`.yml` file for one named config item, preferring `.yaml`. */
async function namedPath(subject, deps, folder, name) {
  const base = resolve(subject.dir);
  const candidates = [join(base, folder, `${name}.yaml`), join(base, folder, `${name}.yml`)];
  for (const candidate of candidates) {
    if (await exists(deps, candidate)) {
      return candidate;
    }
  }
  return candidates[0];
}

async function exists(deps, path) {
  try {
    await reader(deps)(path, "utf8");
    return true;
  } catch {
    return false;
  }
}

function firstMessage(result) {
  return result.errors?.[0]?.message ?? result.message;
}

function subjectFor(ctx, deps) {
  const saved = deps.getSubject?.(ctx.instanceId) ?? {};
  const input = ctx.input ?? {};
  const role = input.role ?? saved.role ?? null;
  const assignment = input.assignment ?? (role ? null : saved.assignment) ?? null;
  const owned = role || assignment;
  const pipeline = owned ? null : (input.pipeline ?? pipelineName(input) ?? saved.pipeline ?? null);
  return {
    dir: input.dir ?? saved.dir ?? ".bureau",
    ...(pipeline ? { pipeline } : {}),
    ...(role ? { role } : {}),
    ...(assignment && !role ? { assignment } : {}),
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

function advisor(deps) {
  return deps.loadAdvisories ?? loadAdvisories;
}

function advisoryOptions(deps) {
  return { repoRoot: deps.repoRoot, resolveAgent: deps.resolveAgent };
}

function publisher(deps) {
  return deps.publish ?? (() => undefined);
}

function reader(deps) {
  return deps.readText ?? readFile;
}

function writer(deps) {
  return deps.writeText ?? writeFile;
}
