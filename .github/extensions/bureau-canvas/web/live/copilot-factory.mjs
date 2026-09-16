// Presentation only: durable authority and recovery validation remain in Bureau.
import { factoryControlsMatch } from "./factory-controls.mjs";

const STATUSES = new Set(["pending", "running", "paused", "halted", "completed", "cancelled", "error"]);
const SETTLED = new Set(["paused", "halted", "completed", "cancelled", "error"]);

export function emptyFactories() {
  return { records: {}, error: null };
}

export function factoryEvents(events) {
  return (events ?? []).reduce(projectFactory, emptyFactories());
}

function fail(state, message) {
  return { ...state, error: message };
}

function prepared(state, intent, eventSeq) {
  if (!intent?.session_id || !intent.step || !intent.factory?.name || !intent.workspace?.directory) {
    return fail(state, "Malformed local factory intent; preserved runtime state needs inspection.");
  }
  const sessionId = intent.session_id;
  if (Object.hasOwn(state.records, sessionId)) {
    return fail(state, `Duplicate local factory intent for ${sessionId}.`);
  }
  return { ...state, records: { ...state.records, [sessionId]: {
    sessionId, step: intent.step, name: intent.factory.name,
    provider: intent.factory.extension, profile: intent.factory.runtime?.profile,
    modelCredential: intent.factory.model_credential,
    version: intent.factory.runtime?.version, workspace: intent.workspace.directory,
    runId: null, attempt: null, nativeStatus: null, canResume: false,
    executionClean: false, executionActive: false, runtimeActive: false, eventSeq, dispatch: null, accounting: null,
    accountingIncomplete: false, problem: null,
  } } };
}

function observed(record, data) {
  const { run, summary } = data;
  if (!record.runId || run?.runId !== record.runId || summary?.runId !== record.runId
    || summary.factoryName !== record.name || run.status !== summary.status || !STATUSES.has(run.status)) {
    return { ...record, problem: "Native factory observation has mismatched identity or unknown status." };
  }
  const consumed = summary.consumed;
  if (!consumed || !["activeMs", "subagents", "nanoAiu"].every((key) =>
    Number.isSafeInteger(consumed[key]) && consumed[key] >= 0)) {
    return { ...record, problem: "Native accounting is missing or cannot be represented exactly in this view." };
  }
  const accounting = Object.fromEntries(Object.entries(consumed)
    .filter(([key]) => ["activeMs", "subagents", "nanoAiu"].includes(key))
    .map(([key, value]) => [key, Math.max(record.accounting?.[key] ?? 0, value)]));
  const reasons = [run.reason, run.failure?.code, run.failure?.reason, summary.terminal?.reason,
    summary.terminal?.failure?.code];
  const interrupted = reasons.includes("interrupted");
  return { ...record, nativeStatus: run.status, accounting,
    canResume: summary.canResume === true && !interrupted,
    accountingIncomplete: record.accountingIncomplete || reasons.some((reason) => /accounting/u.test(reason ?? "")),
    problem: interrupted ? "Runtime was interrupted; hard-crashed factories cannot resume." : record.problem,
  };
}

function update(record, data) {
  switch (data.event) {
    case "session_accepted": return record;
    case "notification": return record; // Never adopt identity or terminal status from a notification.
    case "runtime_opened":
      return { ...record, runtimeActive: true,
        ...(data.purpose === "execution" ? { executionClean: false, executionActive: true } : {}) };
    case "dispatch":
      return { ...record, dispatch: data.operation };
    case "accepted":
      if (!data.run_id || !Number.isSafeInteger(data.attempt) || data.attempt < 1
        || (record.runId && record.runId !== data.run_id)) {
        return { ...record, problem: "Correlated factory acceptance has invalid or conflicting identity." };
      }
      return { ...record, runId: data.run_id, attempt: data.attempt, dispatch: null, nativeStatus: null, canResume: false };
    case "observed": return observed(record, data);
    case "rejected":
      return { ...record, dispatch: null, problem: data.message || "Factory admission was rejected." };
    case "indeterminate":
      return { ...record, problem: data.message || "Factory start is indeterminate; never retry by name or latest run." };
    case "runtime_closed":
      if (data.purpose !== "execution") return { ...record, runtimeActive: false };
      return { ...record, runtimeActive: false, executionClean: data.clean === true, executionActive: false,
        problem: data.clean ? record.problem : data.message || "Original factory execution did not shut down cleanly." };
    default:
      return { ...record, problem: `Unrecognized local factory event: ${data.event ?? "(missing)"}.` };
  }
}

export function projectFactory(state, event) {
  if (event?.kind !== "copilot_factory" || state.error) return state;
  const data = event.data;
  const eventSeq = Number.isSafeInteger(event.seq) && event.seq >= 0 ? event.seq : null;
  if (data?.event === "prepared") return prepared(state, data.intent, eventSeq);
  const sessionId = data?.session_id;
  if (!sessionId || !Object.hasOwn(state.records, sessionId)) {
    return fail(state, "Local factory event has no matching durable intent; preserve its workspace.");
  }
  return { ...state, records: { ...state.records, [sessionId]: { ...update(state.records[sessionId], data), eventSeq } } };
}

export function factoryForStep(state, step) {
  return Object.values(state?.records ?? {}).filter((record) => record.step === step).at(-1) ?? null;
}

export function factoryRunState(state, current, fallback) {
  if (fallback === "finished") return fallback;
  const record = factoryForStep(state, current);
  if (state?.error || record?.problem) return "indeterminate";
  if (!record) return fallback;
  if (record.executionActive) return "running";
  if (record.executionClean && ["paused", "halted", "error"].includes(record.nativeStatus)) return "paused";
  if (record.dispatch === "start" && !record.runId && record.executionClean) return "indeterminate";
  return fallback;
}

export function factoryResumeBlocked(record, control) {
  if (!record) return false;
  return Boolean(record.problem || !factoryControlsMatch(record, control) || control.allowed !== true);
}

export function factoryDetails(record) {
  if (!record) return [];
  const usage = record.accounting;
  const status = record.nativeStatus ?? (record.runId
    ? "awaiting native observation" : record.dispatch === "start" ? "start acceptance pending" : "not admitted");
  const execution = record.executionClean ? "verified clean exit" : "clean exit not yet verified";
  const recovery = record.problem ?? (!record.runId
    ? "Not admitted. Bureau must verify whether this bootstrap can continue."
    : record.canResume
    ? `Native resume eligible; ${execution}. Bureau still verifies saved authority and runtime storage.`
    : `Native resume unavailable; ${execution}.`);
  return [
    ["Local factory", record.name], ["Provider", record.provider],
    ["Model credential reference", record.modelCredential ?? "not recorded"],
    ["SDK session", record.sessionId], ["Native run", record.runId ?? "no accepted run ID"],
    ["Native attempt", record.attempt ?? "not accepted"], ["Native status", status],
    ["Runtime", `${record.version ?? "unknown version"} / ${record.profile ?? "unknown profile"}`],
    ["Measured credits", usage ? `${usage.nanoAiu / 1e9}${record.accountingIncomplete ? " (incomplete floor)" : ""}` : "unavailable"],
    ["Native usage", usage ? `${usage.subagents} subagents; ${usage.activeMs} ms active` : "unavailable"],
    ["Workspace", record.workspace], ["Recovery", recovery],
  ];
}

export function factoryPauseAvailable(record) {
  return !record?.problem && (!record || !SETTLED.has(record.nativeStatus));
}
