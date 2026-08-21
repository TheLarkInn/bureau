// `create`, `delete` and `rename` for every config entity.
//
// Three verbs taking a `kind` rather than fifteen named actions: one schema per
// verb, one place to enforce the shared rules.
//
// Every mutation is a *plan* until an explicit save. Nothing here touches the
// config directory — a delete that landed immediately would be the one edit
// reloading cannot undo. Legality is still `bureau validate`'s call; this
// module reports consequences and produces file text.

import { createText, renameReference, repoNames, withAssignmentLimits, withAssignmentRepos, withAssignmentRuntime, withAssignmentWork, withDeclaredName, withRepo, withoutRepo } from "./entities.mjs";
import { resolve } from "node:path";
import { createFile, deleteFile, listNames, pathFor } from "./files.mjs";
import { blocksDelete, referrers } from "./preflight.mjs";

const KIND_ENUM = ["repo", "role", "assignment", "pipeline"];

/**
 * The whole ordered list in one call, plus an optional registry entry to
 * create alongside it. Adding an unlisted repo touches two files — the
 * registry is shared by every assignment — and both belong to one plan so
 * neither can land without the other.
 */
/**
 * Points an assignment at a role, optionally creating that role in the same
 * plan. Roles are shared with every pipeline step that names them, so this
 * only chooses or creates: renaming and deleting belong to the role itself,
 * where all its referrers are visible.
 */
/**
 * Every limit an assignment may set. A `null` disables one, which means
 * unlimited — except `max_run_hours`, where it means the system default.
 * Counts are at least 1: a zero would compute headroom as permanently zero,
 * which reads as a typo but behaves as a pause.
 */
const LIMIT_KEYS = [
  "max_concurrent", "max_runs_per_hour", "max_runs_per_day",
  "max_open_prs", "max_cost_per_day_usd", "max_run_hours",
];
const U32_LIMITS = new Set([
  "max_concurrent", "max_runs_per_hour", "max_runs_per_day", "max_open_prs",
]);
const LIMIT_SCHEMAS = Object.fromEntries(LIMIT_KEYS.map((key) => {
  if (key === "max_cost_per_day_usd") {
    return [key, { type: ["number", "null"], exclusiveMinimum: 0 }];
  }
  return [key, {
    type: ["integer", "null"],
    minimum: 1,
    maximum: U32_LIMITS.has(key) ? 4_294_967_295 : Number.MAX_SAFE_INTEGER,
  }];
}));

const SET_LIMITS_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string" },
    limits: {
      type: "object",
      additionalProperties: false,
      properties: LIMIT_SCHEMAS,
    },
  },
  required: ["assignment", "limits"],
};

const SET_WORK_SOURCE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string" },
    work: {
      type: "object",
      additionalProperties: false,
      properties: {
        forge: { type: "string", enum: ["github", "ado"] },
        source: { type: "string" },
        filter: { type: "string" },
        approval_label: { type: ["string", "null"] },
        abort_label: { type: "string", minLength: 1 },
        escalate_label: { type: "string", minLength: 1 },
      },
      required: ["forge", "source", "filter", "approval_label", "abort_label", "escalate_label"],
    },
  },
  required: ["assignment", "work"],
};

const SET_ASSIGNMENT_RUNTIME_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string" },
    fields: {
      type: "object",
      additionalProperties: false,
      properties: {
        filter: { type: "string", minLength: 1 },
        approval_label: { type: ["string", "null"] },
        abort_label: { type: "string", minLength: 1 },
        escalate_label: { type: "string", minLength: 1 },
        branch_prefix: { type: "string", minLength: 1 },
      },
      required: ["filter", "approval_label", "abort_label", "escalate_label", "branch_prefix"],
    },
  },
  required: ["assignment", "fields"],
};

const SET_REPOS_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string" },
    repos: { type: "array", items: { type: "string" }, description: "The ordered list; the first entry is the primary repo." },
    register: {
      type: "object",
      additionalProperties: false,
      description: "A new repos.yaml entry to write in the same plan.",
      properties: {
        name: { type: "string" },
        url: { type: "string" },
        forge: { type: "string", enum: ["github", "ado"] },
        access: { type: "string", enum: ["read", "pr", "push"] },
        credential: { type: "string" },
      },
      required: ["name", "url", "forge", "access", "credential"],
    },
  },
  required: ["assignment", "repos"],
};

