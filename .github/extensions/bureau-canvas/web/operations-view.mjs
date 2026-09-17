import React, { useMemo, useRef, useState } from "react";
import { useRunListing } from "./modes.js";
import { filterOperationRuns, operationsOverview } from "./operations.mjs";
import { factoryDetails } from "./live/copilot-factory.mjs";
import { isReadOnly } from "./access-policy.mjs";

const h = React.createElement;
const GUIDE = "https://github.com/TheLarkInn/bureau/blob/main/docs/";
const FILTERS = [["all", "All runs"], ["attention", "Needs attention"], ["active", "Active"], ["paused", "Paused"], ["failed", "Failed"]];

function text(value, fallback = "Unknown") {
  return typeof value === "string" || typeof value === "number" ? String(value) : fallback;
}

function stamp(value) {
  return typeof value === "number" && Number.isFinite(value)
    ? new Date(value).toISOString().replace("T", " ").replace(/\.\d+Z$/u, " UTC") : "Not recorded";
}

function link(label, file) {
  return h("a", { href: `${GUIDE}${file}`, target: "_blank", rel: "noreferrer" }, label);
}

function button(label, target, onNavigate, props = {}) {
  return h("button", { type: "button", className: "btn btn--small", ...props, onClick: () => onNavigate(target) }, label);
}

export function OperationsView({ state, onNavigate, onRefresh }) {
  const listing = useRunListing();
  const lastRead = useRef(null);
  const runTitle = useRef(null);
  const [filter, setFilter] = useState("all");
  if (listing.observation?.state === "ready") lastRead.current = listing;
  const observed = listing.status === "error" && lastRead.current
    ? { ...listing, runs: lastRead.current.runs, observation: { ...listing.observation, last_at_ms: lastRead.current.observation.at_ms } }
    : listing;
  const overview = useMemo(() => operationsOverview(state, observed), [state, observed]);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState(null);
  const refresh = async () => {
    setRefreshing(true);
    setError(null);
    try {
      const result = await onRefresh();
      if (!result?.ok) throw new Error(result?.error || "Could not refresh configuration and run evidence.");
      await listing.refresh();
    } catch (failure) {
      setError(String(failure.message ?? failure));
    } finally {
      setRefreshing(false);
    }
  };
  return h("section", { className: "operations", "aria-labelledby": "operations-title" },
    h("div", { className: "operations-heading" },
      h("div", {}, h("h2", { id: "operations-title", tabIndex: -1 }, "Operations"),
        h("p", { className: "muted" }, "Authoring configuration, recorded run evidence, and next actions.")),
      h("button", { type: "button", className: "btn btn--small", disabled: refreshing, onClick: refresh },
        refreshing ? "Refreshing..." : "Refresh config and evidence")),
    error ? h("p", { role: "alert", className: "ops-notice ops-notice--danger" }, error) : null,
    h(Counts, { counts: overview.counts, onInspect: (key) => {
      setFilter(key);
      runTitle.current?.focus();
    } }),
    h("div", { className: "ops-source-grid" },
      h(Configuration, { config: overview.configuration, onNavigate, access: overview.access }),
      h(ExecutionSource, { sources: overview.recorded_sources, access: overview.access })),
    h(Runs, { overview, loading: listing.status === "loading", onNavigate, filter, setFilter, runTitle }),
    h(Assignments, { overview, onNavigate }),
    h("footer", { className: "ops-boundary" },
      h("h2", {}, "Local operations, not cloud controls"),
      h("p", {}, "This app canvas and the standalone dashboard read the same local evidence and use Bureau CLI controls. The public website does not connect to your runner."),
      h("p", {}, "Local SDK factories belong to pipeline steps. Experimental cloud tasks are separate; cloud pause, resume, cancel, retry, approval, and feedback are unsupported here."),
      h("div", { className: "ops-actions" }, link("Local factory setup", "getting-started.md#opt-in-local-copilot-factories"),
        link("Cloud limitations", "github-cloud-factories.md"), link("Choose a scenario", "scenarios.md"))));
}

