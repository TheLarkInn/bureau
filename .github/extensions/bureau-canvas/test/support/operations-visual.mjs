import { observeRun } from "../../lib/run-observation.mjs";
import { canvasAccess } from "../../web/access-policy.mjs";
import { inspectEventLog } from "../../web/run-evidence.mjs";

export const OPERATIONS_VISUAL_NOW = 1_740_000_200_000;

const SOURCE = {
  remote: "https://example.invalid/reviewed-config.git",
  reference: "reviewed",
  commit: "0123456789abcdef0123456789abcdef01234567",
};
const STEP = { kind: "step_started", data: { step: "implement" } };
const PAUSE = { kind: "output", data: { stream: "run", data: "run paused at a step boundary: operator pause" } };
const FAILED = { kind: "run_finished", data: { outcome: "failure", terminal: "abort", message: "Verification failed." } };

function run(assignment, runId, tail, age, suffix = "") {
  const events = [{
    kind: "run_started",
    data: { run_id: runId, assignment: assignment.name,
      snapshot: { pipeline: { name: assignment.pipeline }, config_source: SOURCE } },
  }, ...tail].map((event, seq) => ({ seq, at_ms: OPERATIONS_VISUAL_NOW - age + seq, ...event }));
  const text = `${events.map((event) => JSON.stringify(event)).join("\n")}\n${suffix}`;
  return observeRun(runId, inspectEventLog(text));
}

function observedListing(state) {
  const assignment = state.config?.view?.assignments?.[0];
  if (!assignment?.pipeline || !state.pipelines?.[assignment.pipeline]) {
    throw new Error("Operations visual evidence requires a configured assignment and pipeline.");
  }
  return {
    runs: [
      run(assignment, "active-run", [STEP], 500),
      run(assignment, "paused-run", [STEP, PAUSE], 900),
      run(assignment, "failed-run", [STEP, FAILED], 800),
      run(assignment, "stale-run", [STEP], 600_000),
      run(assignment, "corrupt-log", [STEP], 1000, "invalid complete JSON line\n"),
    ],
    observation: { state: "ready", at_ms: OPERATIONS_VISUAL_NOW, dir: "(fixture run directory)", message: null },
  };
}

function unavailable(state) {
  state.dir = "(unavailable configuration fixture)";
  state.status = "Validation errors";
  state.access = canvasAccess({ readOnly: true });
  state.validation = { state: "validated", ok: false, checked_at_ms: OPERATIONS_VISUAL_NOW,
    errors: ["assignments/work.yaml: invalid YAML"] };
  state.authoring = { state: "unavailable", commit: null, changes: null, message: "Authoring repository is unavailable." };
  state.config.view = { assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
  state.pipelines = {};
  return { runs: [], observation: { state: "missing", at_ms: OPERATIONS_VISUAL_NOW,
    dir: "(missing fixture run directory)", message: "Run directory is missing; no run evidence has been read." } };
}

// Pin the observation clock, not the verdicts: production readers and reducers
// derive every run status, source, attribution, and unknown cost from these logs.
export function operationsVisualFixture(base, variant = "observed") {
  if (!["observed", "unavailable"].includes(variant)) throw new Error(`Unknown Operations visual fixture: ${variant}`);
  const state = structuredClone(base);
  state.pipeline = null;
  state.selectedPipeline = null;
  state.navigation = { view: "operations" };
  if (state.validation?.checked_at_ms != null) state.validation.checked_at_ms = OPERATIONS_VISUAL_NOW;
  const listing = variant === "observed" ? observedListing(state) : unavailable(state);
  return { state, listing };
}
