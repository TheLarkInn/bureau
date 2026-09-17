export const REVIEW = { review: "Offline fixture review of the supplied work item.", needs_human: false };
export const VERIFICATION = { verified: true, reason: "Offline fixture verification of the assessment." };

export function context(responses = [REVIEW, VERIFICATION], args = null) {
  const pending = [...responses];
  const controller = new AbortController();
  const calls = [];
  const phases = [];
  const steps = [];
  const logs = [];
  return {
    args,
    signal: controller.signal,
    controller,
    calls,
    phases,
    steps,
    logs,
    phase: (title) => phases.push(title),
    log: (message) => logs.push(message),
    step: async (key, producer) => {
      steps.push(key);
      return producer();
    },
    agent: async (prompt, options) => {
      calls.push({ prompt, options });
      const response = pending.shift();
      if (response instanceof Error) throw response;
      return response;
    },
  };
}
