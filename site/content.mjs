export const repository = "https://github.com/TheLarkInn/bureau";
export const source = `${repository}/blob/main`;

export const scenarios = [
  {
    id: "design-review", title: "Design review", group: "plan",
    summary: "Turn a proposed design into reviewable evidence before implementation.",
    readiness: "Customize the example",
  },
  {
    id: "issue-intake", title: "Issue intake", group: "plan",
    summary: "Make incoming reports useful: clarify the problem and preserve the source.",
    readiness: "Customize the example",
  },
  {
    id: "issue-triage", title: "Issue triage", group: "plan",
    summary: "Assess an issue against explicit criteria, with a bounded next action.",
    readiness: "Customize the example",
  },
  {
    id: "customer-feedback", title: "Customer feedback", group: "plan",
    summary: "Work from deliberately imported feedback, not a new customer database.",
    readiness: "Customize the example",
  },
  {
    id: "failing-test-repair", title: "Failing-test repair", group: "build",
    summary: "Reproduce the failure, propose a patch, and let the test decide.",
    readiness: "Customize the example",
  },
  {
    id: "multi-repo-fix", title: "Multi-repo fix", group: "build",
    summary: "Read a pinned public contract from a second repository; change only the primary.",
    readiness: "Customize the example",
  },
  {
    id: "azure-devops", title: "Azure DevOps", group: "build",
    summary: "Pull work through a native query, with explicit approval and repository grants.",
    readiness: "Customize the example",
  },
  {
    id: "local-sdk-factory", title: "Local SDK factory", group: "operate",
    summary: "Opt into a pinned local factory through a qualified runtime and reviewed role.",
    readiness: "Qualification required",
  },
  {
    id: "cloud-automation", title: "Cloud automation", group: "operate",
    summary: "Inspect or explicitly submit an existing automation. Not daemon-dispatched work.",
    readiness: "Experimental / eligibility required",
  },
  {
    id: "recurring-maintenance", title: "Recurring maintenance", group: "operate",
    summary: "Keep a bounded maintenance assignment against a deliberately managed work source.",
    readiness: "Customize the example",
  },
];

export const principles = [
  ["01", "Trust follows the input", "Distinguish reviewed configuration, maintainer input, outside content, and agent output. Each step declares what it accepts."],
  ["02", "A lease before a claim", "Claim eligible work atomically. Renew ownership during execution; stop when that ownership is lost."],
  ["03", "Limits before execution", "Set concurrency, rate, open-PR, cost, and run-time limits. Cost-limited runs fail closed without usable measurement."],
  ["04", "Code verifies the proposal", "Alternate agent and deterministic steps. Explicit outcome edges decide whether to continue, retry, stop, or escalate."],
  ["05", "Git review is authorization", "Review roles, repository grants, and pipelines in a pull request. Reconcile executes committed configuration, not an unmerged draft."],
  ["06", "Evidence outlives the run", "Keep append-only, secret-scrubbed run logs and artifacts. Ordinary pipeline recovery replays the log, not an agent's memory."],
];

export function escapeHtml(value) {
  return String(value).replace(/[&<>"']/gu, (character) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character]);
}

export function renderScenarios() {
  return scenarios.map(({ id, title, group, summary, readiness }, index) => `
        <li class="scenario" data-group="${group}">
          <a href="${source}/docs/scenarios.md#${id}">
            <span class="scenario-number" aria-hidden="true">${String(index + 1).padStart(2, "0")}</span>
            <span class="scenario-copy">
              <span class="scenario-title">${escapeHtml(title)}<span class="link-arrow icon-arrow icon-arrow-up" aria-hidden="true"></span></span>
              <span class="scenario-summary">${escapeHtml(summary)}</span>
              <span class="scenario-readiness">${escapeHtml(readiness)}</span>
            </span>
          </a>
        </li>`).join("");
}

export function renderPrinciples() {
  return principles.map(([number, title, description]) => `
        <article class="principle">
          <span class="eyebrow">${number}</span>
          <h3>${escapeHtml(title)}</h3>
          <p>${escapeHtml(description)}</p>
        </article>`).join("");
}
