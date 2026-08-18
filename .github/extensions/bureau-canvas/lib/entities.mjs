// Scaffolds and structural edits for every config entity.
//
// Per-kind knowledge lives in the descriptor table below rather than in
// branching code, so adding a field is a data change. Everything here produces
// *text*; nothing writes to the config directory. Applying a set of writes is
// the caller's job, because a mutation is a draft until an explicit save.
//
// Nothing here decides whether an edit is legal — `bureau validate --json`
// owns that, and the canvas never authors an error message.

import { createDocument, parse, render } from "./codec.mjs";

/** Closed sets, mirrored from the Rust enums in `config/files.rs`. */
export const CHOICES = {
  adapter: ["copilot", "claude", "fake"],
  access: ["read", "pr", "push"],
  forge: ["github", "ado"],
  trust: ["untrusted", "derived", "maintainer", "trusted"],
  permission: [
    "repo:read",
    "repo:write",
    "repo:push",
    "issues:read",
    "issues:write",
    "pr:read",
    "pr:write",
    "pr:review",
    "pr:merge",
    "runs:read",
    "model:invoke",
  ],
  stepKind: ["deterministic", "agent", "decision", "concurrent"],
  outcome: ["success", "failure", "blocked", "no-work"],
  completion: ["all", "stop_on_failure"],
};

const DESCRIPTORS = {
  repo: {
    required: ["url", "forge", "access", "credential"],
    choices: { forge: CHOICES.forge, access: CHOICES.access },
    // A credential REFERENCE resolved at spawn; values are never in git.
    hints: { credential: "name of a credential the runner resolves; never a token" },
    scaffold: (_name, fields) => ({
      url: fields.url ?? "",
      forge: fields.forge ?? "github",
      access: fields.access ?? "read",
      credential: fields.credential ?? "",
    }),
  },
  role: {
    required: ["name", "agent", "adapter", "permissions", "min_trust"],
    choices: { adapter: CHOICES.adapter, min_trust: CHOICES.trust, permissions: CHOICES.permission },
    hints: { agent: "a plugin invocation like /plugin:agent, or a path to an agent .md" },
    scaffold: (name, fields) => ({
      name,
      agent: fields.agent ?? `/bureau:${name}`,
      adapter: fields.adapter ?? "copilot",
      permissions: fields.permissions ?? ["repo:read", "model:invoke"],
      min_trust: fields.min_trust ?? "maintainer",
    }),
  },
  assignment: {
    required: ["name", "work", "repos", "pipeline", "role", "verify", "branch_prefix"],
    choices: { "work.forge": CHOICES.forge },
    hints: {
      "work.filter": "forge-native query, passed through verbatim",
      repos: "the first entry is primary — the branch lands there",
      limits: "a kill switch against a runaway loop, not a budget",
    },
    scaffold: (name, fields) => ({
      name,
      work: {
        forge: fields.work?.forge ?? "github",
        source: fields.work?.source ?? "",
        filter: fields.work?.filter ?? "",
        approval_label: fields.work?.approval_label ?? null,
      },
      repos: fields.repos ?? [],
      pipeline: fields.pipeline ?? "",
      role: fields.role ?? "",
      verify: fields.verify ?? "",
      branch_prefix: fields.branch_prefix ?? `${name}/`,
    }),
  },
  pipeline: {
    required: ["name", "steps"],
    choices: {},
    hints: { steps: "the first step is the entry step" },
    // A pipeline with no steps is rejected (`has no steps`), so a new one gets
    // one minimal step and is valid from the moment it exists. That step is
    // therefore the entry step, which makes this choice semantic.
    scaffold: (name, fields) => ({
      name,
      steps: fields.steps ?? [{ name: "start", type: "deterministic", run: "true" }],
    }),
  },
};

export function kindNames() {
  return Object.keys(DESCRIPTORS);
}

export function requiredFields(kind) {
  return descriptor(kind).required.slice();
}

