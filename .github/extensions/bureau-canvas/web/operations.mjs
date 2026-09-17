// Import-free projection shared by the app action and both browser hosts.
export const STALE_EVIDENCE_MS = 5 * 60 * 1000;

const LIMITS = [
  ["maxConcurrent", "concurrent runs"], ["maxRunsPerHour", "runs/hour"],
  ["maxRunsPerDay", "runs/day"], ["maxOpenPrs", "open PRs"],
  ["maxCostPerDayUsd", "USD/day"],
];

export function safeguards(assignment) {
  const limits = assignment.limits ?? {};
  return [
    ...LIMITS.map(([key, label]) => `${label}: ${limits[key] ?? "unlimited"}`),
    `run deadline: ${limits.maxRunHours == null ? "24h (default)" : `${limits.maxRunHours}h`}`,
    `approval label: ${assignment.work?.approvalLabel ?? "not configured"}`,
    `abort label: ${assignment.work?.abortLabel ?? "not configured"}`,
    `escalate label: ${assignment.work?.escalateLabel ?? "not configured"}`,
  ];
}

function validationOf(state) {
  const validation = state.validation;
  const pending = (state.plan?.writes.length ?? 0) + (state.plan?.removals.length ?? 0);
  const sample = validation?.state === "fixture";
  const verdict = sample ? "Sample configuration" : validation?.state !== "validated"
    ? "Validation unavailable" : validation.ok ? "Working tree validated" : "Invalid configuration";
  return {
    verdict, pending, sample,
    tone: sample || validation?.state !== "validated" ? "notice" : validation.ok ? "success" : "danger",
    message: validation?.message ?? null,
    errors: validation?.errors ?? [],
    checked_at_ms: validation?.checked_at_ms ?? null,
    dir: state.dir, git: state.authoring ?? { state: "unavailable", commit: null, changes: null },
  };
}

function terminalState(evidence) {
  if (evidence.terminal === "escalate" || evidence.outcome === "blocked") return ["attention", "Needs attention"];
  if (evidence.terminal === "abort" || evidence.outcome === "failure") return ["failed", "Failed"];
  if (["success", "no-work"].includes(evidence.outcome)) return ["completed", evidence.outcome === "no-work" ? "No work" : "Completed"];
  return ["attention", "Unknown outcome"];
}

function stateOf(evidence, stale) {
  if (!evidence || ["invalid", "limited"].includes(evidence.state)) return ["attention", "Evidence unavailable"];
  if (evidence.status === "finished") return terminalState(evidence);
  if (evidence.status === "paused") return ["paused", "Paused"];
  if (evidence.status === "indeterminate") return ["attention", "Indeterminate"];
  if (stale) return ["attention", "Unfinished, stale evidence"];
  return evidence.status === "running" ? ["active", "Active (observed)"] : ["attention", "Run state unknown"];
}

export function operationRun(run, state, now) {
  const evidence = run.evidence;
  const time = evidence?.last_at_ms;
  const stale = evidence?.status !== "finished" && (typeof time !== "number" || now - time > STALE_EVIDENCE_MS || time > now);
  const [status, label] = stateOf(evidence, stale);
  const factoryIssue = run.copilot_factories?.error || Object.values(run.copilot_factories?.records ?? {})
    .some((record) => record.problem || record.accountingIncomplete || !record.accounting);
  const owner = (state.config?.view?.assignments ?? []).find((assignment) => assignment.name === run.assignment);
  const pipeline = run.pipeline ?? owner?.pipeline ?? null;
  const target = pipeline && state.pipelines?.[pipeline] && evidence?.state !== "invalid"
    ? { view: "pipeline", pipeline, mode: evidence?.status === "finished" ? "replay" : "live", run_id: run.run_id } : null;
  return {
    ...run, status, label, stale, pipeline, target,
    needs_attention: ["attention", "paused", "failed"].includes(status) || evidence?.state === "partial" || stale || Boolean(factoryIssue),
    attributed_by: run.pipeline ? "recorded pipeline" : owner ? "current assignment (pipeline not recorded)" : "unattributed",
    cost: typeof evidence?.cost_usd === "number" && Number.isFinite(evidence.cost_usd) && evidence.cost_usd >= 0
      ? `${evidence.cost_usd.toFixed(2)} USD` : "Unknown",
  };
}

export function operationsOverview(state, listing, now = Date.now()) {
  const runs = (listing?.runs ?? []).map((run) => operationRun(run, state, now))
    .sort((a, b) => Number(b.needs_attention) - Number(a.needs_attention)
      || (b.evidence?.last_at_ms ?? 0) - (a.evidence?.last_at_ms ?? 0) || a.run_id.localeCompare(b.run_id));
  const observation = listing?.observation ?? { state: listing?.status === "error" ? "error" : "unknown", message: "Run evidence has not been read." };
  const sources = new Map();
  for (const run of runs) {
    const source = run.evidence?.config_source;
    if ([source?.remote, source?.reference, source?.commit].every((field) => typeof field === "string" && field.length)) {
      sources.set(JSON.stringify(source), source);
    }
  }
  return {
    access: state.access ?? { mode: "local", reason: null },
    configuration: validationOf(state), observation, runs,
    counts: observation.state === "ready" ? {
      attention: runs.filter((run) => run.needs_attention).length,
      active: runs.filter((run) => run.status === "active").length,
      paused: runs.filter((run) => run.status === "paused").length,
      failed: runs.filter((run) => run.status === "failed").length,
    } : null,
    recorded_sources: [...sources.values()],
    assignments: (state.config?.view?.assignments ?? []).map((assignment) => ({
      ...assignment, safeguards: safeguards(assignment),
      runs: runs.filter((run) => run.assignment === assignment.name),
      pipeline_available: Boolean(state.pipelines?.[assignment.pipeline]),
    })),
  };
}

export function filterOperationRuns(runs, filter, query) {
  const needle = query.trim().toLowerCase();
  return runs.filter((run) => (filter === "all" || (filter === "attention" ? run.needs_attention : run.status === filter))
    && [run.run_id, run.assignment, run.pipeline, run.current_step, run.label].some((value) => String(value ?? "").toLowerCase().includes(needle)));
}