const CREATE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    kind: { type: "string", enum: KIND_ENUM },
    name: { type: "string" },
    fields: { type: "object" },
  },
  required: ["kind", "name"],
};

const DELETE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    kind: { type: "string", enum: [...KIND_ENUM, "step"] },
    name: { type: "string" },
    pipeline: { type: "string" },
    confirm: { type: "boolean", description: "Required. Deleting is not undone by reload once saved." },
  },
  required: ["kind", "name"],
};

const RENAME_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    kind: { type: "string", enum: ["role", "pipeline"] },
    from: { type: "string" },
    to: { type: "string" },
  },
  required: ["kind", "from", "to"],
};

export const crudActions = [
  {
    name: "create",
    description: "Plan a new repo, role, assignment or pipeline, scaffolded so it is valid on arrival.",
    inputSchema: CREATE_SCHEMA,
    handler: create,
  },
  {
    name: "delete",
    description: "Plan deleting a config entity. Reports what references it and requires confirmation.",
    inputSchema: DELETE_SCHEMA,
    handler: remove,
  },
  {
    name: "rename",
    description: "Plan renaming a role or pipeline, cascading to everything that references it.",
    inputSchema: RENAME_SCHEMA,
    handler: rename,
  },
  {
    name: "set_repos",
    description: "Plan an assignment's ordered repos list, optionally registering a new repo in repos.yaml in the same plan.",
    inputSchema: SET_REPOS_SCHEMA,
    handler: setRepos,
  },
  {
    name: "set_limits",
    description: "Plan an assignment's limits block. A null disables one limit, which means unlimited.",
    inputSchema: SET_LIMITS_SCHEMA,
    handler: setLimits,
  },
  {
    name: "plan_work_source",
    description: "Plan an assignment's forge-native work-source block.",
    inputSchema: SET_WORK_SOURCE_SCHEMA,
    handler: setWorkSourcePlan,
  },
  {
    name: "set_assignment_runtime",
    description: "Plan an assignment's work rules, terminal labels, and branch prefix.",
    inputSchema: SET_ASSIGNMENT_RUNTIME_SCHEMA,
    handler: setAssignmentRuntime,
  },
];

export function emptyPlan() {
  return { writes: [], removals: [], notes: [] };
}

export async function create(ctx, deps = {}) {
  const { dir, plan, payload } = await context(ctx, deps);
  const { kind, name, fields = {} } = ctx.input;
  const collection = kind === "repo" ? payload?.config?.repos : payload?.config?.[`${kind}s`];
  if (Object.hasOwn(collection ?? {}, name)) {
    throw new Error(`\`${name}\` already exists; choose another name`);
  }
  const target = pathFor(dir, kind, name);
  const pending = plan.writes.findLast((write) => write.path === target);
  if (kind === "repo" && pending && repoNames(pending.text).includes(name)) {
    throw new Error(`\`${name}\` already has a pending create`);
  }
  if (kind !== "repo" && pending) {
    throw new Error(`\`${name}\` already has a pending create`);
  }
  const next = kind === "repo"
    ? withWrites(plan, [await repoWrite(dir, plan, name, fields)])
    : { ...plan, writes: [...plan.writes, { path: target, text: createText(kind, name, fields), create: true }] };
  return record(ctx, deps, next, { action: "create", kind, name });
}

export async function remove(ctx, deps = {}) {
  const { dir, plan, payload } = await context(ctx, deps);
  const { kind, name, pipeline, confirm } = ctx.input;
  const found = referrers(payload, kind, name, { pipeline })
    .filter((item) => !coveredByRemoval(item, plan.removals));
  if (!confirm) {
    return { action: "delete", kind, name, confirmed: false, referrers: found, blocking: blocksDelete(found) };
  }
  if (blocksDelete(found)) {
    throw new Error(`cannot delete \`${name}\` while ${found.length} reference${found.length === 1 ? "" : "s"} still point at it`);
  }
  const next = kind === "repo"
    ? withWrites(plan, [await repoWrite(dir, plan, name, null)])
    : { ...plan, removals: [...plan.removals, { path: pathFor(dir, kind, name), kind, name }] };
  return record(ctx, deps, next, { action: "delete", kind, name, confirmed: true, referrers: found });
}

function coveredByRemoval(referrer, removals) {
  return removals.some((entry) =>
    (entry.kind === referrer.kind && entry.name === referrer.name)
    || (referrer.kind === "step" && entry.kind === "pipeline" && referrer.name.startsWith(`${entry.name}/`)));
}