export function fieldHints(kind) {
  return { ...descriptor(kind).hints };
}

export function choicesFor(kind, field) {
  return descriptor(kind).choices[field]?.slice() ?? null;
}

export function scaffold(kind, name, fields = {}) {
  return descriptor(kind).scaffold(name, fields);
}

/** Text for a new file-per-entity config file. */
export function createText(kind, name, fields = {}, lineEnding = "\n") {
  if (kind === "repo") {
    throw new Error("repos live in repos.yaml; use `withRepo`");
  }
  return createDocument(scaffold(kind, name, fields), createdStyleFor(lineEnding));
}

/** Adds or replaces one entry in `repos.yaml`, leaving the others untouched. */
export function withRepo(reposText, name, fields = {}) {
  return editRepos(reposText, (repos) => {
    repos[name] = scaffold("repo", name, fields);
  });
}

export function withoutRepo(reposText, name) {
  return editRepos(reposText, (repos) => {
    delete repos[name];
  });
}

export function repoNames(reposText) {
  return Object.keys(parse(reposText).view.value?.repos ?? {}).sort();
}

function editRepos(reposText, mutate) {
  const parsed = parse(reposText, { path: "repos.yaml" });
  const view = structuredClone(parsed.view);
  view.value.repos = view.value.repos ?? {};
  mutate(view.value.repos);
  const rendered = render(view, parsed.doc, parsed.style);
  return blockStyleRepos(rendered);
}

/**
 * An empty `repos: {}` is a flow map, and the emitter keeps flow style for
 * anything added into it — valid YAML, but it puts every repo on one line and
 * a one-repo change then rewrites the whole file. Re-emit as block style,
 * which is what the committed config uses.
 */
function blockStyleRepos(text) {
  if (!/^repos:\s*\{/mu.test(text)) {
    return text;
  }
  const parsed = parse(text, { path: "repos.yaml" });
  return createDocument(parsed.view.value, createdStyleFor(parsed.style.lineEnding));
}

/** Renames a role or pipeline inside one referring file, if it refers to it. */
export function renameReference(text, path, kind, from, to) {
  const parsed = parse(text, { path });
  const view = structuredClone(parsed.view);
  const changed = view.kind === "pipeline" ? renameInPipeline(view, kind, from, to) : renameInDocument(view, kind, from, to);
  return changed ? render(view, parsed.doc, parsed.style) : null;
}

function renameInPipeline(view, kind, from, to) {
  if (kind !== "role") {
    return false;
  }
  let changed = false;
  for (const step of view.steps) {
    if (step.kind === "agent" && step.fields?.role === from) {
      step.fields.role = to;
      changed = true;
    }
  }
  return changed;
}

function renameInDocument(view, kind, from, to) {
  const value = view.value ?? {};
  const field = kind === "role" ? "role" : kind === "pipeline" ? "pipeline" : null;
  if (field && value[field] === from) {
    value[field] = to;
    return true;
  }
  if (kind === "repo" && Array.isArray(value.repos) && value.repos.includes(from)) {
    value.repos = value.repos.map((repo) => (repo === from ? to : repo));
    return true;
  }
  return false;
}

/** The declared name inside a file, rewritten so a rename stays consistent. */
export function withDeclaredName(text, path, to) {
  const parsed = parse(text, { path });
  const view = structuredClone(parsed.view);
  if (view.kind === "pipeline") {
    view.name = to;
  } else {
    view.value.name = to;
  }
  return render(view, parsed.doc, parsed.style);
}

function createdStyleFor(lineEnding) {
  return { lineEnding, finalNewline: true, indentSeq: false, flowCollectionPadding: false, lineWidth: 0 };
}

function descriptor(kind) {
  const found = DESCRIPTORS[kind];
  if (!found) {
    throw new Error(`unknown config kind \`${kind}\``);
  }
  return found;
}
