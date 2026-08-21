// `create`, `delete` and `rename` for every config entity.
//
// Three verbs taking a `kind` rather than fifteen named actions: one schema per
// verb, one place to enforce the shared rules.
//
// Every mutation is a *plan* until an explicit save. Nothing here touches the
// config directory — a delete that landed immediately would be the one edit
// reloading cannot undo. Legality is still `bureau validate`'s call; this
// module reports consequences and produces file text.

import { choicesFor, createText, renameReference, withAssignmentLimits, withAssignmentRepos, withAssignmentRole, withDeclaredName, withRepo, withoutRepo } from "./entities.mjs";
import { deleteFile, listNames, pathFor } from "./files.mjs";
import { blocksDelete, referrers } from "./preflight.mjs";

const KIND_ENUM = ["repo", "role", "assignment", "pipeline"];
/** Sourced from the entity descriptors, which mirror the Rust enums. */
const PERMISSION_ENUM = choicesFor("role", "permissions") ?? [];

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

const SET_LIMITS_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string" },
    limits: {
      type: "object",
      additionalProperties: false,
      properties: Object.fromEntries(LIMIT_KEYS.map((key) => [key, { type: ["number", "null"], minimum: 1 }])),
    },
  },
  required: ["assignment", "limits"],
};

const SET_ROLE_SCHEMA = {
  type: "object",
  additionalProperties: false,
  properties: {
    dir: { type: "string" },
    assignment: { type: "string" },
    role: { type: "string", description: "The role the assignment names." },
    create: {
      type: "object",
      additionalProperties: false,
      description: "Fields for a new roles/<role>.yaml written in the same plan.",
      properties: {
        agent: { type: "string", description: "A plugin invocation like /plugin:agent, or a path to an agent .md." },
        adapter: { type: "string", enum: ["copilot", "claude", "fake"] },
        permissions: { type: "array", items: { type: "string", enum: PERMISSION_ENUM } },
        min_trust: { type: "string", enum: ["untrusted", "derived", "maintainer", "trusted"] },
      },
      required: ["agent", "adapter", "permissions", "min_trust"],
    },
  },
  required: ["assignment", "role"],
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
    name: "set_role",
    description: "Plan the role an assignment names, optionally creating that role in the same plan.",
    inputSchema: SET_ROLE_SCHEMA,
    handler: setRole,
  },
  {
    name: "set_limits",
    description: "Plan an assignment's limits block. A null disables one limit, which means unlimited.",
    inputSchema: SET_LIMITS_SCHEMA,
    handler: setLimits,
  },
];

export function emptyPlan() {
  return { writes: [], removals: [], notes: [] };
}

export async function create(ctx, deps = {}) {
  const { dir, plan } = await context(ctx, deps);
  const { kind, name, fields = {} } = ctx.input;
  const next = kind === "repo"
    ? { ...plan, writes: [...plan.writes, await repoWrite(dir, plan, name, fields)] }
    : { ...plan, writes: [...plan.writes, { path: pathFor(dir, kind, name), text: createText(kind, name, fields) }] };
  return record(ctx, deps, next, { action: "create", kind, name });
}

export async function remove(ctx, deps = {}) {
  const { dir, plan, payload } = await context(ctx, deps);
  const { kind, name, pipeline, confirm } = ctx.input;
  const found = referrers(payload, kind, name, { pipeline });
  if (!confirm) {
    return { action: "delete", kind, name, confirmed: false, referrers: found, blocking: blocksDelete(found) };
  }
  const next = kind === "repo"
    ? { ...plan, writes: [...plan.writes, await repoWrite(dir, plan, name, null)] }
    : { ...plan, removals: [...plan.removals, { path: pathFor(dir, kind, name), kind, name }] };
  return record(ctx, deps, next, { action: "delete", kind, name, confirmed: true, referrers: found });
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
  const text = withAssignmentLimits(await read(deps, path), path, fullLimits(limits));
  const next = { ...plan, writes: [...plan.writes, { path, text }] };
  return record(ctx, deps, next, { action: "set_limits", assignment, limits: fullLimits(limits) });
}

function fullLimits(limits) {
  return Object.fromEntries(LIMIT_KEYS.map((key) => [key, limits[key] ?? null]));
}

/**
 * The role an assignment names, and the role file itself when one is being
 * created, in a single plan. Creating never overwrites an existing role: it
 * is shared, so silently changing its grants would change what every
 * pipeline step naming it may do.
 */
export async function setRole(ctx, deps = {}) {
  const { dir, plan, payload } = await context(ctx, deps);
  const { assignment, role, create } = ctx.input;
  const created = create ? [roleWrite(dir, payload, role, create)] : [];
  const path = pathFor(dir, "assignment", assignment);
  const text = withAssignmentRole(await read(deps, path), path, role);
  const next = { ...plan, writes: [...plan.writes, ...created, { path, text }] };
  return record(ctx, deps, next, { action: "set_role", assignment, role, created: Boolean(create) });
}

function roleWrite(dir, payload, role, fields) {
  const known = payload?.config?.roles ?? {};
  if (Object.hasOwn(known, role)) {
    throw new Error(`\`${role}\` already exists; choose it instead of creating it`);
  }
  return { path: pathFor(dir, "role", role), text: createText("role", role, fields) };
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
  const registered = register ? [await registerWrite(dir, plan, payload, register)] : [];
  const path = pathFor(dir, "assignment", assignment);
  const text = withAssignmentRepos(await read(deps, path), path, repos);
  const next = { ...plan, writes: [...plan.writes, ...registered, { path, text }] };
  return record(ctx, deps, next, { action: "set_repos", assignment, repos, registered: Boolean(register) });
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

/** Applies a plan to disk. Only a save calls this. */
export async function applyPlan(dir, plan, deps = {}) {
  const { writeFile } = await import("node:fs/promises");
  const write = deps.writeText ?? writeFile;
  for (const entry of plan.writes) {
    await write(entry.path, entry.text);
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
