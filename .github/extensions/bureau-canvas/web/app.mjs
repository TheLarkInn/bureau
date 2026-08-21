import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Background,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  getSmoothStepPath,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
} from "@xyflow/react";
// graph-overlays: mode switcher plus live/replay overlay controllers. The
// pipeline graph rendering below stays as-is; overlay modes only restyle it.
import { ModeSwitcher } from "./modes.js";
import { useLiveOverlay } from "./live/live.js";
import { useReplayOverlay } from "./replay/replay.js";
import { resolveOverlay } from "./live/overlay.js";

const h = React.createElement;
const CARD_WIDTH = 240;
const CARD_HEIGHT = 112;
const CONFIG_PAD = 72;
const FRAME_PAD = 34;
const flowItemTypes = {
  stepCard: StepCard,
  terminalPill: TerminalPill,
  concurrentFrame: ConcurrentFrame,
};
const configItemTypes = { configCard: ConfigCardNode };
const flowEdgeTypes = { routed: RoutedEdge };
const edgeColors = {
  success: "var(--outcome-success)",
  failure: "var(--outcome-failure)",
  blocked: "var(--outcome-blocked)",
  "no-work": "var(--outcome-no-work)",
  data: "var(--relation-data)",
  observes: "var(--relation-observes)",
};

createRoot(document.querySelector("#root")).render(h(App));
window.__bureauCanvasMounted = true;
window.dispatchEvent(new Event("bureau-mounted"));

function App() {
  const [state, setState] = useState(null);
  const [selectedStep, setSelectedStep] = useState(null);

  useEffect(() => {
    let alive = true;
    fetch("./state", { cache: "no-store" })
      .then((response) => response.json())
      .then((next) => alive && setState(next));
    const events = new EventSource("./events");
    const localState = (event) => setState(event.detail);
    events.addEventListener("state", (event) => setState(JSON.parse(event.data)));
    events.addEventListener("focus", (event) => applyFocus(JSON.parse(event.data), setSelectedStep, setState));
    window.addEventListener("bureau-state", localState);
    return () => {
      alive = false;
      events.close();
      window.removeEventListener("bureau-state", localState);
    };
  }, []);

  if (!state) {
    return h("main", { className: "app-shell" }, h("p", { className: "status" }, "Loading…"));
  }

  return h(
    "main",
    { className: "app-shell" },
    h(Header, { state }),
    h(DraftBar, { plan: state.plan }),
    state.selectedPipeline ? null : h(CreateBar, { dir: state.dir }),
    h(Findings, { className: "general-findings", findings: state.generalFindings ?? [] }),
    state.selectedPipeline
      ? h(PipelineView, { state, selectedStep, setSelectedStep })
      : h(ConfigView, { state }),
  );
}

function applyFocus(payload, setSelectedStep, setState) {
  const focus = payload?.focus;
  if (focus?.kind === "step") {
    setSelectedStep(focus.step ?? focus.name ?? null);
  }
  if (focus?.kind === "pipeline") {
    postIntent({ kind: "open-pipeline", pipeline: focus.name ?? focus.pipeline }).then((result) => {
      if (result?.ok) {
        setState(result.state);
      }
    });
  }
}

function Header({ state }) {
  const view = state.config?.view ?? emptyConfigView();
  return h(
    "header",
    { className: "app-header" },
    h("div", {}, h("h1", {}, "Bureau config"), h("p", { className: "summary" }, summaryText(view))),
    h(
      "div",
      { className: "status", "aria-live": "polite" },
      h("p", {}, state.status),
      h("p", {}, state.validation?.dir ?? state.dir),
      h("p", {}, `${view.orphans.length} orphan${view.orphans.length === 1 ? "" : "s"}`),
    ),
  );
}

/**
 * Unsaved work has to look unsaved. Every card looked saved until now because
 * everything always was; a pending create, rename or delete must read
 * differently and be discardable.
 */
function DraftBar({ plan }) {
  if (!plan) {
    return null;
  }
  const pending = plan.writes.length + plan.removals.length;
  return h(
    "section",
    { className: "draft-bar", "data-testid": "draft-bar" },
    h("p", {}, `${pending} unsaved change${pending === 1 ? "" : "s"}`),
    h("ul", { className: "draft-list" }, [
      ...plan.writes.map((path) => h("li", { key: `w:${path}` }, `write ${shortPath(path)}`)),
      ...plan.removals.map((path) => h("li", { key: `r:${path}` }, `delete ${shortPath(path)}`)),
    ]),
    h(
      "div",
      { className: "draft-actions" },
      h("button", { type: "button", onClick: () => postIntent({ kind: "save-plan" }).then(publishLocalState) }, "Save"),
      h("button", { type: "button", onClick: () => postIntent({ kind: "discard-plan" }).then(publishLocalState) }, "Discard"),
    ),
  );
}

function shortPath(path) {
  return String(path).replaceAll("\\", "/").split("/").slice(-2).join("/");
}

/** Create controls, one per kind, scaffolded so a new entity is valid at once. */
function CreateBar({ dir }) {
  const [kind, setKind] = useState("role");
  const [name, setName] = useState("");
  const submit = (event) => {
    event.preventDefault();
    if (!name.trim()) {
      return;
    }
    postIntent({ kind: "create", input: { dir, kind, name: name.trim(), fields: {} } }).then((result) => {
      setName("");
      publishLocalState(result);
    });
  };
  return h(
    "form",
    { className: "create-bar", onSubmit: submit, "data-testid": "create-bar" },
    h(
      "select",
      { value: kind, onChange: (event) => setKind(event.target.value), "aria-label": "New entity kind" },
      ["repo", "role", "assignment", "pipeline"].map((option) => h("option", { key: option, value: option }, option)),
    ),
    h("input", { value: name, onChange: (event) => setName(event.target.value), placeholder: "name", "aria-label": "New entity name" }),
    h("button", { type: "submit" }, "Create"),
  );
}

/** Delete asks first and shows what breaks; the entry-step case reads louder. */
function DeleteControl({ dir, kind, name }) {
  const [preflight, setPreflight] = useState(null);
  const ask = () => postIntent({ kind: "delete", input: { dir, kind, name } }).then((response) => setPreflight(response?.result ?? null));
  const confirm = () => postIntent({ kind: "delete", input: { dir, kind, name, confirm: true } }).then((result) => {
    setPreflight(null);
    publishLocalState(result);
  });
  if (!preflight) {
    return h("button", { type: "button", className: "card-action", onClick: ask }, "Delete");
  }
  return h(
    "div",
    { className: `preflight${preflight.referrers?.length ? " preflight--blocking" : ""}`, "data-testid": "preflight" },
    h("p", {}, preflight.referrers?.length ? `${preflight.referrers.length} reference${preflight.referrers.length === 1 ? "" : "s"}` : "Nothing references this"),
    h("ul", {}, (preflight.referrers ?? []).map((item) => h("li", { key: item.name, className: `severity-${item.severity}` }, item.message))),
    h("button", { type: "button", onClick: confirm }, "Confirm delete"),
    h("button", { type: "button", onClick: () => setPreflight(null) }, "Cancel"),
  );
}