export async function rename(ctx, deps = {}) {
  const { dir, plan, payload } = await context(ctx, deps);
  const { kind, from, to } = ctx.input;
  const cascade = await cascadeWrites(dir, payload, kind, from, to, deps);
  const source = pathFor(dir, kind, from);
  const text = withDeclaredName(await read(deps, source), source, to);
  const next = {
    ...plan,
    // All or nothing: a rename whose referrers are left stale is worse than one
    // that never happened.
    writes: [...plan.writes, ...cascade, { path: pathFor(dir, kind, to), text }],
    removals: [...plan.removals, { path: source, kind, name: from }],
  };
  return record(ctx, deps, next, { action: "rename", kind, from, to, cascaded: cascade.length });
}

/**
 * The assignment's limits block. Every key is written, disabled ones as
 * `null`, so the block keeps a stable shape and any comment above a key
 * stays anchored to it.
 */
export async function setLimits(ctx, deps = {}) {
  const { dir, plan } = await context(ctx, deps);
  const { assignment, limits } = ctx.input;
  const path = pathFor(dir, "assignment", assignment);
  const text = withAssignmentLimits(await plannedText(deps, plan, path), path, fullLimits(limits));
  const next = withWrites(plan, [{ path, text }]);
  return record(ctx, deps, next, { action: "set_limits", assignment, limits: fullLimits(limits) });
}

export async function setWorkSourcePlan(ctx, deps = {}) {
  const { assignment } = ctx.input;
  const work = validatedTerminalLabels(ctx.input.work);
  const { dir, plan } = await context(ctx, deps);
  const path = pathFor(dir, "assignment", assignment);
  const text = withAssignmentWork(await plannedText(deps, plan, path), path, work);
  const next = withWrites(plan, [{ path, text }]);
  return record(ctx, deps, next, { action: "plan_work_source", assignment, work });
}

export async function setAssignmentRuntime(ctx, deps = {}) {
  const { assignment } = ctx.input;
  const fields = validatedTerminalLabels(ctx.input.fields);
  const { dir, plan } = await context(ctx, deps);
  const path = pathFor(dir, "assignment", assignment);
  const text = withAssignmentRuntime(await plannedText(deps, plan, path), path, fields);
  const next = withWrites(plan, [{ path, text }]);
  return record(ctx, deps, next, { action: "set_assignment_runtime", assignment, fields });
}

function validatedTerminalLabels(fields) {
  const abort = fields.abort_label?.trim();
  const escalate = fields.escalate_label?.trim();
  if (!abort || !escalate) {
    throw new Error("abort and escalation labels are required");
  }
  if (abort.toLowerCase() === escalate.toLowerCase()) {
    throw new Error("abort and escalation labels must differ");
  }
  return { ...fields, abort_label: abort, escalate_label: escalate };
}

function fullLimits(limits) {
  return Object.fromEntries(LIMIT_KEYS.map((key) => [key, limits[key] ?? null]));
}

/**
 * The assignment's list, and the registry entry when one is being added, in
 * a single plan. Registering never edits an entry that already exists: the
 * registry is shared, so silently changing a repo's access would change what
 * every other assignment using it may do.
 */
export async function setRepos(ctx, deps = {}) {
  const { dir, plan, payload } = await context(ctx, deps);
  const { assignment, repos, register } = ctx.input;
  assertRegisterAvailable(payload, register);
  assertLandingRepo(payload, repos, register);
  const registered = register ? [await registerWrite(dir, plan, payload, register)] : [];
  const path = pathFor(dir, "assignment", assignment);
  const text = withAssignmentRepos(await plannedText(deps, plan, path), path, repos);
  const next = withWrites(plan, [...registered, { path, text }]);
  return record(ctx, deps, next, { action: "set_repos", assignment, repos, registered: Boolean(register) });
}

function assertRegisterAvailable(payload, register) {
  if (register && Object.hasOwn(payload?.config?.repos ?? {}, register.name)) {
    throw new Error(`\`${register.name}\` is already in the registry; rename this one or use the existing entry`);
  }
}

