const TERMINALS = ["done", "abort", "escalate"];
const OUTCOMES = ["success", "failure", "blocked", "no-work"];
const CONTROL_FIELDS = [
  ["next", "success"],
  ["on_failure", "failure"],
  ["on_blocked", "blocked"],
  ["on_no_work", "no-work"],
];

export function configView(payload) {
  const config = configOf(payload);
  const view = emptyConfigView(payload);
  if (!config) {
    return view;
  }
  const used = usedBy(config);
  view.repos = repoItems(config, used.repos);
  view.roles = roleItems(config, used.roles);
  view.assignments = assignmentItems(config);
  view.pipelines = pipelineItems(config, used.pipelines);
  view.orphans = orphanItems(view);
  return view;
}

/**
 * The config relation graph (Q16): assignment → pipeline/repos, then each
 * pipeline → the roles its agent steps actually use. Assignment `role` is a
 * scaffold-era field the engine never reads, so showing that edge would make
 * a non-operational declaration look like runtime behavior.
 */
export function relationView(payload) {
  const config = configOf(payload);
  if (!config) {
    return { nodes: [], edges: [] };
  }
  const nodes = relationNodes(config);
  const edges = relationEdges(config, new Set(nodes.map((node) => node.id)));
  return { nodes, edges };
}

function relationNodes(config) {
  return [
    ...entries(config.assignments).map(([name]) => ({ id: `assignment:${name}`, kind: "assignment", name })),
    ...entries(config.pipelines).map(([name]) => ({ id: `pipeline:${name}`, kind: "pipeline", name })),
    ...entries(config.roles).map(([name]) => ({ id: `role:${name}`, kind: "role", name })),
    ...entries(config.repos).map(([name]) => ({ id: `repo:${name}`, kind: "repo", name })),
  ];
}

function relationEdges(config, ids) {
  return [
    ...entries(config.assignments).flatMap(([name, assignment]) => assignmentRelations(name, assignment)),
    ...entries(config.pipelines).flatMap(([name, pipeline]) => pipelineRelations(name, pipeline)),
  ].filter((edge) => ids.has(edge.source) && ids.has(edge.target));
}

function assignmentRelations(name, assignment) {
  return [
    relationEdge("pipeline", `assignment:${name}`, `pipeline:${assignment.pipeline}`),
    ...(assignment.repos ?? []).map((repo) => relationEdge("repo", `assignment:${name}`, `repo:${repo}`)),
  ];
}

/** Reads `step.role` exactly as `addPipelineUses` does — see the note there. */
function pipelineRelations(name, pipeline) {
  const roles = [];
  for (const step of pipeline.steps ?? []) {
    if (step.role && !roles.includes(step.role)) {
      roles.push(step.role);
    }
  }
  return roles.map((role) => relationEdge("role", `pipeline:${name}`, `role:${role}`));
}

function relationEdge(relation, source, target) {
  return { id: `${relation}:${source}->${target}`, source, target, relation };
}

export function pipelineView(payload, name) {
  const pipeline = configOf(payload)?.pipelines?.[name];
  if (!pipeline) {
    return emptyPipelineView(name);
  }
  const parentByMember = memberParents(pipeline);
  const edges = edgeItems(pipeline, parentByMember);
  const usedTerminals = new Set(edges.filter((edge) => edge.target.startsWith("terminal:")).map((edge) => edge.target));
  return {
    name: pipeline.name ?? name,
    steps: stepItems(pipeline, parentByMember),
    terminals: TERMINALS.map(terminalItem).filter((terminal) => usedTerminals.has(terminal.id)),
    edges,
  };
}

function configOf(payload) {
  const config = payload?.config;
  return config && typeof config === "object" ? config : null;
}

function emptyConfigView(payload) {
  return {
    dir: payload?.dir ?? payload?.config_dir ?? "",
    repos: [],
    roles: [],
    assignments: [],
    pipelines: [],
    orphans: [],
  };
}

function emptyPipelineView(name) {
  return { name, steps: [], terminals: [], edges: [] };
}

function entries(items) {
  return Object.entries(items ?? {}).sort(([left], [right]) => left.localeCompare(right));
}

function usedBy(config) {
  const used = {
    repos: seededUses(config.repos),
    roles: seededUses(config.roles),
    pipelines: seededUses(config.pipelines),
  };
  for (const [name, assignment] of entries(config.assignments)) {
    addAssignmentUses(used, name, assignment);
  }
  for (const [name, pipeline] of entries(config.pipelines)) {
    addPipelineUses(used.roles, name, pipeline);
  }
  return used;
}

