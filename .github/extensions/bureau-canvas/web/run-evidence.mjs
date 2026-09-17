// Presentation checks, not a replacement for Bureau's authoritative replay.
const KINDS = new Set([
  "run_started", "step_started", "output", "step_finished", "group_started",
  "group_member_started", "group_member_finished", "group_member_cancelled",
  "group_finished", "copilot_factory", "checkpoint", "branch_pushed", "pr_created", "run_finished",
]);
export const MAX_RUN_LOG_EVENTS = 10_000;
const U64_MAX = Number(0xffffffffffffffffn);

export function runListingResult(payload) {
  if (!Array.isArray(payload?.runs)) throw new Error("Run listing has no run array.");
  const complete = !payload.observation || payload.observation.state === "ready";
  return { ...payload, status: complete ? "ready" : "error" };
}

export function inspectEvents(events) {
  if (!Array.isArray(events)) return "Run history has no event array.";
  if (events.length > MAX_RUN_LOG_EVENTS) return `Run preview exceeds ${MAX_RUN_LOG_EVENTS} events; use Bureau CLI inspection.`;
  for (const [index, event] of events.entries()) {
    if (!event || !Number.isInteger(event.seq) || event.seq < 0 || event.seq > U64_MAX
      || !Number.isSafeInteger(event.at_ms)
      || event.at_ms < 0 || event.at_ms > 8_640_000_000_000_000
      || !event.data || typeof event.data !== "object" || Array.isArray(event.data)) {
      return `Invalid run record at position ${index}; inspect the saved log.`;
    }
    if (!KINDS.has(event.kind)) return `Unsupported run event ${String(event.kind)} at position ${index}.`;
    const data = event.data;
    if ((["step_started", "step_finished"].includes(event.kind) && typeof data.step !== "string")
      || (event.kind === "output" && (typeof data.stream !== "string" || typeof data.data !== "string"))
      || (event.kind === "group_started" && (!Array.isArray(data.members) || data.members.some((member) => typeof member !== "string")))
      || (event.kind === "run_finished" && typeof data.outcome !== "string")) {
      return `Invalid ${event.kind} payload at position ${index}; inspect the saved log.`;
    }
  }
  return null;
}

export function inspectEventLog(text) {
  const events = [];
  const lines = text.split("\n");
  let warning = null;
  for (const [index, line] of lines.entries()) {
    if (!line.trim()) continue;
    if (events.length >= MAX_RUN_LOG_EVENTS) return { events: [], error: `Run preview exceeds ${MAX_RUN_LOG_EVENTS} events; use Bureau CLI inspection.`, warning: null, limited: true };
    try {
      const event = JSON.parse(line);
      if (index === lines.length - 1 && !text.endsWith("\n") && inspectEvents([event])) {
        warning = "Incomplete final append; only the readable event prefix is shown.";
      } else {
        events.push(event);
      }
    } catch {
      if (index === lines.length - 1 && !text.endsWith("\n")) {
        warning = "Incomplete final append; only the readable event prefix is shown.";
      } else {
        return { events, error: `Invalid JSON on run-log line ${index + 1}; later records are not trusted.`, warning };
      }
    }
  }
  return { events, error: inspectEvents(events), warning };
}

export function runIdentityProblem(runId, events) {
  const started = events.filter((event) => event.kind === "run_started");
  if (started.length !== 1 || events[0]?.kind !== "run_started") {
    return "A single initial run_started record is missing; run state is unknown.";
  }
  if (started[0].data.run_id !== runId) return "Recorded run identity does not match its directory.";
  if (typeof started[0].data.assignment !== "string") return "Recorded assignment identity is missing or invalid.";
  const snapshot = started[0].data.snapshot;
  if (snapshot?.run_id && snapshot.run_id !== runId) return "Snapshot run identity does not match its directory.";
  if ([snapshot?.pipeline?.name, snapshot?.assignment?.pipeline, started[0].data.pipeline]
    .some((name) => name != null && typeof name !== "string")) return "Recorded pipeline identity is invalid.";
  const finished = events.filter((event) => event.kind === "run_finished");
  if (finished.length > 1) return "Multiple run_finished records; inspect the saved log.";
  return null;
}