function emptyConfigView() {
  return { assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
}

function summaryText(view) {
  return `${view.assignments.length} assignment · ${view.roles.length} roles · ${view.repos.length} repos · ${view.pipelines.length} pipelines`;
}

function ConfigView({ state }) {
  const view = state.config?.view ?? emptyConfigView();
  return h(
    "section",
    { className: "view-shell view-shell--config" },
    h("h2", { className: "config-heading" }, "Assignments"),
    h(AssignmentStack, { state, view }),
    h(OrphanStrip, { view }),
    h(RelationSection, { state }),
  );
}

/** The landing: assignments as a vertical stack, each expanding in place. */
function AssignmentStack({ state, view }) {
  const [expanded, setExpanded] = useState(null);
  const toggle = (name) => setExpanded((current) => (current === name ? null : name));
  if (!view.assignments.length) {
    return h("p", { className: "muted" }, "No assignments yet.");
  }
  return h(
    "div",
    { className: "assignment-stack" },
    view.assignments.map((assignment) =>
      h(AssignmentCard, {
        key: assignment.name,
        state,
        assignment,
        expanded: expanded === assignment.name,
        onToggle: () => toggle(assignment.name),
      }),
    ),
  );
}

function AssignmentCard({ state, assignment, expanded, onToggle }) {
  const findings = state.findingsByItem?.[`assignment:${assignment.name}`] ?? [];
  const className = `assignment-card${expanded ? " assignment-card--expanded" : ""}`;
  return h(
    "article",
    { className, "data-ref": `assignment:${assignment.name}` },
    h(
      "button",
      {
        type: "button",
        className: "assignment-head",
        "aria-expanded": Boolean(expanded),
        onClick: onToggle,
      },
      h("span", { className: "kind-label" }, "assignment"),
      h("span", { className: "assignment-name" }, assignment.name),
      h("span", { className: "assignment-glance" }, assignmentGlance(assignment)),
    ),
    expanded ? h(AssignmentDetail, { state, assignment }) : null,
    findings.length ? h(Findings, { findings }) : null,
  );
}

/** The one-line at-a-glance summary shown on a collapsed card. */
function assignmentGlance(assignment) {
  const repos = (assignment.repos ?? []).length;
  const limits = Object.values(assignment.limits ?? {}).filter((value) => value != null).length;
  return `${assignment.work?.source ?? "no source"} · ${repos} repo${repos === 1 ? "" : "s"} · ${limits} limit${limits === 1 ? "" : "s"}`;
}

/** The expanded body: work source, repos, role, pipeline, limits. */
function AssignmentDetail({ state, assignment }) {
  return h(
    "div",
    { className: "assignment-detail" },
    h(DetailRow, { label: "work source" }, h(WorkSourceField, { assignment })),
    h(DetailRow, { label: "filter" }, h("code", {}, assignment.work?.filter ?? "—")),
    assignment.work?.approvalLabel ? h(DetailRow, { label: "approval label" }, h("code", {}, assignment.work.approvalLabel)) : null,
    h(DetailRow, { label: "repos" }, h(ReposField, { state, assignment })),
    h(DetailRow, { label: "role" }, h(RoleField, { state, assignment })),
    h(DetailRow, { label: "pipeline" }, h(PipelineLink, { name: assignment.pipeline })),
    h(DetailRow, { label: "verify" }, h("code", {}, assignment.verify ?? "not set")),
    h(DetailRow, { label: "limits" }, h(LimitsField, { assignment })),
  );
}

function DetailRow({ label, children }) {
  return h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, label), h("span", { className: "detail-value" }, children));
}

function RefLink({ kind, name }) {
  return h("span", { className: `ref ref--${kind}` }, name ?? "—");
}

function PipelineLink({ name }) {
  if (!name) {
    return h("span", { className: "muted" }, "—");
  }
  return h(
    "button",
    { type: "button", className: "ref ref--pipeline ref-link", onClick: () => selectPipeline(name) },
    name,
  );
}

const ACCESS_LEVELS = ["read", "pr", "push"];
/** Only these grants can take a branch, so only they can hold the primary. */
const LANDING_ACCESS = ["pr", "push"];

function repoEntry(state, name) {
  return (state.config?.view?.repos ?? []).find((repo) => repo.name === name);
}

/**
 * Why the first repo cannot take the branch, when it cannot.
 *
 * A repo missing from the registry is a different problem from one that is
 * registered read-only: the first needs registering, the second needs a
 * different repo promoted. Reporting either as the other sends the operator
 * to the wrong fix.
 */
function landingProblem(state, name) {
  const entry = repoEntry(state, name);
  if (!entry) {
    return { kind: "unknown", message: `\`${name}\` is not in repos.yaml, so its access is unknown — register it before a branch can land there.` };
  }
  if (!LANDING_ACCESS.includes(entry.access)) {
    return { kind: "read-only", message: `\`${name}\` is read-only, so no branch can land there. Move up a repo with push or pr access.` };
  }
  return null;
}

function AccessTag({ access }) {
  return h("span", { className: `access access--${access ?? "unknown"}` }, access ?? "unregistered");
}

/**
 * The repos list, clickable: order is meaning here, so the editor is a ranked
 * list rather than a set of chips. The first entry is the primary repo and
 * the branch lands there, which is invisible in YAML and stated here.
 */
function ReposField({ state, assignment }) {
  const [editing, setEditing] = useState(false);
  if (editing) {
    return h(ReposEditor, { state, assignment, onDone: () => setEditing(false) });
  }
  const repos = assignment.repos ?? [];
  return h(
    "button",
    {
      type: "button", className: "repos-value", "aria-expanded": false,
      title: "Change the repos this assignment touches",
      onClick: () => setEditing(true),
    },
    repos.length
      ? h("span", { className: "chips" }, repos.map((name, index) =>
          h("span", { key: name, className: `chip repo-chip${index === 0 ? " repo-chip--primary" : ""}` },
            index === 0 ? `${name} · primary` : name)))
      : h("span", { className: "muted" }, "no repos"),
  );
}