function seededUses(items) {
  return new Map(entries(items).map(([name]) => [name, []]));
}

function addUse(uses, name, ref) {
  if (!name || !uses.has(name)) {
    return;
  }
  const refs = uses.get(name);
  if (!refs.includes(ref)) {
    refs.push(ref);
  }
}

function addAssignmentUses(used, name, assignment) {
  const ref = `assignment:${name}`;
  for (const repo of assignment.repos ?? []) {
    addUse(used.repos, repo, ref);
  }
  addUse(used.roles, assignment.role, ref);
  addUse(used.pipelines, assignment.pipeline, ref);
}

/**
 * A role a step names is a role in use, whatever kind of step names it.
 *
 * This filtered on `type === "agent"` and `lib/preflight.mjs` did not, so the
 * two disagreed about the same config: a `role` on a deterministic step — which
 * `bureau validate` rejects, and which the canvas still has to draw, because
 * showing an invalid config is what the findings are for — left the role listed
 * as Unreferenced while deleting it was blocked by the very step the strip said
 * did not exist. Whether the reference is legal is `validate`'s judgement to
 * make; whether one exists is not.
 *
 * `pipelineRelations` is the third projection of the same question, and it has
 * to answer it the same way or the contradiction only moves: a filtered graph
 * would draw that role as an edgeless card — the graph's own way of saying
 * "unreferenced" — beside a strip that had just stopped saying it.
 */
function addPipelineUses(roles, name, pipeline) {
  for (const step of pipeline.steps ?? []) {
    addUse(roles, step.role, `pipeline:${name}/${step.name}`);
  }
}

function repoItems(config, uses) {
  return entries(config.repos).map(([name, repo]) => ({
    name,
    url: repo.url,
    forge: repo.forge,
    access: repo.access,
    credential: repo.credential,
    usedBy: sortedUses(uses, name),
  }));
}

function roleItems(config, uses) {
  return entries(config.roles).map(([name, role]) => ({
    name,
    agent: role.agent,
    adapter: role.adapter,
    permissions: role.permissions ?? [],
    minTrust: role.min_trust,
    usedBy: sortedUses(uses, name),
  }));
}

function assignmentItems(config) {
  return entries(config.assignments).map(([name, assignment]) => ({
    name,
    work: workItem(assignment.work),
    repos: assignment.repos ?? [],
    primaryRepo: assignment.repos?.[0] ?? null,
    pipeline: assignment.pipeline,
    branchPrefix: assignment.branch_prefix,
    limits: limitsItem(assignment.limits),
  }));
}

function workItem(work = {}) {
  const item = { forge: work.forge, source: work.source, filter: work.filter };
  if (work.approval_label != null) {
    item.approvalLabel = work.approval_label;
  }
  item.abortLabel = work.abort_label ?? null;
  item.escalateLabel = work.escalate_label ?? null;
  return item;
}

function limitsItem(limits = {}) {
  return {
    maxConcurrent: limits.max_concurrent ?? null,
    maxRunsPerHour: limits.max_runs_per_hour ?? null,
    maxRunsPerDay: limits.max_runs_per_day ?? null,
    maxOpenPrs: limits.max_open_prs ?? null,
    maxCostPerDayUsd: limits.max_cost_per_day_usd ?? null,
    maxRunHours: limits.max_run_hours ?? null,
  };
}

function pipelineItems(config, uses) {
  return entries(config.pipelines).map(([name, pipeline]) => ({
    name,
    stepCount: pipeline.steps?.length ?? 0,
    kinds: unique((pipeline.steps ?? []).map((step) => step.type)),
    roles: unique((pipeline.steps ?? []).map((step) => step.role).filter(Boolean)),
    terminals: usedTerminals(pipeline),
    usedBy: sortedUses(uses, name),
  }));
}

function usedTerminals(pipeline) {
  const found = new Set();
  for (const step of pipeline.steps ?? []) {
    for (const target of controlTargets(step)) {
      if (TERMINALS.includes(target)) {
        found.add(target);
      }
    }
  }
  return TERMINALS.filter((terminal) => found.has(terminal));
}

function sortedUses(uses, name) {
  return [...(uses.get(name) ?? [])].sort();
}

function unique(values) {
  const seen = new Set();
  return values.filter((value) => {
    if (seen.has(value)) {
      return false;
    }
    seen.add(value);
    return true;
  });
}