function assertLandingRepo(payload, repos, register) {
  if (repos.length === 0) {
    throw new Error("an assignment needs at least one repo; the first is where the branch lands");
  }
  const known = { ...(payload?.config?.repos ?? {}) };
  if (register) {
    known[register.name] = register;
  }
  const primary = known[repos[0]];
  if (!primary) {
    throw new Error(`primary repo \`${repos[0]}\` is not in repos.yaml`);
  }
  if (!["pr", "push"].includes(primary.access)) {
    throw new Error(`primary repo \`${repos[0]}\` has \`${primary.access}\` access; the branch needs pr or push access`);
  }
}

async function registerWrite(dir, plan, payload, register) {
  const known = payload?.config?.repos ?? {};
  if (Object.hasOwn(known, register.name)) {
    throw new Error(`\`${register.name}\` is already in the registry; rename this one or use the existing entry`);
  }
  const { name, ...fields } = register;
  return repoWrite(dir, plan, name, fields);
}

async function cascadeWrites(dir, payload, kind, from, to, deps) {
  const writes = [];
  for (const referring of await referringPaths(dir, payload)) {
    const text = await read(deps, referring);
    const updated = renameReference(text, referring, kind, from, to);
    if (updated) {
      writes.push({ path: referring, text: updated });
    }
  }
  return writes;
}

async function referringPaths(dir, payload) {
  const paths = [];
  for (const kind of ["assignment", "pipeline"]) {
    const names = Object.keys(payload?.config?.[`${kind}s`] ?? {});
    const known = names.length > 0 ? names : await listNames(dir, kind);
    paths.push(...known.map((name) => pathFor(dir, kind, name)));
  }
  return paths;
}

async function repoWrite(dir, plan, name, fields) {
  const path = pathFor(dir, "repo");
  const current = plan.writes.find((write) => write.path === path)?.text ?? (await readOrEmpty(path));
  return { path, text: fields ? withRepo(current, name, fields) : withoutRepo(current, name) };
}

async function readOrEmpty(path) {
  const { readFile } = await import("node:fs/promises");
  return readFile(path, "utf8").catch(() => "repos: {}\n");
}

async function read(deps, path) {
  if (deps.readText) {
    return deps.readText(path);
  }
  const { readFile } = await import("node:fs/promises");
  return readFile(path, "utf8");
}

async function plannedText(deps, plan, path) {
  return plan.writes.findLast((write) => write.path === path)?.text ?? read(deps, path);
}

function withWrites(plan, writes) {
  const replaced = new Set(writes.map((write) => write.path));
  const next = writes.map((write) => {
    const existing = plan.writes.findLast((candidate) => candidate.path === write.path);
    return existing?.create ? { ...existing, ...write, create: true } : write;
  });
  return { ...plan, writes: [...plan.writes.filter((write) => !replaced.has(write.path)), ...next] };
}

/** Applies a plan to disk. Only a save calls this. */
export async function applyPlan(dir, plan, deps = {}) {
  for (const entry of plan.writes) {
    if (entry.create) {
      await createFile(dir, entry.kind ?? kindForPath(dir, entry.path), entry.name ?? nameForPath(entry.path), entry.text);
    } else {
      const { writeFile } = await import("node:fs/promises");
      const write = deps.writeText ?? writeFile;
      await write(entry.path, entry.text);
    }
  }

  function kindForPath(dir, path) {
    const relative = path.slice(resolve(dir).length + 1).replaceAll("\\", "/");
    return relative.startsWith("roles/") ? "role"
      : relative.startsWith("assignments/") ? "assignment"
        : relative.startsWith("pipelines/") ? "pipeline" : "repo";
  }

  function nameForPath(path) {
    return path.replaceAll("\\", "/").split("/").at(-1).replace(/\.(ya?ml)$/u, "");
  }
  for (const entry of plan.removals) {
    await deleteFile(dir, entry.kind, entry.name);
  }
  return { written: plan.writes.length, removed: plan.removals.length };
}

async function context(ctx, deps) {
  const dir = ctx.input?.dir ?? deps.getSubject?.(ctx.instanceId)?.dir ?? ".bureau";
  const plan = deps.getPlan?.(ctx.instanceId) ?? emptyPlan();
  const payload = deps.loadFindings ? await deps.loadFindings(dir) : { config: null };
  return { dir, plan, payload };
}

function record(ctx, deps, plan, summary) {
  deps.setPlan?.(ctx.instanceId, plan);
  return { ...summary, pending: { writes: plan.writes.map((write) => write.path), removals: plan.removals.map((entry) => entry.path) } };
}
