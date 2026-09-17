export const navigationSchema = {
  type: "object",
  additionalProperties: false,
  required: ["view"],
  properties: {
    view: { type: "string", enum: ["operations", "config", "pipeline"] },
    assignment: { type: "string" },
    pipeline: { type: "string" },
    mode: { type: "string", enum: ["design", "live", "replay"] },
    run_id: { type: "string", pattern: "^[A-Za-z0-9][A-Za-z0-9._-]*$" },
  },
};

export function navigationTarget(input, state, runs = []) {
  if (!input || !["operations", "config", "pipeline"].includes(input.view)) throw new Error("Choose Operations, configuration, or a pipeline.");
  if (Object.keys(input).some((key) => !Object.hasOwn(navigationSchema.properties, key))) throw new Error("Unknown navigation field.");
  if (Object.keys(input).some((key) => typeof input[key] !== "string" || !input[key])) throw new Error("Navigation fields must be nonempty strings.");
  if (input.run_id && !/^[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(input.run_id)) throw new Error("Invalid run identifier.");
  if (input.view !== "pipeline") {
    if (input.pipeline || input.mode || input.run_id) throw new Error("Run navigation requires a pipeline.");
    if (input.assignment && (input.view !== "config"
      || !state.config?.view?.assignments.some((item) => item.name === input.assignment))) {
      throw new Error("Assignment is not in this authoring configuration.");
    }
    return { view: input.view, ...(input.assignment ? { assignment: input.assignment } : {}) };
  }
  if (typeof input.pipeline !== "string" || !input.pipeline || input.assignment) throw new Error("Choose a pipeline by name.");
  const mode = input.mode ?? "design";
  if (!["design", "live", "replay"].includes(mode)) throw new Error("Unknown pipeline mode.");
  if (input.run_id) {
    if (mode === "design") throw new Error("A run must open in Live or Replay.");
    const run = runs.find((item) => item.run_id === input.run_id);
    const owner = state.config?.view?.assignments.find((item) => item.name === run?.assignment);
    if (!run || (run.pipeline ?? owner?.pipeline) !== input.pipeline) throw new Error("Run is not attributed to this pipeline.");
    if (!state.pipelines?.[input.pipeline]) throw new Error("Recorded pipeline is absent from this authoring configuration; inspect the run log instead.");
    if (run.evidence?.state === "invalid") throw new Error(run.evidence.message);
  }
  return { view: "pipeline", pipeline: input.pipeline, mode, ...(input.run_id ? { run_id: input.run_id } : {}) };
}

export function navigationKey(target) {
  return JSON.stringify([target.view, target.assignment ?? null, target.pipeline ?? null,
    target.mode ?? (target.view === "pipeline" ? "design" : null), target.run_id ?? null]);
}

export function intentNavigation(intent) {
  if (intent.kind === "navigate") return intent.input;
  if (intent.kind === "open-pipeline") return { view: "pipeline", pipeline: intent.pipeline };
  if (intent.kind === "back-to-config") return { view: "config" };
  return null;
}
