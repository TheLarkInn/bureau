/** Human copy for the three pipeline terminals. Wire names stay unchanged. */
export const TERMINAL_COPY = {
  done: { label: "Publish", detail: "Push the branch and open a pull request" },
  abort: { label: "Failed", detail: "Stop without publishing" },
  escalate: { label: "Needs human", detail: "Comment on the work item and stop" },
};

export function terminalCopy(name) {
  return TERMINAL_COPY[name] ?? { label: name, detail: "" };
}

export function terminalOption(name) {
  const copy = terminalCopy(name);
  return `${copy.label} — ${copy.detail}`;
}