function Counts({ counts, onInspect }) {
  return h("div", { className: "ops-counts", role: "group", "aria-label": "Observed run counts" },
    [["attention", "Needs attention"], ["active", "Active, observed"], ["paused", "Paused"], ["failed", "Failed"]].map(([key, label]) =>
      h("button", { key, type: "button", className: "btn ops-count-link", onClick: () => onInspect(key),
        "aria-label": `Show ${label.toLowerCase()} runs: ${counts ? counts[key] : "unknown count"}` },
      h("span", { className: "ops-count" }, counts ? String(counts[key]) : "Unknown"), h("span", {}, label))));
}

function Configuration({ config, onNavigate, access }) {
  const changes = config.git.changes;
  return h("section", { className: "ops-source", "aria-labelledby": "authoring-title" },
    h("h2", { id: "authoring-title" }, isReadOnly(access) ? "Configuration source (read-only)" : "Authoring configuration"),
    h("p", { className: `ops-verdict ops-verdict--${config.tone}` }, config.verdict),
    config.message ? h("p", {}, config.message) : null,
    h("code", { className: "ops-path" }, config.dir),
    h("dl", { className: "ops-details" },
      h("dt", {}, isReadOnly(access) ? "Displayed HEAD" : "Authoring HEAD"), h("dd", {}, h("code", {}, config.git.commit ?? "Not observed")),
      h("dt", {}, "Working-tree changes"), h("dd", {}, Array.isArray(changes)
        ? `${changes.length} config file${changes.length === 1 ? "" : "s"} changed or untracked`
        : config.git.message ?? "Not observed"),
      h("dt", {}, "Unsaved plan"), h("dd", {}, config.pending ? `${config.pending} pending changes; not validated as saved files` : "No pending plan"),
      h("dt", {}, "Validation read"), h("dd", {}, stamp(config.checked_at_ms))),
    changes?.length ? h("details", {}, h("summary", {}, "Changed authoring files"),
      h("ul", { className: "ops-file-list" }, changes.map((path) => h("li", { key: path }, h("code", {}, path))))) : null,
    config.errors.length ? h("ul", { className: "ops-errors", "aria-label": "Configuration validation errors" },
      config.errors.map((error, index) => h("li", { key: index }, text(error, JSON.stringify(error))))) : null,
    h("p", { className: "muted" }, isReadOnly(access)
      ? "This source is inspection-only. Edit a separate authoring checkout; validation is not execution authorization."
      : "Saving changes only the working tree. Validation is not review, a commit, or execution authorization."),
    h("div", { className: "ops-actions" }, button(isReadOnly(access) ? "Inspect configuration" : "Open configuration", { view: "config" }, onNavigate)));
}

function ExecutionSource({ sources, access }) {
  return h("section", { className: "ops-source", "aria-labelledby": "execution-source-title" },
    h("h2", { id: "execution-source-title" }, "Committed execution source"),
    h("p", { className: "ops-verdict ops-verdict--notice" }, "Current adopted source not observed"),
    h("p", {}, "Reconcile reads the configured committed remote/ref, not this authoring directory. A clean local HEAD does not prove what is running."),
    sources.length ? h("details", {}, h("summary", {}, `Recorded sources: ${sources.length} distinct revision${sources.length === 1 ? "" : "s"}`),
      sources.map((source) => h("dl", { key: JSON.stringify(source), className: "ops-details" },
        h("dt", {}, "Remote"), h("dd", {}, h("code", {}, source.remote)),
        h("dt", {}, "Ref"), h("dd", {}, h("code", {}, source.reference)),
        h("dt", {}, "Commit"), h("dd", {}, h("code", {}, source.commit))))) : h("p", { className: "muted" }, "No committed source is recorded in the available run evidence."),
    h("p", {}, "Run sources are historical evidence, not a claim about today's local settings or daemon."),
    isReadOnly(access)
      ? h("p", {}, "Managed source adoption is separate from this read-only view. Make changes in an authoring worktree and review the config PR.")
      : h("p", {}, "Inspect local setup with ", h("code", {}, "bureau doctor --json"), "; change the reviewed source with ", h("code", {}, "bureau setup"), "."),
    h("div", { className: "ops-actions" }, link("Setup and review guide", "getting-started.md")));
}

