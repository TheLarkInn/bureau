// What a delete or rename breaks, reported before it is applied.
//
// This is not a correctness mechanism: `bureau validate` already rejects a
// config with a dangling reference, and `save` is gated on it. It exists
// because the CLI structurally cannot tell you *beforehand*, one file at a
// time, that removing one step stranded three others.
//
// Advisory, never blocking. The canvas does not get a second rule set.

import { orphanedBy, stepConsequences } from "./steps.mjs";
import { pipelineView } from "./view.mjs";

const SOURCE = "preflight";

/**
 * References to `name` across the whole config, plus anything that would be
 * stranded by removing it.
 */
export function referrers(payload, kind, name, options = {}) {
  const config = payload?.config;
  if (!config) {
    return [];
  }
  if (kind === "step") {
    return stepReferrers(payload, options.pipeline, name);
  }
  return [...assignmentReferrers(config, kind, name), ...pipelineRoleReferrers(payload, kind, name)];
}

function assignmentReferrers(config, kind, name) {
  const found = [];
  for (const [assignment, value] of Object.entries(config.assignments ?? {})) {
    if (kind === "role" && value.role === name) {
      found.push(reference("assignment", assignment, `assignment \`${assignment}\` runs role \`${name}\``));
    }
    if (kind === "pipeline" && value.pipeline === name) {
      found.push(reference("assignment", assignment, `assignment \`${assignment}\` runs pipeline \`${name}\``));
    }
    if (kind === "repo") {
      found.push(...repoReference(assignment, value, name));
    }
  }
  return found;
}

function repoReference(assignment, value, name) {
  const index = (value.repos ?? []).indexOf(name);
  if (index < 0) {
    return [];
  }
  const detail = index === 0
    ? `assignment \`${assignment}\` lands its branch on \`${name}\``
    : `assignment \`${assignment}\` lists \`${name}\``;
  return [reference("assignment", assignment, detail, index === 0 ? "primary-repo" : "referrer")];
}

function pipelineRoleReferrers(payload, kind, name) {
  if (kind !== "role") {
    return [];
  }
  const found = [];
  for (const pipeline of Object.keys(payload.config.pipelines ?? {})) {
    for (const step of pipelineView(payload, pipeline).steps) {
      if (step.fields?.role === name) {
        found.push(reference("step", `${pipeline}/${step.name}`, `step \`${step.name}\` in \`${pipeline}\` runs role \`${name}\``));
      }
    }
  }
  return found;
}

function stepReferrers(payload, pipeline, name) {
  if (!pipeline) {
    return [];
  }
  const view = pipelineView(payload, pipeline);
  const consequences = stepConsequences(view, name).map((item) => reference(
    item.severity === "entry-step" ? "pipeline" : "step",
    item.step,
    item.message,
    item.severity,
  ));
  const stranded = orphanedBy(view, name).map((step) => reference(
    "step",
    step,
    `step \`${step}\` becomes unreachable without \`${name}\``,
    "orphaned",
  ));
  return [...consequences, ...stranded];
}

function reference(kind, name, message, severity = "referrer") {
  return { source: SOURCE, severity, kind, name, message };
}

/** True when a delete would leave the config referencing something absent. */
export function blocksDelete(found) {
  return found.some((item) => item.severity !== "orphaned");
}
