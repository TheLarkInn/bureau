// `create`, `delete` and `rename` for every config entity.
//
// Three verbs taking a `kind` rather than fifteen named actions: one schema per
// verb, one place to enforce the shared rules.
//
// Every mutation is a *plan* until an explicit save. Nothing here touches the
// config directory — a delete that landed immediately would be the one edit
// reloading cannot undo. Legality is still `bureau validate`'s call; this
// module reports consequences and produces file text.

import { createText, renameReference, withDeclaredName, withRepo, withoutRepo } from "./entities.mjs";
import { deleteFile, listNames, pathFor } from "./files.mjs";
import { blocksDelete, referrers } from "./preflight.mjs";

const KIND_ENUM = ["repo", "role", "assignment", "pipeline"];

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