function Runs({ overview, loading, onNavigate, filter, setFilter, runTitle }) {
  const [query, setQuery] = useState("");
  const runs = filterOperationRuns(overview.runs, filter, query);
  const { observation } = overview;
  return h("section", { className: "ops-section", "aria-labelledby": "observed-runs-title" },
    h("div", { className: "operations-heading" },
      h("h2", { id: "observed-runs-title", ref: runTitle, tabIndex: -1 }, "Observed runs"),
      h("p", { className: "muted" }, `Last read: ${stamp(observation.last_at_ms ?? observation.at_ms)}`)),
    h("p", { className: "muted" }, "Run logs are not a daemon heartbeat. Unfinished runs with no event in five minutes are labeled stale; a quiet run is not proof of a stopped process."),
    observation.state !== "ready" ? h("p", { className: "ops-notice", role: "status" },
      loading ? "Reading run evidence..." : `${observation.message ?? "Run observation is unavailable."}${observation.last_at_ms ? " Showing the last readable snapshot, not current activity." : ""}`) : null,
    observation.dir ? h("p", { className: "muted ops-path" }, "Run directory: ", h("code", {}, observation.dir)) : null,
    h("div", { className: "ops-run-tools" },
      h("div", { className: "ops-filters", role: "group", "aria-label": "Filter observed runs" },
        FILTERS.map(([key, label]) => h("button", { key, type: "button", className: "btn btn--small", "aria-pressed": filter === key, onClick: () => setFilter(key) }, label))),
      h("label", { className: "ops-search" }, "Find a run",
        h("input", { type: "search", value: query, placeholder: "Run, assignment, pipeline, or step", onChange: (event) => setQuery(event.target.value) }))),
    observation.state === "ready" ? h("p", { className: "muted", role: "status" }, `${runs.length} of ${overview.runs.length} recorded runs shown. Attention first, then newest evidence.`) : null,
    runs.length ? h("ol", { className: "ops-run-list", "aria-label": "Run evidence" }, runs.map((run) =>
      h(Run, { key: run.run_id, run, onNavigate }))) : h("p", { className: "ops-empty" },
        observation.state === "ready" ? (overview.runs.length ? "No runs match these filters." : "No runs recorded in this directory. Open a pipeline's Live view to inspect its controls; nothing starts automatically.") : "No current run evidence to display."));
}

function Run({ run, onNavigate }) {
  const evidence = run.evidence ?? {};
  return h("li", { className: "ops-run", "data-run-id": run.run_id },
    h("div", { className: "ops-run-main" },
      h("div", {}, h("h3", { className: "ops-run-title" }, run.run_id),
        h("p", { className: "muted" }, text(run.assignment, "Assignment not recorded"), " / ", text(run.pipeline, "Pipeline not recorded")),
        run.current_step ? h("p", {}, "Step: ", h("code", {}, text(run.current_step))) : null),
      h("div", { className: "ops-run-state" },
        h("span", { className: `ops-state ops-state--${run.status}` }, run.label),
        run.stale ? h("span", { className: "muted" }, "Stale or missing timestamp") : null),
      h("dl", { className: "ops-run-facts" },
        h("dt", {}, "Last event"), h("dd", {}, stamp(evidence.last_at_ms)),
        h("dt", {}, "Recorded cost"), h("dd", {}, run.cost)),
      h("div", { className: "ops-actions" }, run.target
        ? button(run.target.mode === "live" ? "Inspect in Live" : "Open in Replay", run.target, onNavigate,
          { "aria-label": `${run.target.mode === "live" ? "Inspect in Live" : "Open in Replay"}: ${run.run_id}` })
        : h("span", { className: "muted" }, "No safe pipeline link"))),
    evidence.message ? h("p", { className: "ops-notice" }, text(evidence.message)) : null,
    evidence.detail ? h("p", { className: "ops-run-detail" }, text(evidence.detail)) : null,
    h("details", { className: "ops-run-evidence" }, h("summary", {}, "Run source and evidence"),
      h("dl", { className: "ops-details" },
        h("dt", {}, "Pipeline attribution"), h("dd", {}, run.attributed_by),
        h("dt", {}, "Bureau outcome"), h("dd", {}, text(evidence.outcome, "Not recorded")),
        h("dt", {}, "Config commit"), h("dd", {}, h("code", {}, text(evidence.config_source?.commit, "Not recorded")))),
      h("p", { className: "muted" }, "Recorded USD is a log measurement, not an invoice or remaining budget. Missing accounting is unknown, not zero."),
      h("a", { href: `./runs/${encodeURIComponent(run.run_id)}/events`, target: "_blank", rel: "noreferrer" }, "Read run event response"),
      h("p", {}, "Authoritative inspection: ", h("code", {}, `bureau show ${run.run_id} --json`))),
    run.copilot_factories ? h(Factories, { factories: run.copilot_factories }) : null);
}

