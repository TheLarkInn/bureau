/** Human copy for the three pipeline terminals. Wire names stay unchanged. */
export const TERMINAL_COPY = {
  done: { label: "Publish", detail: "Push the branch and open a pull request" },
  abort: { label: "Failed", detail: "Add the assignment's abort label and stop" },
  escalate: { label: "Needs human", detail: "Comment on the work item, add the escalation label, and stop" },
};

export function terminalCopy(name) {
  return TERMINAL_COPY[name] ?? { label: name, detail: "" };
}

export function terminalOption(name) {
  const copy = terminalCopy(name);
  return `${copy.label} — ${copy.detail}`;
}