function ReposEditor({ state, assignment, onDone }) {
  const [repos, setRepos] = useState(assignment.repos ?? []);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const commit = (next, register) => {
    setBusy(true);
    const input = { assignment: assignment.name, repos: next, ...(register ? { register } : {}) };
    postIntent({ kind: "set-repos", input }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
        onDone();
      } else {
        setError(result?.error ?? "could not save those repos");
      }
    });
  };

  const move = (from, to) => {
    const next = [...repos];
    [next[from], next[to]] = [next[to], next[from]];
    setRepos(next);
  };

  if (adding) {
    return h(RepoAdder, {
      state, repos, busy, error,
      onCancel: () => { setAdding(false); setError(null); },
      onPick: (name) => { setRepos([...repos, name]); setAdding(false); },
      onRegister: (register) => commit([...repos, register.name], register),
    });
  }
  const problem = repos.length ? landingProblem(state, repos[0]) : null;
  return h(
    "div",
    { className: "repos-editor" },
    repos.length
      ? repos.map((name, index) => h(RepoRow, {
          key: name, state, name, index, total: repos.length,
          problem: index === 0 ? problem : null,
          onUp: () => move(index, index - 1),
          onDown: () => move(index, index + 1),
          onRemove: () => setRepos(repos.filter((item) => item !== name)),
        }))
      : h("p", { className: "note" }, "No repos yet — add the one the branch should land in."),
    problem
      ? h("p", { className: problem.kind === "unknown" ? "note note--warn" : "note note--err" }, problem.message)
      : h("p", { className: "note" }, "Rows below the first supply read-only context to the run."),
    error ? h("p", { className: "note note--err" }, error) : null,
    h("div", { className: "actions" },
      h("button", {
        type: "button", className: "btn btn--primary",
        disabled: busy || sameOrder(repos, assignment.repos ?? []),
        onClick: () => commit(repos),
      }, busy ? "Saving…" : "Save repos"),
      h("button", { type: "button", className: "btn", onClick: () => setAdding(true) }, "+ Add repo"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel")),
  );
}

function sameOrder(left, right) {
  return left.length === right.length && left.every((name, index) => name === right[index]);
}

function RepoRow({ state, name, index, total, problem, onUp, onDown, onRemove }) {
  const primary = index === 0;
  const entry = repoEntry(state, name);
  const flag = problem ? ` repo-row--${problem.kind === "unknown" ? "unknown" : "broken"}` : "";
  return h(
    "div",
    { className: `repo-row${primary ? " repo-row--primary" : ""}${flag}` },
    h("span", { className: "repo-rank" }, String(index + 1)),
    h("span", { className: "repo-name" }, name),
    h("span", { className: "repo-primary" }, primary ? "primary — the branch lands here" : ""),
    h(AccessTag, { access: entry?.access }),
    h("span", { className: "repo-move" },
      h("button", { type: "button", className: "icon-btn", disabled: index === 0, "aria-label": `Move ${name} up`, onClick: onUp }, "↑"),
      h("button", { type: "button", className: "icon-btn", disabled: index === total - 1, "aria-label": `Move ${name} down`, onClick: onDown }, "↓")),
    h("button", { type: "button", className: "icon-btn", "aria-label": `Remove ${name}`, onClick: onRemove }, "✕"),
  );
}

function RepoAdder({ state, repos, busy, error, onCancel, onPick, onRegister }) {
  const [url, setUrl] = useState("");
  const [resolved, setResolved] = useState(null);
  const [failure, setFailure] = useState(null);
  const [name, setName] = useState("");
  const [access, setAccess] = useState("read");
  const [credential, setCredential] = useState(credentialsOf(state)[0] ?? "");

  const registry = state.config?.view?.repos ?? [];
  const unlisted = registry.filter((repo) => !repos.includes(repo.name));

  const resolve = (next) => {
    setUrl(next);
    setResolved(null);
    setFailure(null);
    if (!next.trim()) {
      return;
    }
    postIntent({ kind: "resolve-repo", url: next }).then((result) => {
      const outcome = result?.resolved;
      if (outcome?.ok) {
        setResolved(outcome);
        setName(outcome.name);
      } else {
        setFailure(outcome?.reason ?? "could not read that URL");
      }
    });
  };

  const taken = registry.some((repo) => repo.name === name);
  const willLand = repos.length === 0;
  const landBlocked = willLand && !LANDING_ACCESS.includes(access);
  const credentials = credentialsOf(state);

  return h(
    "div",
    { className: "repos-editor" },
    h("span", { className: "detail-label" }, "add a repo"),
    unlisted.length
      ? h("div", { className: "repo-known" }, unlisted.map((repo) =>
          h("div", { key: repo.name, className: "repo-row" },
            h("span", { className: "repo-name" }, repo.name),
            h(AccessTag, { access: repo.access }),
            h("button", { type: "button", className: "icon-btn", "aria-label": `Add ${repo.name}`, onClick: () => onPick(repo.name) }, "add"))))
      : h("p", { className: "note" }, "Every registered repo is already listed."),
    h("p", { className: "note" }, "Not in the registry? Paste its URL and it is added to repos.yaml as well."),
    h("input", {
      type: "text", className: "paste-input", value: url, autoFocus: true, "aria-label": "Repository URL",
      placeholder: "Paste a repository URL…", onChange: (event) => resolve(event.target.value),
    }),
    failure ? h("p", { className: "note note--err" }, failure) : null,
    resolved ? h(ResolvedRepo, {
      resolved, name, setName, access, setAccess, credential, setCredential,
      credentials, taken, willLand, landBlocked,
    }) : null,
    error ? h("p", { className: "note note--err" }, error) : null,
    h("div", { className: "actions" },
      resolved
        ? h("button", {
            type: "button", className: "btn btn--primary",
            disabled: busy || !name || taken || landBlocked || !credential,
            onClick: () => onRegister({ name, url: resolved.url, forge: resolved.forge, access, credential }),
          }, busy ? "Adding…" : "Add to registry and this assignment")
        : null,
      h("button", { type: "button", className: "btn", onClick: onCancel }, "Back")),
  );
}

function credentialsOf(state) {
  return [...new Set((state.config?.view?.repos ?? []).map((repo) => repo.credential).filter(Boolean))];
}

function ResolvedRepo(props) {
  const { resolved, name, setName, access, setAccess, credential, setCredential, credentials, taken, willLand, landBlocked } = props;
  return h(
    "div",
    { className: "repos-preview" },
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "repo-name" }, "name"),
      h("input", { id: "repo-name", type: "text", className: "role-input", value: name, onChange: (event) => setName(event.target.value) })),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "forge"), h("code", {}, resolved.forge)),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "url"), h("code", {}, resolved.url)),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "repo-access" }, "access"),
      h("select", { id: "repo-access", className: "role-select", value: access, onChange: (event) => setAccess(event.target.value) },
        ACCESS_LEVELS.map((value) => h("option", { key: value, value }, value)))),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "repo-credential" }, "credential"),
      credentials.length
        ? h("select", { id: "repo-credential", className: "role-select", value: credential, onChange: (event) => setCredential(event.target.value) },
            credentials.map((value) => h("option", { key: value, value }, value)))
        : h("p", { className: "note note--err" }, "No credential is referenced by any registered repo yet. Add one to repos.yaml before registering — a repo without a credential cannot be cloned.")),
    taken ? h("p", { className: "note note--err" }, `\`${name}\` already names a different repository in the registry — rename this one.`) : null,
    willLand
      ? h("p", { className: landBlocked ? "note note--err" : "note" },
          landBlocked
            ? "This will be the first repo, so the branch lands here — read access cannot take a branch. Choose pr or push."
            : "This will be the first repo, so the branch lands here. Its access allows that.")
      : h("p", { className: "note" }, "Added below the primary, so the run reads it for context."),
  );
}

/**
 * Every limit, in the order the file writes them. `max_run_hours` is the one
 * whose absence is a ceiling rather than none — the system default — so it is
 * never reported as unlimited.
 */
const LIMIT_FIELDS = [
  { key: "max_concurrent", view: "maxConcurrent", unit: "runs at once", noun: "concurrent runs", short: "at once" },
  { key: "max_runs_per_hour", view: "maxRunsPerHour", unit: "runs / hour", noun: "runs per hour", short: "/hour" },
  { key: "max_runs_per_day", view: "maxRunsPerDay", unit: "runs / day", noun: "runs per day", short: "/day" },
  { key: "max_open_prs", view: "maxOpenPrs", unit: "open PRs", noun: "open pull requests", short: "open PRs" },
  { key: "max_cost_per_day_usd", view: "maxCostPerDayUsd", unit: "USD / day", noun: "spend per day", short: "USD/day" },
  { key: "max_run_hours", view: "maxRunHours", unit: "hours / run", noun: "run length", short: "h/run", defaulted: 24 },
];

const isSet = (limits, key) => limits[key] !== null && limits[key] !== undefined;

/** The view model spells the keys differently from the file. */
function limitsFromView(view) {
  return Object.fromEntries(LIMIT_FIELDS.map((field) => [field.key, view?.[field.view] ?? null]));
}

function defaultLimit(field) {
  return field.defaulted ?? (field.key === "max_cost_per_day_usd" ? 25 : 1);
}

/**
 * The resting summary: capped limits as ordinary chips, everything uncapped
 * collapsed into one. Six amber rows at rest would be noise; a silent blank
 * would be worse, because an omitted limit means no ceiling at all.
 */