function Factories({ factories }) {
  return h("details", { className: "ops-factories" },
    h("summary", {}, "Local SDK factory evidence"),
    h("p", {}, "Native factory status is separate from the Bureau run outcome. Continuation eligibility is decided by Bureau, not this overview."),
    factories.error ? h("p", { className: "ops-notice" }, text(factories.error)) : null,
    Object.values(factories.records ?? {}).map((record) => h("dl", { key: record.sessionId, className: "ops-details" },
      factoryDetails(record).map(([label, value]) => h(React.Fragment, { key: label },
        h("dt", {}, label), h("dd", {}, text(value)))))));
}

function Assignments({ overview, onNavigate }) {
  const assignmentAction = isReadOnly(overview.access) ? "Inspect assignment" : "Configure assignment";
  const controlsAction = isReadOnly(overview.access) ? "Inspect Live" : "Run controls";
  return h("section", { className: "ops-section", "aria-labelledby": "ops-assignments-title" },
    h("h2", { id: "ops-assignments-title" }, "Assignments and safeguards"),
    h("p", { className: "muted" }, `${overview.configuration.sample ? "Sample declarations" : isReadOnly(overview.access) ? "Displayed declarations" : "Authoring declarations"} only. Limits are configured ceilings, not measured headroom, quota, or permission to start work.`),
    overview.assignments.length ? h("ul", { className: "ops-assignment-list" }, overview.assignments.map((assignment) =>
      h("li", { key: assignment.name, className: "ops-assignment" },
        h("div", { className: "operations-heading" },
          h("div", {}, h("h3", { className: "ops-run-title" }, assignment.name),
            h("p", { className: "muted" }, text(assignment.work?.source, "No work source"), " / ", text(assignment.pipeline, "No pipeline"))),
          h("div", { className: "ops-actions" },
            button(assignmentAction, { view: "config", assignment: assignment.name }, onNavigate, { "aria-label": `${assignmentAction} ${assignment.name}` }),
            assignment.pipeline_available ? button("Open pipeline", { view: "pipeline", pipeline: assignment.pipeline, mode: "design" }, onNavigate,
              { "aria-label": `Open pipeline ${assignment.pipeline}` }) : null,
            assignment.pipeline_available ? button(controlsAction, { view: "pipeline", pipeline: assignment.pipeline, mode: "live" }, onNavigate,
              { "aria-label": `${controlsAction} for ${assignment.name}` }) : null)),
        h("p", { className: "muted" }, `${assignment.runs.length} recorded run${assignment.runs.length === 1 ? "" : "s"} attributed to this assignment. Repositories: ${(assignment.repos ?? []).join(", ") || "none"}.`),
        h("details", {}, h("summary", {}, "Configured safeguards"),
          h("ul", { className: "ops-safeguards" }, assignment.safeguards.map((value) => h("li", { key: value }, value)))))))
      : h("div", { className: "ops-empty" }, h("p", {}, "No assignments available. Inspect configuration validation before choosing a reviewed scenario."),
        h("div", { className: "ops-actions" }, button("Open configuration", { view: "config" }, onNavigate), link("Choose a scenario", "scenarios.md"))));
}