function orphanItems(view) {
  return [
    ...orphansOf("repo", view.repos),
    ...orphansOf("role", view.roles),
    ...orphansOf("pipeline", view.pipelines),
  ];
}

function orphansOf(kind, items) {
  return items
    .filter((item) => item.usedBy.length === 0)
    .map((item) => ({ kind, name: item.name }));
}

function memberParents(pipeline) {
  const parents = new Map();
  for (const step of pipeline.steps ?? []) {
    if (step.type !== "concurrent") {
      continue;
    }
    for (const member of step.steps ?? []) {
      parents.set(member, step.name);
    }
  }
  return parents;
}

function stepItems(pipeline, parentByMember) {
  return (pipeline.steps ?? []).map((step, order) => {
    const item = {
      id: step.name,
      name: step.name,
      type: "step",
      kind: step.type,
      order,
      fields: fieldsItem(step),
    };
    const parentId = parentByMember.get(step.name);
    if (parentId) {
      item.parentId = parentId;
    }
    return item;
  });
}

function fieldsItem(step) {
  const fields = {
    inputsFrom: step.inputs_from ?? [],
    maxAttempts: step.max_attempts ?? 1,
  };
  copyIfPresent(fields, "run", step.run);
  copyIfPresent(fields, "role", step.role);
  copyIfPresent(fields, "fixture", step.fixture);
  copyIfPresent(fields, "trust", step.trust);
  copyIfPresent(fields, "over", step.over);
  copyMembers(fields, step);
  copyIfPresent(fields, "completion", step.completion);
  copyIfPresent(fields, "maxConcurrent", step.max_concurrent);
  copyIfPresent(fields, "timeoutSecs", step.timeout_secs);
  return fields;
}

function copyIfPresent(target, name, value) {
  if (value != null) {
    target[name] = value;
  }
}

/** `members` describes a concurrent group only; an empty list is not one. */
function copyMembers(fields, step) {
  if (Array.isArray(step.steps) && step.steps.length > 0) {
    fields.members = step.steps;
  }
}

function terminalItem(name) {
  return { id: `terminal:${name}`, name, type: "terminal" };
}

function edgeItems(pipeline, parentByMember) {
  return (pipeline.steps ?? []).flatMap((step) => [
    ...controlEdges(step, parentByMember),
    ...dataEdges(step),
    ...observesEdges(step),
  ]);
}

function controlEdges(step, parentByMember) {
  if (parentByMember.has(step.name)) {
    return [];
  }
  return [
    ...namedControlEdges(step),
    ...decisionControlEdges(step),
  ];
}

function namedControlEdges(step) {
  return CONTROL_FIELDS.flatMap(([field, outcome]) => {
    const target = step[field];
    return presentTarget(target) ? [controlEdge(step.name, target, outcome)] : [];
  });
}

function decisionControlEdges(step) {
  if (step.type !== "decision") {
    return [];
  }
  return sortedOutcomeEntries(step.on).map(([outcome, target]) => controlEdge(step.name, target, outcome));
}

function sortedOutcomeEntries(on = {}) {
  const known = OUTCOMES.flatMap((outcome) => (presentTarget(on[outcome]) ? [[outcome, on[outcome]]] : []));
  const extra = Object.entries(on)
    .filter(([outcome, target]) => !OUTCOMES.includes(outcome) && presentTarget(target))
    .sort(([left], [right]) => left.localeCompare(right));
  return [...known, ...extra];
}

function dataEdges(step) {
  return (step.inputs_from ?? []).filter(presentTarget).map((source) => ({
    id: `data:${source}->${step.name}`,
    source,
    target: step.name,
    relation: "data",
  }));
}

function observesEdges(step) {
  if (step.type !== "decision" || !presentTarget(step.over)) {
    return [];
  }
  return [{
    id: `observes:${step.over}->${step.name}`,
    source: step.over,
    target: step.name,
    relation: "observes",
  }];
}

function controlEdge(source, target, outcome) {
  const resolvedTarget = targetId(target);
  return {
    id: `control:${source}:${outcome}->${resolvedTarget}`,
    source,
    target: resolvedTarget,
    relation: "control",
    outcome,
  };
}

function controlTargets(step) {
  return [
    ...CONTROL_FIELDS.map(([field]) => step[field]).filter(presentTarget),
    ...Object.values(step.on ?? {}).filter(presentTarget),
  ];
}

function targetId(target) {
  return TERMINALS.includes(target) ? `terminal:${target}` : target;
}

function presentTarget(target) {
  return typeof target === "string" && target.length > 0;
}