function LimitsSummary({ limits }) {
  const capped = LIMIT_FIELDS.filter((field) => isSet(limits, field.key));
  const uncapped = LIMIT_FIELDS.filter((field) => !isSet(limits, field.key) && !field.defaulted);
  if (!capped.length) {
    return h("span", { className: "chips" }, h("span", { className: "chip chip--none" }, "unbounded — no limits set"));
  }
  return h(
    "span",
    { className: "chips" },
    capped.map((field) => h("span", { key: field.key, className: "chip" }, `${limits[field.key]} ${field.short}`)),
    uncapped.length ? h("span", { className: "chip chip--off" }, `${uncapped.length} unlimited`) : null,
    isSet(limits, "max_run_hours") ? null : h("span", { className: "chip" }, "24h/run default"),
  );
}

function LimitsField({ assignment }) {
  const [editing, setEditing] = useState(false);
  const limits = limitsFromView(assignment.limits);
  if (editing) {
    return h(LimitsEditor, { assignment, saved: limits, onDone: () => setEditing(false) });
  }
  return h(
    "button",
    {
      type: "button", className: "limits-value", "aria-expanded": false,
      title: "Change the limits on this assignment",
      onClick: () => setEditing(true),
    },
    h(LimitsSummary, { limits }),
  );
}

function LimitsEditor({ assignment, saved, onDone }) {
  const [draft, setDraft] = useState(saved);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const changed = LIMIT_FIELDS.some((field) => draft[field.key] !== saved[field.key]);
  // A cleared box is kept as its raw text rather than coerced: `Number("")`
  // is 0, and a zero limit computes headroom as permanently zero — a total
  // block produced by one backspace.
  const invalid = LIMIT_FIELDS.some((field) => isSet(draft, field.key) && !validLimit(draft[field.key]));
  const set = (key, value) => setDraft((current) => ({ ...current, [key]: value }));

  const save = () => {
    setBusy(true);
    postIntent({ kind: "set-limits", input: { assignment: assignment.name, limits: draft } }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
        onDone();
      } else {
        setError(result?.error ?? "could not save those limits");
      }
    });
  };

  return h(
    "div",
    { className: "limits-editor" },
    LIMIT_FIELDS.map((field) => h(LimitRow, {
      key: field.key, field, value: draft[field.key],
      onToggle: () => set(field.key, isSet(draft, field.key) ? null : defaultLimit(field)),
      onChange: (value) => set(field.key, value),
    })),
    h("p", { className: "note" }, "Off means no ceiling at all — except run length, which falls back to the system default of 24 hours."),
    invalid ? h("p", { className: "note note--err" }, "A limit that is on needs a whole number of at least 1. Switch it off for unlimited.") : null,
    error ? h("p", { className: "note note--err" }, error) : null,
    h("div", { className: "actions" },
      h("button", { type: "button", className: "btn btn--primary", disabled: busy || !changed || invalid, onClick: save },
        busy ? "Saving…" : "Save limits"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel"),
      changed ? h("span", { className: "limits-dirty" }, "unsaved changes") : null),
  );
}

function validLimit(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 1;
}

function LimitRow({ field, value, onToggle, onChange }) {
  const on = value !== null && value !== undefined;
  const uncapped = !on && !field.defaulted;
  const bad = on && !validLimit(value);
  return h(
    "div",
    { className: `limit-row${uncapped ? " limit-row--off" : ""}` },
    h("button", {
      type: "button", className: "switch", "aria-pressed": on,
      "aria-label": `${field.noun} limit`,
      onClick: onToggle,
    }, on ? "on" : "off"),
    h("span", { className: "limit-name" }, field.key),
    on
      ? h("input", {
          type: "number", min: "1", step: "1",
          className: `limit-input${bad ? " limit-input--invalid" : ""}`,
          value: String(value), "aria-label": field.noun,
          "aria-invalid": bad ? "true" : undefined,
          onChange: (event) => {
            const raw = event.target.value;
            onChange(raw === "" || Number.isNaN(Number(raw)) ? raw : Number(raw));
          },
        })
      : h("span", { className: `note${field.defaulted ? "" : " note--warn"}` },
          field.defaulted ? "system default" : "unlimited"),
    h("span", { className: "limit-unit" }, !on && field.defaulted ? `${field.defaulted} ${field.unit}` : field.unit),
  );
}

/**
 * The work source value, clickable: paste the board or issues page you are
 * already looking at and the fields are derived from it. Deriving is a
 * preview — nothing is written until the derivation is accepted.
 */
function WorkSourceField({ assignment }) {
  const [open, setOpen] = useState(false);
  const label = `${assignment.work?.forge ?? "?"} · ${assignment.work?.source ?? "?"}`;
  if (!open) {
    return h(
      "button",
      { type: "button", className: "ws-value", title: "Link a board or issues page", onClick: () => setOpen(true) },
      label,
    );
  }
  return h(WorkSourceEditor, { assignment, onDone: () => setOpen(false) });
}

function WorkSourceEditor({ assignment, onDone }) {
  const [url, setUrl] = useState("");
  const [derived, setDerived] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const derive = (next) => {
    setUrl(next);
    setDerived(null);
    setError(null);
    if (!next.trim()) {
      return;
    }
    postIntent({ kind: "derive-work-source", url: next }).then((result) => {
      const outcome = result?.derived;
      if (outcome?.ok) {
        setDerived(outcome);
      } else {
        setError(outcome?.reason ?? "could not read that URL");
      }
    });
  };

  const apply = () => {
    setBusy(true);
    postIntent({ kind: "set-work-source", input: { assignment: assignment.name, url } }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
        onDone();
      } else {
        setError(result?.error ?? "could not save that work source");
      }
    });
  };

  return h(
    "div",
    { className: "ws-open" },
    h("span", { className: "detail-label" }, "link a work source"),
    h("input", {
      type: "text",
      className: "paste-input",
      autoFocus: true,
      placeholder: "Paste a board, query, or issues URL…",
      value: url,
      onChange: (event) => derive(event.target.value),
    }),
    error ? h("p", { className: "note note--err" }, error) : null,
    derived ? h(DerivedWorkSource, { derived }) : null,
    h(
      "div",
      { className: "actions" },
      h("button", {
        type: "button",
        className: "btn btn--primary",
        disabled: !derived || busy,
        onClick: apply,
      }, busy ? "Saving…" : "Use this"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel"),
    ),
  );
}

/** What the paste would write, including anything it had to infer. */
function DerivedWorkSource({ derived }) {
  return h(
    "div",
    { className: "derived" },
    h("div", { className: "derived-row" }, h("span", { className: "detail-label" }, "forge"),
      h("span", { className: `pill pill--${derived.forge}` }, derived.forge)),
    h("div", { className: "derived-row" }, h("span", { className: "detail-label" }, "source"), h("code", {}, derived.source)),
    h("div", { className: "derived-row" }, h("span", { className: "detail-label" }, "filter"), h("code", {}, derived.filter)),
    derived.exact
      ? h("p", { className: "note" }, "derived exactly from the URL")
      : (derived.notes ?? []).map((note, index) => h("p", { className: "note note--warn", key: index }, `⚠ ${note}`)),
  );
}

const PERMISSIONS = [
  "repo:read", "repo:write", "repo:push", "issues:read", "issues:write",
  "pr:read", "pr:write", "pr:review", "pr:merge", "runs:read", "model:invoke",
];
const TRUST_GRADES = ["untrusted", "derived", "maintainer", "trusted"];
const ADAPTERS = ["copilot", "claude", "fake"];
/** A new role reads and thinks, nothing more, until told otherwise. */
const LEAST_PRIVILEGE = ["repo:read", "model:invoke"];

