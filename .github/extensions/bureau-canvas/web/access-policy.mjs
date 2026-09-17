export const READ_ONLY_REASON = "This host is read-only: configuration writes and run controls are disabled. Managed dispatch belongs to the daemon. Edit a separate authoring worktree and review a config PR.";

const READ_ACTIONS = new Set(["describe", "focus", "reload", "operations", "navigate"]);
const READ_INTENTS = new Set(["operations", "navigate", "open-pipeline", "back-to-config", "derive-work-source", "resolve-repo"]);

export function isReadOnly(access) {
  return access?.mode === "read-only";
}

export function readOnlyControl(access) {
  return isReadOnly(access) ? { disabled: true, title: access.reason, "aria-describedby": "read-only-notice" } : {};
}

export function canvasAccess(input = {}, options = {}, setting) {
  const invalid = setting !== undefined && setting !== "0" && setting !== "1";
  const readOnly = invalid || setting === "1" || input.readOnly === true || options.readOnly === true;
  return Object.freeze({
    mode: readOnly ? "read-only" : "local",
    reason: readOnly ? `${invalid ? "Invalid BUREAU_CANVAS_READ_ONLY; expected 0 or 1. " : ""}${READ_ONLY_REASON}` : null,
  });
}

export function mutationRefusal(access, name, surface = "intent") {
  const allowed = surface === "action" ? READ_ACTIONS : READ_INTENTS;
  return isReadOnly(access) && !allowed.has(name) ? access.reason : null;
}
