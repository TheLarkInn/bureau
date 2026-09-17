const samples = {
  pass: {
    title: "Ready for human review",
    detail: "Verified work is a proposal, not an automatic merge.",
    verify: "Required checks pass.",
    review: "Evidence accompanies the proposal.",
    publish: "The forge owns review and merge.",
  },
  repair: {
    title: "Verification failed. Publication waits.",
    detail: "The declared failure edge returns to repair within the configured retry limit.",
    verify: "A required check fails. Follow the failure edge.",
    review: "Waiting for verified work.",
    publish: "No pull request is opened in this sample.",
  },
  budget: {
    title: "No headroom. No new run.",
    detail: "The configured assignment limit is reached. Admission stops before execution.",
    verify: "Not started.",
    review: "Not started.",
    publish: "No pull request is opened in this sample.",
  },
};

function select(buttons, selected) {
  for (const button of buttons) button.setAttribute("aria-pressed", String(button === selected));
}

const sample = document.querySelector(".sample");
const outcomes = document.querySelectorAll("[data-outcome]");
for (const button of outcomes) {
  button.addEventListener("click", () => {
    const state = samples[button.dataset.outcome];
    select(outcomes, button);
    sample.dataset.state = button.dataset.outcome;
    sample.querySelector("[data-result-title]").textContent = state.title;
    sample.querySelector("[data-result-detail]").textContent = state.detail;
    for (const name of ["verify", "review", "publish"]) {
      sample.querySelector(`[data-${name}]`).textContent = state[name];
    }
    sample.querySelector('[data-step="observe"] small').textContent =
      button.dataset.outcome === "budget" ? "Admission blocked. No lease claimed." : "Eligible item. Owned lease.";
    sample.querySelector('[data-step="implement"] small').textContent =
      button.dataset.outcome === "budget" ? "Not started." : "Isolated Git worktree.";
  });
}

const filters = document.querySelectorAll("[data-filter]");
const scenarios = document.querySelectorAll(".scenario");
for (const button of filters) {
  button.addEventListener("click", () => {
    select(filters, button);
    let visible = 0;
    for (const scenario of scenarios) {
      scenario.hidden = button.dataset.filter !== "all" && scenario.dataset.group !== button.dataset.filter;
      if (!scenario.hidden) visible += 1;
    }
    document.querySelector(".scenario-count").textContent = `${visible} scenarios`;
  });
}

const architectures = document.querySelectorAll("[data-arch]");
for (const button of architectures) {
  button.addEventListener("click", () => {
    select(architectures, button);
    document.querySelector("[data-asset]").textContent =
      `bureau-v<version>-${button.dataset.arch}-unknown-linux-musl.tar.gz`;
  });
}

for (const element of document.querySelectorAll("[data-enhance]")) element.hidden = false;