/** How consequential a grant is, which drives its colour and the warning. */
function permissionRisk(permission) {
  if (permission === "repo:push" || permission === "pr:merge") {
    return "push";
  }
  if (permission.endsWith(":write") || permission === "pr:review") {
    return "write";
  }
  return permission === "model:invoke" ? "model" : "read";
}

function PermissionChips({ permissions }) {
  if (!permissions?.length) {
    return h("span", { className: "muted" }, "no grants");
  }
  return h("span", { className: "chips" }, permissions.map((permission) =>
    h("span", { key: permission, className: `perm perm--${permissionRisk(permission)}` }, permission)));
}

/**
 * The role value, clickable: the capability summary opens in place into a
 * picker over existing roles, or a form for a new one. Renaming and deleting
 * are deliberately absent — a role is shared with every pipeline step that
 * names it, so those belong where all its referrers are visible.
 */
function RoleField({ state, assignment }) {
  const [editing, setEditing] = useState(false);
  const roles = state.config?.view?.roles ?? [];
  const role = roles.find((candidate) => candidate.name === assignment.role);
  if (editing) {
    return h(RoleEditor, { state, assignment, roles, onDone: () => setEditing(false) });
  }
  return h(
    "button",
    {
      type: "button",
      className: "rolebox",
      "aria-expanded": false,
      onClick: () => setEditing(true),
    },
    h("span", { className: "rolebox-top" },
      h("span", { className: "rolename" }, assignment.role ?? "not set"),
      role ? h("span", { className: "trust" }, role.minTrust) : null),
    h(PermissionChips, { permissions: role?.permissions }),
    role ? h("span", { className: "note" }, `${role.adapter} · ${role.agent}`) : null,
  );
}

function RoleEditor({ state, assignment, roles, onDone }) {
  const [creating, setCreating] = useState(false);
  const [pending, setPending] = useState(assignment.role);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const commit = (input) => {
    setBusy(true);
    postIntent({ kind: "set-role", input: { assignment: assignment.name, ...input } }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
        onDone();
      } else {
        setError(result?.error ?? "could not save that role");
      }
    });
  };

  if (creating) {
    return h(RoleCreator, {
      roles, busy, error,
      onCancel: () => { setCreating(false); setError(null); },
      onCreate: (name, create) => commit({ role: name, create }),
    });
  }
  const preview = roles.find((candidate) => candidate.name === pending);
  return h(
    "div",
    { className: "role-editor" },
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "role-pick" }, "role"),
      h("select", {
        id: "role-pick", className: "role-select", value: pending ?? "",
        onChange: (event) => setPending(event.target.value),
      }, roles.map((candidate) => h("option", { key: candidate.name, value: candidate.name }, candidate.name)))),
    preview ? h(RolePreview, { role: preview }) : null,
    error ? h("p", { className: "note note--err" }, error) : null,
    h("p", { className: "note" }, "Renaming or deleting a role happens on the role itself — it is shared with every pipeline step that names it."),
    h("div", { className: "actions" },
      h("button", {
        type: "button", className: "btn btn--primary",
        disabled: busy || !pending || pending === assignment.role,
        onClick: () => commit({ role: pending }),
      }, busy ? "Saving…" : "Use this role"),
      h("button", { type: "button", className: "btn", onClick: () => setCreating(true) }, "+ New role"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel")),
  );
}

function RolePreview({ role }) {
  return h(
    "div",
    { className: "role-preview" },
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "adapter"), h("code", {}, role.adapter)),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "grants"), h(PermissionChips, { permissions: role.permissions })),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "min trust"), h("span", { className: "trust" }, role.minTrust)),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "agent"), h("code", {}, role.agent)),
  );
}

function RoleCreator({ roles, busy, error, onCancel, onCreate }) {
  const [name, setName] = useState("");
  const [agent, setAgent] = useState("/bureau:implementer");
  const [adapter, setAdapter] = useState("copilot");
  const [minTrust, setMinTrust] = useState("derived");
  const [permissions, setPermissions] = useState(LEAST_PRIVILEGE);

  const taken = roles.some((candidate) => candidate.name === name);
  const elevated = permissions.filter((permission) => ["push", "write"].includes(permissionRisk(permission)));
  const toggle = (permission) => setPermissions((current) =>
    current.includes(permission) ? current.filter((item) => item !== permission) : [...current, permission]);

  return h(
    "div",
    { className: "role-editor" },
    h("span", { className: "detail-label" }, "new role"),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "role-name" }, "name"),
      h("input", { id: "role-name", type: "text", className: "role-input", value: name, placeholder: "patcher", autoFocus: true, onChange: (event) => setName(event.target.value) })),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "role-agent" }, "agent"),
      h("input", { id: "role-agent", type: "text", className: "role-input", value: agent, placeholder: "/plugin:agent", onChange: (event) => setAgent(event.target.value) })),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "role-adapter" }, "adapter"),
      h("select", { id: "role-adapter", className: "role-select", value: adapter, onChange: (event) => setAdapter(event.target.value) },
        ADAPTERS.map((value) => h("option", { key: value, value }, value)))),
    h("div", { className: "detail-row" },
      h("span", { className: "detail-label", id: "role-grants" }, "grants"),
      h("div", { className: "perm-grid", role: "group", "aria-labelledby": "role-grants" }, PERMISSIONS.map((permission) =>
        h("button", {
          key: permission, type: "button", className: "perm-toggle",
          "data-risk": permissionRisk(permission),
          "aria-pressed": permissions.includes(permission),
          onClick: () => toggle(permission),
        }, permission)))),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "role-trust" }, "min trust"),
      h("select", { id: "role-trust", className: "role-select", value: minTrust, onChange: (event) => setMinTrust(event.target.value) },
        TRUST_GRADES.map((value) => h("option", { key: value, value }, value)))),
    taken ? h("p", { className: "note note--err" }, `\`${name}\` already exists — choose it from the list instead.`) : null,
    elevated.length
      ? h("p", { className: "note note--warn" }, `${elevated.join(", ")} hands the run a credential that can change things. Review of the config PR is the only gate on that.`)
      : h("p", { className: "note" }, "Read-only grants. A step that never pushes should never hold a token that can."),
    error ? h("p", { className: "note note--err" }, error) : null,
    h("div", { className: "actions" },
      h("button", {
        type: "button", className: "btn btn--primary",
        disabled: busy || !name || taken,
        onClick: () => onCreate(name, { agent, adapter, permissions, min_trust: minTrust }),
      }, busy ? "Creating…" : "Create and use it"),
      h("button", { type: "button", className: "btn", onClick: onCancel }, "Back")),
  );
}

/** Repos / roles / pipelines nothing references, surfaced without graph noise. */
function OrphanStrip({ view }) {
  if (!view.orphans?.length) {
    return null;
  }
  return h(
    "section",
    { className: "orphan-strip", "aria-label": "Unreferenced config" },
    h("h3", {}, "Unreferenced"),
    h(
      "div",
      { className: "chips" },
      view.orphans.map((orphan) =>
        h("span", { key: `${orphan.kind}:${orphan.name}`, className: `chip orphan-chip orphan-chip--${orphan.kind}` }, `${orphan.kind}: ${orphan.name}`),
      ),
    ),
  );
}

