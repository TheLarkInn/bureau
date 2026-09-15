// Eligibility is computed by Bureau, not inferred from the browser's replay.
export function parseFactoryControls(payload, runId) {
  if (payload?.run_id !== runId || !Object.hasOwn(payload, "local_factory_resume")) {
    throw new Error("Bureau returned local factory controls for a different or missing run.");
  }
  const control = payload.local_factory_resume;
  if (control === null) return null;
  if (!control || typeof control.session_id !== "string" || !control.session_id
    || !Number.isSafeInteger(control.event_seq) || control.event_seq < 0
    || typeof control.allowed !== "boolean"
    || (control.reason !== null && typeof control.reason !== "string")) {
    throw new Error("Bureau returned malformed local factory resume eligibility.");
  }
  if (control.allowed ? control.reason !== null : !control.reason?.trim()) {
    throw new Error("Bureau returned inconsistent local factory resume eligibility.");
  }
  return control;
}

export function factoryControlsMatch(record, control) {
  return Boolean(record && control && Number.isSafeInteger(record.eventSeq)
    && control.session_id === record.sessionId && control.event_seq === record.eventSeq);
}
