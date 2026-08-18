import { FINDING_SOURCES } from "./findings.mjs";
import { configView, pipelineView } from "./view.mjs";

const TRUST_ORDER = new Map([
  ["untrusted", 0],
  ["derived", 1],
  ["maintainer", 2],
  ["trusted", 3],
]);

const DEFAULT_TRUST = "untrusted";
const AGENT_OUTPUT_TRUST = "derived";
// Permission variants in files.rs whose docs grant write, push, edit, review, or merge authority.
const WRITE_CAPABLE_PERMISSIONS = new Set([
  "repo:write",
  "repo:push",
  "issues:write",
  "pr:write",
  "pr:review",
  "pr:merge",
]);

export function trustFindings(payload) {
  const view = configView(payload);
  const roles = rolesByName(view.roles);
  return view.pipelines.flatMap((pipeline) => pipelineFindings(payload, pipeline.name, roles));
}

function pipelineFindings(payload, name, roles) {
  const view = pipelineView(payload, name);
  const steps = stepsById(view.steps);
  const dataEdges = dataEdgesBySource(view.edges);
  const findings = [];
  for (const step of view.steps) {
    addStepFindings(findings, { name, step, roles, steps, dataEdges });
  }
  return uniqueFindings(findings);
}

function addStepFindings(findings, context) {
  const trust = untrustedOriginOutput(context.step, context.roles);
  if (trust) {
    carryTrust(findings, { ...context, trust, path: [context.step.id] });
  }
}

function carryTrust(findings, context) {
  for (const edge of context.dataEdges.get(context.step.id) ?? []) {
    carryEdge(findings, { ...context, edge });
  }
}

function carryEdge(findings, context) {
  const target = context.steps.get(context.edge.target);
  if (!target || context.path.includes(target.id) || !passesGate(context.trust, target, context.roles)) {
    return;
  }
  const path = [...context.path, target.id];
  addFinding(findings, { ...context, target, path });
  carryTrust(findings, { ...context, step: target, path, trust: outputTrust(target, context.trust) });
}

function addFinding(findings, context) {
  const permissions = writePermissions(context.target, context.roles);
  if (permissions.length === 0) {
    return;
  }
  findings.push(findingFor({ ...context, permissions }));
}

function findingFor({ name, target, roles, trust, path, permissions }) {
  const role = roles.get(target.fields.role);
  const pathText = path.join(" -> ");
  return {
    source: FINDING_SOURCES.advisory,
    marker: "trust-advisory",
    path: `pipelines/${name}.yaml`,
    message: `Trust advisory: untrusted-origin content travels ${pathText}; it arrives at ${trust} trust, satisfying role \`${role.name}\` with write-capable ${permissionText(permissions)}.`,
    target: { kind: "step", pipeline: name, step: target.id },
  };
}

function untrustedOriginOutput(step, roles) {
  return rank(effectiveMinTrust(step, roles)) === rank(DEFAULT_TRUST) ? outputTrust(step, DEFAULT_TRUST) : null;
}

function passesGate(trust, step, roles) {
  return rank(trust) >= rank(effectiveMinTrust(step, roles));
}

function effectiveMinTrust(step, roles) {
  return normalizeTrust(step.fields.trust ?? roles.get(step.fields.role)?.minTrust);
}

function outputTrust(step, trust) {
  return step.kind === "agent" ? AGENT_OUTPUT_TRUST : trust;
}

function writePermissions(step, roles) {
  return (roles.get(step.fields.role)?.permissions ?? []).filter((permission) => WRITE_CAPABLE_PERMISSIONS.has(permission));
}

function rolesByName(roles) {
  return new Map(roles.map((role) => [role.name, role]));
}

function stepsById(steps) {
  return new Map(steps.map((step) => [step.id, step]));
}

function dataEdgesBySource(edges) {
  const dataEdges = new Map();
  for (const edge of edges.filter((candidate) => candidate.relation === "data")) {
    const edgesForStep = dataEdges.get(edge.source) ?? [];
    edgesForStep.push(edge);
    dataEdges.set(edge.source, edgesForStep);
  }
  return dataEdges;
}

function normalizeTrust(trust) {
  return TRUST_ORDER.has(trust) ? trust : DEFAULT_TRUST;
}

function rank(trust) {
  return TRUST_ORDER.get(normalizeTrust(trust));
}

function permissionText(permissions) {
  return permissions.length === 1 ? `permission \`${permissions[0]}\`` : `permissions ${permissions.map((permission) => `\`${permission}\``).join(", ")}`;
}

function uniqueFindings(findings) {
  const seen = new Set();
  return findings.filter((finding) => {
    const key = `${finding.path}\0${finding.target.step}\0${finding.message}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}