/** The full relation graph, collapsed by default as a secondary section. */
function RelationSection({ state }) {
  const flow = useMemo(() => toConfigFlow(state, null, () => {}), [state]);
  return h(
    "details",
    { className: "relation-section" },
    h("summary", {}, "Relation graph"),
    h(
      "div",
      { className: "config-flow", "aria-label": "Bureau config relation graph" },
      h(ReactFlow, {
        nodes: flow.nodes,
        edges: flow.edges,
        nodeTypes: configItemTypes,
        fitView: true,
        fitViewOptions: { padding: 0.18 },
        minZoom: 0.2,
        maxZoom: 1.5,
        nodesDraggable: false,
        nodesConnectable: false,
        elementsSelectable: true,
        proOptions: { hideAttribution: true },
      }, h(Background, { gap: 24, size: 1.5 }), h(Controls), h(MiniMap, { pannable: true, zoomable: true })),
    ),
  );
}

/** Same surface as the pipeline view: pan, zoom, fit, minimap. */
function toConfigFlow(state, expanded, onToggle) {
  const layout = state.config?.layout ?? { items: [], edges: [] };
  return {
    nodes: layout.items.map((item) => ({
      id: item.id,
      type: "configCard",
      position: { x: item.x, y: item.y },
      data: { state, item, expanded: expanded === item.id, onToggle },
      draggable: false,
      connectable: false,
      // An expanded card grows past its reserved box, so it must sit above its
      // neighbours rather than push them around.
      zIndex: expanded === item.id ? 10 : 0,
    })),
    edges: layout.edges.map((edge) => ({
      id: edge.id,
      source: edge.source,
      target: edge.target,
      type: "smoothstep",
      style: { stroke: "var(--border-color-default, #d0d7de)", strokeWidth: 1.4 },
    })),
  };
}

function ConfigCardNode({ data }) {
  return h(ConfigCard, { state: data.state, item: data.item, expanded: data.expanded, onToggle: data.onToggle });
}
function ConfigCard({ state, item, expanded, onToggle }) {
  const data = configData(state, item);
  const className = `card card--${item.kind}${item.orphan ? " card--orphan" : ""}${expanded ? " card--expanded" : ""}`;
  const deletable = ["repo", "role", "assignment", "pipeline"].includes(item.kind);
  return h(
    "article",
    // React Flow positions the node wrapper; the height comes from layout so
    // the rendered card can never exceed the box reserved for it. An expanded
    // card is the deliberate exception and overlays instead.
    { className, "data-ref": item.id, style: expanded ? {} : { height: item.height } },
    item.kind === "pipeline"
      ? h("button", { className: "card-button", type: "button", onClick: () => selectPipeline(item.name) }, configCardContent(state, item, data, expanded, onToggle))
      : h("div", {}, configCardContent(state, item, data, expanded, onToggle)),
    deletable ? h(DeleteControl, { dir: state.dir, kind: item.kind, name: item.name }) : null,
  );
}

function configCardContent(state, item, data, expanded, onToggle) {
  const detail = detailText(item, data);
  return [
    h("p", { className: "kind-label", key: "kind" }, item.kind.replace("-", " ")),
    h("h2", { key: "title" }, item.name),
    // The detail carries a whole shell command for an assignment, so it is the
    // affordance rather than a hidden tooltip: click to read it in full.
    h(
      "button",
      {
        className: `detail detail-toggle${expanded ? " detail-toggle--open" : ""}`,
        key: "detail",
        type: "button",
        title: detail,
        "aria-expanded": Boolean(expanded),
        onClick: (event) => {
          event.stopPropagation();
          onToggle?.(item.id);
        },
      },
      detail,
    ),
    h(Chips, { key: "chips", chips: chipsFor(state, item, data) }),
    item.kind === "pipeline" ? h(StepBadges, { key: "steps", state, name: item.name }) : null,
    h(Findings, { key: "findings", findings: state.findingsByItem?.[item.id] ?? [] }),
  ];
}

function configData(state, item) {
  const view = state.config?.view ?? emptyConfigView();
  const sources = { assignment: view.assignments, role: view.roles, repo: view.repos, pipeline: view.pipelines };
  if (item.kind === "work-source") {
    return view.assignments.find((assignment) => `work-source:${assignment.name}` === item.id) ?? {};
  }
  return (sources[item.kind] ?? []).find((value) => value.name === item.name) ?? {};
}

function detailText(item, data) {
  if (item.kind === "work-source") {
    return `${data.work?.forge ?? "unknown"} · ${data.work?.source ?? item.name}`;
  }
  if (item.kind === "assignment") {
    return `verify: ${data.verify ?? "not set"}`;
  }
  if (item.kind === "role") {
    return `${data.adapter ?? "adapter"} · min_trust: ${data.minTrust ?? "unknown"}`;
  }
  if (item.kind === "repo") {
    return data.access ?? "access unknown";
  }
  return `${data.stepCount ?? 0} steps`;
}

function chipsFor(state, item, data) {
  if (item.kind === "work-source") {
    return [data.work?.filter, data.work?.approvalLabel ? `approval: ${data.work.approvalLabel}` : null].filter(Boolean).map((text) => ({ text }));
  }
  if (item.kind === "assignment") {
    return Object.entries(data.limits ?? {}).filter(([, value]) => value != null).map(([name, value]) => ({ text: `${name}: ${value}` }));
  }
  if (item.kind === "role") {
    return (data.permissions ?? []).map((permission) => ({ text: permission, refs: data.usedBy ?? [], className: "permission-chip" }));
  }
  if (item.kind === "repo") {
    const primaries = (state.config?.view?.assignments ?? []).filter((assignment) => assignment.primaryRepo === data.name);
    return [{ text: data.access }, ...primaries.map((assignment) => ({ text: `primary: ${assignment.name}` }))];
  }
  const counts = state.pipelines?.[data.name]?.summary?.kindCounts ?? {};
  return Object.entries(counts).map(([kind, count]) => ({ text: `${kind}×${count}` }));
}

function Chips({ chips }) {
  if (!chips?.length) {
    return null;
  }
  return h("div", { className: "chips" }, chips.map((chip) => h(Chip, { key: `${chip.text}:${chip.refs?.join("|") ?? ""}`, chip })));
}

function Chip({ chip }) {
  return h("span", {
    className: `chip ${chip.className ?? ""}`.trim(),
    tabIndex: chip.refs ? 0 : undefined,
    onPointerEnter: chip.refs ? () => setHighlights(chip.refs) : undefined,
    onPointerLeave: chip.refs ? clearHighlights : undefined,
    onFocus: chip.refs ? () => setHighlights(chip.refs) : undefined,
    onBlur: chip.refs ? clearHighlights : undefined,
  }, chip.text);
}

function StepBadges({ state, name }) {
  const steps = state.pipelines?.[name]?.summary?.agentSteps ?? [];
  if (steps.length === 0) {
    return null;
  }
  return h("div", { className: "step-list" }, steps.map((step) => h("span", { key: step.ref, className: "step-badge", "data-ref": step.ref }, `${step.name} · ${step.role} · trust: ${step.trust ?? "role"}`)));
}

function PipelineView({ state, selectedStep, setSelectedStep }) {
  const name = state.selectedPipeline.name;
  const pipeline = state.pipelines?.[name];
  // graph-overlays: design keeps the static graph; live and replay restyle
  // it from run events via the shared reducer in web/live/overlay.js.
  const [mode, setMode] = useState("design");
  const live = useLiveOverlay();
  const replay = useReplayOverlay();
  const active = mode === "live" ? live : mode === "replay" ? replay : null;
  const flow = useMemo(
    () => toFlow(pipeline, state, selectedStep, active?.decoration ?? null),
    [pipeline, state, selectedStep, active?.decoration],
  );
  return h(
    "section",
    { className: "view-shell view-shell--pipeline" },
    h(
      "section",
      { className: "pipeline-main" },
      h(
        "div",
        { className: "pipeline-toolbar" },
        h("button", { className: "back-button", type: "button", onClick: backToConfig }, "Back to config"),
        h("h2", {}, name),
        h(ModeSwitcher, { mode, onMode: setMode }),
        h("a", { className: "editor-link", href: `./editor.html?pipeline=${encodeURIComponent(name)}` }, "Edit"),
        active?.controls ?? null,
      ),
      h(
        "div",
        { className: "pipeline-flow" },
        h(ReactFlow, {
          nodes: flow.nodes,
          edges: flow.edges,
          nodeTypes: flowItemTypes,
          edgeTypes: flowEdgeTypes,
          fitView: true,
          fitViewOptions: { padding: 0.22 },
          minZoom: 0.2,
          maxZoom: 1.5,
          nodesDraggable: false,
          nodesConnectable: false,
          elementsSelectable: true,
          proOptions: { hideAttribution: true },
          onNodeClick: (_, item) => item.type === "stepCard" && setSelectedStep(item.data.step.id),
        }, h(Background, { gap: 24, size: 1.5 }), h(Controls), h(MiniMap, { pannable: true, zoomable: true })),
      ),
    ),
    h(SidePanel, { state, pipeline, name }),
  );
}

function toFlow(pipeline, state, selectedStep, decoration = null) {
  const layout = pipeline?.layout ?? { steps: [], terminals: [], edges: [] };
  const handles = pipeline?.handles ?? { items: {}, edges: {} };
  // graph-overlays: live/replay restyle the static layout; hidden members
  // collapse into their group node and their edges remap onto it.
  const resolved = decoration ? resolveOverlay(pipeline, decoration.overlay, decoration) : null;
  const visible = new Set((resolved?.nodes ?? layout.steps).map((node) => node.id));
  const frames = (pipeline?.containers ?? []).map((frame) => flowFrame(frame));
  const steps = layout.steps
    .filter((step) => visible.has(step.id))
    .map((step) => flowStep(step, state, layout.name, handles.items[step.id], selectedStep, resolved));
  const terminals = layout.terminals.map((terminal) => flowTerminal(terminal, handles.items[terminal.id]));
  const backIndexes = routeIndexes(layout.edges, "back");
  return {
    nodes: [...frames, ...steps, ...terminals],
    edges: overlayEdges(layout.edges, handles, backIndexes, resolved),
  };
}

/** Remap hidden-member edges onto their group node and drop the duplicates. */
function overlayEdges(edges, handles, backIndexes, resolved) {
  const seen = new Set();
  const drawn = [];
  for (const edge of edges) {
    const remapped = resolved ? { ...edge, source: resolved.remapEdge(edge.source), target: resolved.remapEdge(edge.target) } : edge;
    const key = `${remapped.source}->${remapped.target}:${remapped.outcome ?? remapped.relation}`;
    if (remapped.source === remapped.target || seen.has(key)) {
      continue;
    }
    seen.add(key);
    drawn.push(flowEdge(remapped, handles.edges[edge.id], backIndexes.get(edge.id) ?? 0, resolved, edge.id));
  }
  return drawn;
}

function flowFrame(frame) {
  return {
    id: frame.id,
    type: "concurrentFrame",
    position: { x: frame.x - FRAME_PAD, y: frame.y - FRAME_PAD },
    data: { frame },
    style: { width: frame.width + CARD_WIDTH + FRAME_PAD * 2, height: frame.height + CARD_HEIGHT + FRAME_PAD * 2 },
    selectable: false,
    draggable: false,
    zIndex: -1,
  };
}

function flowStep(step, state, pipelineName, handles, selectedStep, resolved) {
  const ref = `pipeline:${pipelineName}/${step.name}`;
  const node = resolved?.nodes.find((item) => item.id === step.id) ?? null;
  return {
    id: step.id,
    type: "stepCard",
    position: { x: step.x, y: step.y },
    data: {
      step,
      handles: handles ?? emptyHandles(),
      findings: state.findingsByStep?.[ref] ?? [],
      selected: selectedStep === step.name,
      overlayClass: node?.className ?? "",
      paused: Boolean(node?.paused),
      expanded: resolved?.expandedGroups.has(step.name) ?? false,
      members: memberRows(resolved, step),
      onToggleGroup: resolved?.onToggleGroup ?? null,
    },
    style: { width: CARD_WIDTH },
    draggable: false,
  };
}

/** Expanded groups surface one outcome row per member on the group card. */
function memberRows(resolved, step) {
  if (!resolved || !resolved.expandedGroups.has(step.name)) {
    return null;
  }
  const members = resolved.overlayGroups[step.name]?.members ?? {};
  return Object.entries(members).map(([name, record]) => ({ name, ...record }));
}

function flowTerminal(terminal, handles) {
  return {
    id: terminal.id,
    type: "terminalPill",
    position: { x: terminal.x, y: terminal.y + 26 },
    data: { terminal, handles: handles ?? emptyHandles() },
    style: { width: 136 },
    draggable: false,
  };
}

function flowEdge(edge, endpoints, backIndex, resolved, originalId) {
  const key = edge.relation === "control" ? edge.outcome : edge.relation;
  const animated = resolved?.animatedEdges.has(originalId ?? edge.id) ?? false;
  return {
    id: resolved ? `overlay:${originalId}` : edge.id,
    source: edge.source,
    target: edge.target,
    sourceHandle: endpoints?.source,
    targetHandle: endpoints?.target,
    type: "routed",
    className: `flow-edge--${key}${animated ? " flow-edge--live" : ""}`,
    animated,
    markerEnd: { type: MarkerType.ArrowClosed, color: edgeColors[key] ?? edgeColors.success },
    data: {
      label: edgeLabelText(edge),
      offset: edge.route === "back" ? 26 + backIndex * 12 : 12,
      captionShiftY: edgeCaptionShiftY(edge),
      route: edge.route,
    },
  };
}

function edgeLabelText(edge) {
  if (edge.relation === "control") {
    return edge.outcome;
  }
  return edge.relation === "observes" ? "over" : undefined;
}

function edgeCaptionShiftY(edge) {
  if (edge.relation !== "control") {
    return 0;
  }
  return { success: -18, failure: 0, blocked: 18, "no-work": 36 }[edge.outcome] ?? 0;
}

function routeIndexes(edges, route) {
  const counts = new Map();
  const indexes = new Map();
  for (const edge of edges.filter((edge) => edge.route === route)) {
    const key = edge.target;
    const index = counts.get(key) ?? 0;
    counts.set(key, index + 1);
    indexes.set(edge.id, index);
  }
  return indexes;
}

function RoutedEdge({ id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, data = {} }) {
  const [path, labelX, labelY] = getSmoothStepPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    offset: data.offset ?? 12,
  });
  const [captionX, captionY] = edgeCaptionPosition({ data, labelX, labelY, sourceX, sourceY, targetX });
  return h(React.Fragment, null,
    h(BaseEdge, { id, path, markerEnd }),
    data.label ? h(EdgeLabelRenderer, null, h("div", {
      className: "react-flow__edge-label edge-caption",
      style: { transform: `translate(-50%, -50%) translate(${captionX}px, ${captionY}px)` },
    }, data.label)) : null,
  );
}

function edgeCaptionPosition({ data, labelX, labelY, sourceX, sourceY, targetX }) {
  if (data.route === "exit") {
    const direction = Math.sign(targetX - sourceX) || 1;
    const distance = Math.min(180, Math.max(96, Math.abs(targetX - sourceX) * 0.4));
    return [sourceX + direction * distance, sourceY - 10];
  }
  return [labelX, labelY + (data.captionShiftY ?? 0)];
}

function StepCard({ data }) {
  const step = data.step;
  const className = [
    "flow-card",
    `flow-card--${step.kind}`,
    step.parentId ? "flow-card--member" : "",
    data.selected ? "is-highlighted" : "",
    data.overlayClass ?? "",
    data.paused ? "overlay-paused" : "",
    unreachableClass(data.findings),
  ].filter(Boolean).join(" ");
  return h(
    "article",
    { className },
    h(Handles, { handles: data.handles }),
    h("button", { className: "step-button", type: "button" },
      h("p", { className: "kind-label" }, step.kind),
      h("h2", {}, step.name, data.paused ? h("span", { className: "paused-badge" }, "paused") : null),
      h("p", { className: "detail", title: stepDetail(step) }, stepDetail(step)),
      h(Chips, { chips: stepChips(step) }),
      data.expanded ? h(MemberList, { members: data.members ?? [], group: step.name, onToggleGroup: data.onToggleGroup }) : null,
      h(Findings, { findings: data.findings }),
    ),
  );
}

/** Expanded groups list member outcomes on the group card itself. */
function MemberList({ members, group, onToggleGroup }) {
  return h(
    "div",
    { className: "member-list" },
    h(
      "button",
      { className: "member-collapse", type: "button", onClick: (event) => { event.stopPropagation(); onToggleGroup?.(group); } },
      "collapse",
    ),
    h(
      "ul",
      {},
      members.map((member) =>
        h("li", { key: member.name, className: `member-row member-row--${member.outcome ?? member.state}` },
          h("span", { className: "member-name" }, member.name),
          h("span", { className: "member-outcome" }, member.outcome ?? member.state),
        ),
      ),
    ),
  );
}

function TerminalPill({ data }) {
  return h("article", { className: "flow-card terminal-pill" }, h(Handles, { handles: data.handles }), h("h2", {}, data.terminal.name));
}

function ConcurrentFrame() {
  return h("div", { className: "concurrent-frame" });
}

function Handles({ handles }) {
  return [
    ...(handles.target ?? []).map((handle, index, list) => handleElement(handle, "target", index, list)),
    ...(handles.source ?? []).map((handle, index, list) => handleElement(handle, "source", index, list)),
  ];
}

function handleElement(handle, type, index, list) {
  return h(Handle, {
    key: `${type}:${handle.id}`,
    id: handle.id,
    type,
    position: handlePosition(handle.side),
    isConnectable: false,
    className: `flow-handle flow-handle--${handle.name}`,
    style: handleStyle(handle.side, index, list),
  });
}

function handlePosition(side) {
  return { top: Position.Top, right: Position.Right, bottom: Position.Bottom, left: Position.Left }[side];
}

function handleStyle(side, index, list) {
  const percent = list.length <= 1 ? 50 : 22 + (index * 56) / (list.length - 1);
  if (side === "left" || side === "right") {
    return { top: `${percent}%` };
  }
  return { left: `${percent}%` };
}

function emptyHandles() {
  return { source: [], target: [] };
}

function stepDetail(step) {
  if (step.kind === "deterministic") {
    return step.fields.run ?? "run command not set";
  }
  if (step.kind === "agent") {
    return `role: ${step.fields.role ?? "not set"}`;
  }
  if (step.kind === "decision") {
    return `over: ${step.fields.over ?? "not set"}`;
  }
  return `${step.fields.members?.length ?? 0} in parallel`;
}

function stepChips(step) {
  return [
    step.parentId ? { text: `member of ${step.parentId}` } : null,
    step.fields.trust ? { text: `trust: ${step.fields.trust}` } : null,
    step.fields.maxAttempts > 1 ? { text: `attempts: ${step.fields.maxAttempts}` } : null,
  ].filter(Boolean);
}

function unreachableClass(findings) {
  return findings.some((finding) => /unreachable/i.test(finding.message ?? "")) ? "flow-card--unreachable" : "";
}

function SidePanel({ state, pipeline, name }) {
  const findings = pipelineFindings(state, name);
  return h(
    "aside",
    { className: "side-panel" },
    h("section", { className: "panel-section" }, h("h2", {}, name), h("p", { className: "muted" }, pipelineCounts(pipeline))),
    h("section", { className: "panel-section" }, h("h3", {}, `Validation (${findings.length})`), findings.length ? h(Findings, { findings }) : h("p", { className: "muted" }, "clean — bureau validate would pass")),
    h("section", { className: "panel-section" }, h("h3", {}, "Legend"), h(Legend)),
    h("section", { className: "panel-section" }, h("h3", {}, "Trust flow"), h("p", { className: "muted" }, "Reserved for trust analysis.")),
  );
}

function pipelineCounts(pipeline) {
  const steps = pipeline?.layout?.steps?.length ?? 0;
  const terminals = pipeline?.layout?.terminals?.length ?? 0;
  const edges = pipeline?.layout?.edges?.length ?? 0;
  return `${steps} steps · ${terminals} terminals · ${edges} edges`;
}

function pipelineFindings(state, name) {
  return (state.findings ?? []).filter((finding) => {
    const target = finding.target ?? {};
    return target.pipeline === name || target.kind === "pipeline" && target.pipeline === name;
  });
}

function Legend() {
  return h("div", { className: "legend" }, [
    legendItem("success", "var(--outcome-success)"),
    legendItem("failure", "var(--outcome-failure)"),
    legendItem("blocked", "var(--outcome-blocked)"),
    legendItem("no-work", "var(--outcome-no-work)"),
    legendItem("inputs_from", "var(--relation-data)", "legend-swatch--data"),
    legendItem("over", "var(--relation-observes)", "legend-swatch--observes"),
  ]);
}

function legendItem(text, color, className = "") {
  return h("span", { className: "legend-item", key: text }, h("span", { className: `legend-swatch ${className}`.trim(), style: { "--swatch": color } }), ` ${text}`);
}

function Findings({ findings, className = "findings" }) {
  if (!findings?.length) {
    return null;
  }
  return h("div", { className }, findings.map((finding, index) => h("span", { className: findingClass(finding), key: `${finding.message}:${index}` }, finding.message)));
}

function findingClass(finding) {
  const advisory = finding.marker === "advisory" || finding.source === "advisory";
  return `finding ${advisory ? "finding--advisory" : "finding--validation"}`;
}

function setHighlights(refs) {
  document.body.classList.add("has-highlight");
  for (const element of document.querySelectorAll("[data-ref]")) {
    element.classList.toggle("is-highlighted", refs.includes(element.dataset.ref));
  }
}

function clearHighlights() {
  document.body.classList.remove("has-highlight");
  for (const element of document.querySelectorAll(".is-highlighted")) {
    element.classList.remove("is-highlighted");
  }
}

function selectPipeline(name) {
  postIntent({ kind: "open-pipeline", pipeline: name }).then(publishLocalState);
}

function backToConfig() {
  postIntent({ kind: "back-to-config" }).then(publishLocalState);
}

function publishLocalState(result) {
  if (result?.ok) {
    window.dispatchEvent(new CustomEvent("bureau-state", { detail: result.state }));
  }
}

function postIntent(body) {
  return fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  }).then((response) => response.ok ? response.json() : null);
}