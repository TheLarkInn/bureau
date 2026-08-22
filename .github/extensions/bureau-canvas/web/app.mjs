import React, { useEffect, useMemo, useRef, useState } from "react";
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
import { terminalCopy } from "./terminals.js";
import { RelationGraph } from "./editor/relation.mjs";
import { nextExpandedAssignment } from "./assignment-state.js";

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
      h("p", { className: "config-path", title: state.validation?.dir ?? state.dir }, state.validation?.dir ?? state.dir),
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
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  if (!plan) {
    return null;
  }
  const pending = plan.writes.length + plan.removals.length;
  const act = (kind) => {
    setBusy(true);
    setError(null);
    postIntent({ kind }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
      } else {
        setError(result?.error ?? `could not ${kind === "save-plan" ? "save" : "discard"} changes`);
      }
    });
  };
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
      h("button", { type: "button", className: "btn btn--small btn--primary", "data-testid": "draft-save", disabled: busy, onClick: () => act("save-plan") }, busy ? "Working…" : "Save"),
      h("button", { type: "button", className: "btn btn--small", "data-testid": "draft-discard", disabled: busy, onClick: () => act("discard-plan") }, "Discard"),
    ),
    error ? h("p", { className: "note note--err", role: "alert" }, error) : null,
  );
}

function shortPath(path) {
  return String(path).replaceAll("\\", "/").split("/").slice(-2).join("/");
}

/** A quiet global create affordance; the form appears only when requested. */
function CreateBar({ dir }) {
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState("pipeline");
  const [name, setName] = useState("");
  const [error, setError] = useState(null);
  const trigger = useRef(null);
  const close = () => closeDisclosure(setOpen, trigger);
  const submit = (event) => {
    event.preventDefault();
    if (!name.trim()) {
      return;
    }
    setError(null);
    postIntent({ kind: "create", input: { dir, kind, name: name.trim(), fields: {} } }).then((result) => {
      if (!result?.ok) {
        setError(result?.error ?? `could not create ${kind}`);
        return;
      }
      setName("");
      close();
      publishLocalState(result);
    });
  };
  if (!open) {
    return h(
      "div",
      { className: "create-toolbar" },
      h("button", {
        type: "button",
        ref: trigger,
        className: "btn btn--primary",
        "data-testid": "create-open",
        onClick: () => setOpen(true),
      }, "+ New pipeline or role"),
    );
  }
  return h(
    "form",
    {
      className: "create-bar",
      onSubmit: submit,
      onKeyDown: (event) => {
        if (event.key === "Escape") {
          setName("");
          close();
        }
      },
      "data-testid": "create-bar",
    },
    h(
      "div",
      { className: "create-bar__header" },
      h("span", { className: "detail-label" }, "New reusable config"),
      h("button", {
        type: "button",
        className: "icon-btn",
        "aria-label": "Close create form",
        onClick: () => {
          setName("");
          close();
        },
      }, "×"),
    ),
    h(
      "div",
      { className: "create-bar__fields" },
      h("label", { className: "detail-label", htmlFor: "create-kind" }, "Kind"),
      h(
        "select",
        {
          id: "create-kind",
          className: "form-control form-select",
          value: kind,
          onChange: (event) => setKind(event.target.value),
        },
        ["pipeline", "role"].map((option) =>
          h("option", { key: option, value: option }, option)),
      ),
      error ? h("p", { className: "note note--err", role: "alert" }, error) : null,
      h("label", { className: "detail-label", htmlFor: "create-name" }, "Name"),
      h("input", {
        id: "create-name",
        className: "form-control",
        value: name,
        onChange: (event) => setName(event.target.value),
        placeholder: `${kind} name`,
        autoFocus: true,
      }),
    ),
    h(
      "div",
      { className: "actions" },
      h("button", {
        type: "submit",
        className: "btn btn--primary",
        "data-testid": "create-submit",
        disabled: !name.trim(),
      }, `Create ${kind}`),
      h("button", {
        type: "button",
        className: "btn",
        "data-testid": "create-cancel",
        onClick: () => {
          setName("");
          close();
        },
      }, "Cancel"),
    ),
  );
}

/** Delete asks first and shows what breaks; the entry-step case reads louder. */
function DeleteControl({ dir, kind, name }) {
  const [preflight, setPreflight] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const ask = () => {
    setBusy(true);
    setError(null);
    postIntent({ kind: "delete", input: { dir, kind, name } }).then((response) => {
      setBusy(false);
      if (response?.ok) {
        setPreflight(response.result);
      } else {
        setError(response?.error ?? `could not inspect ${name}`);
      }
    });
  };
  const confirm = () => postIntent({ kind: "delete", input: { dir, kind, name, confirm: true } }).then((result) => {
    if (result?.ok) {
      setPreflight(null);
      publishLocalState(result);
    } else {
      setError(result?.error ?? `could not delete ${name}`);
    }
  });
  if (!preflight) {
    return h(React.Fragment, null,
      h("button", { type: "button", className: "btn btn--small btn--danger card-action", "data-testid": "delete-start", disabled: busy, onClick: ask }, busy ? "Checking…" : "Delete"),
      error ? h("p", { className: "note note--err", role: "alert" }, error) : null);
  }
  return h(
    "div",
    { className: `preflight${preflight.referrers?.length ? " preflight--blocking" : ""}`, "data-testid": "preflight" },
    h("p", {}, preflight.referrers?.length ? `${preflight.referrers.length} reference${preflight.referrers.length === 1 ? "" : "s"}` : "Nothing references this"),
    h("ul", {}, (preflight.referrers ?? []).map((item) => h("li", { key: item.name, className: `severity-${item.severity}` }, item.message))),
    preflight.blocking
      ? h("p", { className: "note note--err" }, "Repoint these references before deleting this item.")
      : null,
    h("div", { className: "actions" },
      h("button", { type: "button", className: "btn btn--small btn--danger", "data-testid": "delete-confirm", disabled: preflight.blocking, onClick: confirm }, "Confirm delete"),
      h("button", { type: "button", className: "btn btn--small", onClick: () => setPreflight(null) }, "Cancel")),
    error ? h("p", { className: "note note--err", role: "alert" }, error) : null,
  );
}

function emptyConfigView() {
  return { assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
}

function summaryText(view) {
  return [
    countLabel(view.assignments.length, "assignment"),
    countLabel(view.roles.length, "role"),
    countLabel(view.repos.length, "repo"),
    countLabel(view.pipelines.length, "pipeline"),
  ].join(" · ");
}

function countLabel(count, noun) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function ConfigView({ state }) {
  const view = state.config?.view ?? emptyConfigView();
  return h(
    "section",
    { className: "view-shell view-shell--config" },
    h(
      "div",
      { className: "config-heading-row" },
      h("h2", { className: "config-heading" }, "Assignments"),
      h(CreateBar, { dir: state.dir }),
    ),
    h(AssignmentStack, { state, view }),
    h(OrphanStrip, { state, view }),
    h(RelationSection, { state }),
  );
}

/** The landing: assignments as a vertical stack, each expanding in place. */
function AssignmentStack({ state, view }) {
  const [expanded, setExpanded] = useState(() => sessionStorage.getItem("bureau.expanded-assignment"));
  useEffect(() => {
    if (expanded) {
      sessionStorage.setItem("bureau.expanded-assignment", expanded);
    } else {
      sessionStorage.removeItem("bureau.expanded-assignment");
    }
  }, [expanded]);
  const toggle = (name) => setExpanded((current) =>
    nextExpandedAssignment(current, name, confirmClosingEditor));
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
      h("span", { className: "assignment-caret", "aria-hidden": "true" }, "›"),
    ),
    expanded ? h(AssignmentDetail, { state, assignment }) : null,
    findings.length ? h(Findings, { findings }) : null,
  );
}

/** The one-line at-a-glance summary shown on a collapsed card. */
function assignmentGlance(assignment) {
  const repos = (assignment.repos ?? []).length;
  const limits = Object.values(assignment.limits ?? {}).filter((value) => value != null).length;
  return `${assignment.work?.source ?? "no source"} · ${assignment.pipeline ?? "no pipeline"} · ${repos} repo${repos === 1 ? "" : "s"} · ${limits} limit${limits === 1 ? "" : "s"}`;
}

/** The expanded body: work source, repos, pipeline and limits. */
function AssignmentDetail({ state, assignment }) {
  return h(
    "div",
    { className: "assignment-detail" },
    h(DetailRow, { label: "work source" }, h(WorkSourceField, { assignment })),
    h(DetailRow, { label: "work rules" }, h(AssignmentRuntimeField, { assignment })),
    h(DetailRow, { label: "forge signals" }, h(TerminalLabelsField, { assignment })),
    h(DetailRow, { label: "repos" }, h(ReposField, { state, assignment })),
    h(DetailRow, { label: "pipeline" }, h(PipelineLink, { state, name: assignment.pipeline })),
    h(DetailRow, { label: "limits" }, h(LimitsField, { assignment })),
    h("div", { className: "assignment-actions" },
      h(DeleteControl, { dir: state.dir, kind: "assignment", name: assignment.name })),
  );
}

function DetailRow({ label, children }) {
  return h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, label), h("span", { className: "detail-value" }, children));
}

function RefLink({ kind, name }) {
  return h("span", { className: `ref ref--${kind}` }, name ?? "—");
}

function PipelineLink({ state, name }) {
  if (!name) {
    return h("span", { className: "muted" }, "—");
  }
  const summary = state.pipelines?.[name]?.summary ?? {};
  const counts = summary.kindCounts ?? {};
  const total = Object.values(counts).reduce((sum, count) => sum + count, 0);
  const roles = summary.agentSteps?.map((step) => step.role).filter(Boolean) ?? [];
  return h(
    "button",
    {
      type: "button",
      className: "pipeline-ref",
      "aria-label": `Open pipeline ${name}`,
      onClick: () => confirmClosingEditor() && selectPipeline(name),
    },
    h("span", { className: "pipeline-ref__name" }, name),
    h(
      "span",
      { className: "pipeline-ref__summary" },
      `${total} step${total === 1 ? "" : "s"}`,
      roles.length ? ` · ${[...new Set(roles)].join(" · ")}` : "",
    ),
  );
}

function confirmClosingEditor() {
  const editor = document.querySelector(".ws-open, .repos-editor, .limits-editor, .assignment-runtime-editor, .terminal-label-editor");
  return !editor || window.confirm("Discard the unsaved field changes?");
}

function runtimeFields(assignment, changes = {}) {
  return {
    filter: assignment.work?.filter ?? "",
    approval_label: assignment.work?.approvalLabel ?? null,
    abort_label: assignment.work?.abortLabel ?? "",
    escalate_label: assignment.work?.escalateLabel ?? "",
    branch_prefix: assignment.branchPrefix ?? "",
    ...changes,
  };
}

function AssignmentRuntimeField({ assignment }) {
  const [editing, setEditing] = useState(false);
  const trigger = useRef(null);
  const close = () => closeDisclosure(setEditing, trigger);
  return h(
    "div",
    { className: "field-disclosure" },
    h(
      "button",
      {
        ref: trigger,
        type: "button",
        className: "runtime-value",
        "aria-expanded": editing,
        title: "Change the work filter, approval label, or branch prefix",
        onClick: () => setEditing((current) => !current),
      },
      h("span", { className: "chips" },
        h("span", { className: "chip" }, assignment.work?.filter ?? "no filter"),
        h("span", { className: "chip" }, assignment.work?.approvalLabel ? `approval: ${assignment.work.approvalLabel}` : "no approval label"),
        h("span", { className: "chip" }, `branches: ${assignment.branchPrefix ?? "not set"}`)),
    ),
    editing ? h(AssignmentRuntimeEditor, { assignment, onDone: close }) : null,
  );
}

function AssignmentRuntimeEditor({ assignment, onDone }) {
  const initial = {
    filter: assignment.work?.filter ?? "",
    approval_label: assignment.work?.approvalLabel ?? "",
    branch_prefix: assignment.branchPrefix ?? "",
  };
  const [fields, setFields] = useState(initial);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const changed = Object.keys(initial).some((key) => fields[key] !== initial[key]);
  const invalid = !fields.filter.trim() || !fields.branch_prefix.trim();
  const set = (field, value) => setFields((current) => ({ ...current, [field]: value }));
  const save = () => {
    setBusy(true);
    setError(null);
    const input = { assignment: assignment.name, fields: runtimeFields(assignment, {
      filter: fields.filter.trim(),
      approval_label: fields.approval_label.trim() || null,
      branch_prefix: fields.branch_prefix.trim(),
    }) };
    postIntent({ kind: "set-assignment-runtime", input }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
        onDone();
      } else {
        setError(result?.error ?? "could not save those work rules");
      }
    });
  };
  return h(
    "div",
    {
      className: "assignment-runtime-editor",
      onKeyDown: (event) => event.key === "Escape" && onDone(),
    },
    h("label", {}, "Work-item filter", h("input", {
      className: `form-control form-control--mono${fields.filter.trim() ? "" : " form-control--invalid"}`,
      "data-testid": "wr-filter",
      value: fields.filter,
      onChange: (event) => set("filter", event.target.value),
    })),
    h("label", {}, "Approval label (optional)", h("input", {
      className: "form-control form-control--mono",
      "data-testid": "wr-approval",
      value: fields.approval_label,
      onChange: (event) => set("approval_label", event.target.value),
    })),
    h("label", {}, "Branch prefix", h("input", {
      className: `form-control form-control--mono${fields.branch_prefix.trim() ? "" : " form-control--invalid"}`,
      "data-testid": "wr-branch",
      value: fields.branch_prefix,
      onChange: (event) => set("branch_prefix", event.target.value),
    })),
    invalid ? h("p", { className: "note note--err" }, "Filter and branch prefix cannot be empty.") : null,
    error ? h("p", { className: "note note--err", role: "alert" }, error) : null,
    h("div", { className: "actions" },
      h("button", { type: "button", className: "btn btn--primary", "data-testid": "work-rules-save", disabled: busy || invalid || !changed, onClick: save }, busy ? "Saving…" : "Save work rules"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel")),
  );
}

/**
 * The forge signals value, clickable. Like every other field on the card, the
 * value stays on screen and the editor discloses beneath it — swapping the
 * value out was this field's alone, and it made the row jump.
 */
function TerminalLabelsField({ assignment }) {
  const [editing, setEditing] = useState(false);
  const trigger = useRef(null);
  const close = () => closeDisclosure(setEditing, trigger);
  return h(
    "div",
    { className: "field-disclosure" },
    h(
      "button",
      {
        ref: trigger,
        type: "button",
        className: "terminal-label-value",
        "aria-expanded": editing,
        title: "Change the labels Bureau applies at terminal states",
        onClick: () => setEditing((current) => !current),
      },
      h(TerminalSignal, { kind: "abort", label: assignment.work?.abortLabel }),
      h(TerminalSignal, { kind: "escalate", label: assignment.work?.escalateLabel }),
    ),
    editing ? h(TerminalLabelsEditor, { assignment, onDone: close }) : null,
  );
}

function TerminalSignal({ kind, label }) {
  const copy = terminalCopy(kind);
  return h(
    "span",
    { className: `terminal-signal terminal-signal--${kind}` },
    h("span", { className: "terminal-signal__name" }, copy.label),
    h("code", {}, label || "not configured"),
  );
}

function TerminalLabelsEditor({ assignment, onDone }) {
  const initial = {
    abort_label: assignment.work?.abortLabel ?? "",
    escalate_label: assignment.work?.escalateLabel ?? "",
  };
  const [fields, setFields] = useState(initial);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);
  const changed = Object.keys(initial).some((key) => fields[key] !== initial[key]);
  const invalid = !fields.abort_label.trim() || !fields.escalate_label.trim()
    || fields.abort_label.trim() === fields.escalate_label.trim();
  const set = (field, value) => setFields((current) => ({ ...current, [field]: value }));
  const save = () => {
    setBusy(true);
    setError(null);
    const labels = {
      abort_label: fields.abort_label.trim(),
      escalate_label: fields.escalate_label.trim(),
    };
    const input = { assignment: assignment.name, fields: runtimeFields(assignment, labels) };
    postIntent({ kind: "set-assignment-runtime", input }).then((result) => {
      setBusy(false);
      if (result?.ok) {
        publishLocalState(result);
        onDone();
      } else {
        setError(result?.error ?? "could not save those forge signals");
      }
    });
  };
  return h(
    "div",
    { className: "terminal-label-editor", onKeyDown: (event) => event.key === "Escape" && onDone() },
    h("label", {}, "Failed run label", h("input", {
      className: "form-control form-control--mono",
      "data-testid": "sig-abort",
      value: fields.abort_label,
      onChange: (event) => set("abort_label", event.target.value),
    })),
    h("label", {}, "Needs-human label", h("input", {
      className: "form-control form-control--mono",
      "data-testid": "sig-escalate",
      value: fields.escalate_label,
      onChange: (event) => set("escalate_label", event.target.value),
    })),
    h("p", { className: "note" }, "Bureau preserves unrelated work-item labels."),
    invalid ? h("p", { className: "note note--err" }, "Both labels are required and must differ.") : null,
    error ? h("p", { className: "note note--err", role: "alert" }, error) : null,
    h("div", { className: "actions" },
      h("button", { type: "button", className: "btn btn--primary", "data-testid": "signals-save", disabled: busy || invalid || !changed, onClick: save }, busy ? "Saving…" : "Save forge signals"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel")),
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
  const trigger = useRef(null);
  const close = () => closeDisclosure(setEditing, trigger);
  const repos = assignment.repos ?? [];
  return h(
    "div",
    { className: "field-disclosure" },
    h(
      "button",
      {
        ref: trigger,
        type: "button", className: "repos-value", "aria-expanded": editing,
        title: "Change the repos this assignment touches",
        onClick: () => setEditing((current) => !current),
      },
      repos.length
        ? h("span", { className: "chips" }, repos.map((name, index) =>
            h("span", { key: name, className: `chip repo-chip${index === 0 ? " repo-chip--primary" : ""}` },
              index === 0 ? `${name} · primary` : name)))
        : h("span", { className: "muted" }, "no repos"),
    ),
    editing ? h(ReposEditor, { state, assignment, onDone: close }) : null,
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
    { className: "repos-editor", onKeyDown: (event) => event.key === "Escape" && onDone() },
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
      : repos.length > 1
        ? h("p", { className: "note" }, "Rows below the first supply read-only context to the run.")
        : null,
    error ? h("p", { className: "note note--err" }, error) : null,
    h("div", { className: "actions" },
      h("button", {
        type: "button", className: "btn btn--primary", "data-testid": "repos-save",
        disabled: busy || repos.length === 0 || Boolean(problem) || sameOrder(repos, assignment.repos ?? []),
        onClick: () => commit(repos),
      }, busy ? "Saving…" : "Save repos"),
      h("button", { type: "button", className: "btn", "data-testid": "repos-add", onClick: () => setAdding(true) }, "+ Add repo"),
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
      type: "text", className: "form-control form-control--mono", value: url, autoFocus: true, "aria-label": "Repository URL",
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
      h("input", { id: "repo-name", type: "text", className: "form-control form-control--mono", value: name, onChange: (event) => setName(event.target.value) })),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "forge"), h("code", {}, resolved.forge)),
    h("div", { className: "detail-row" }, h("span", { className: "detail-label" }, "url"), h("code", {}, resolved.url)),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "repo-access" }, "access"),
      h("select", { id: "repo-access", className: "form-control form-select", value: access, onChange: (event) => setAccess(event.target.value) },
        ACCESS_LEVELS.map((value) => h("option", { key: value, value }, value)))),
    h("div", { className: "detail-row" },
      h("label", { className: "detail-label", htmlFor: "repo-credential" }, "credential"),
      credentials.length
        ? h("select", { id: "repo-credential", className: "form-control form-select", value: credential, onChange: (event) => setCredential(event.target.value) },
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
  { key: "max_concurrent", view: "maxConcurrent", unit: "runs at once", noun: "Concurrent runs", short: "at once", integer: true, max: 4_294_967_295 },
  { key: "max_runs_per_hour", view: "maxRunsPerHour", unit: "runs / hour", noun: "Runs per hour", short: "/hour", integer: true, max: 4_294_967_295 },
  { key: "max_runs_per_day", view: "maxRunsPerDay", unit: "runs / day", noun: "Runs per day", short: "/day", integer: true, max: 4_294_967_295 },
  { key: "max_open_prs", view: "maxOpenPrs", unit: "open PRs", noun: "Open pull requests", short: "open PRs", integer: true, max: 4_294_967_295 },
  { key: "max_cost_per_day_usd", view: "maxCostPerDayUsd", unit: "USD / day", noun: "Daily model cost", short: "USD/day", integer: false },
  { key: "max_run_hours", view: "maxRunHours", unit: "hours / run", noun: "Run deadline", short: "h/run", defaulted: 24, integer: true, max: Number.MAX_SAFE_INTEGER },
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
  const trigger = useRef(null);
  const close = () => closeDisclosure(setEditing, trigger);
  return h(
    "div",
    { className: "field-disclosure" },
    h(
      "button",
      {
        ref: trigger,
        type: "button", className: "limits-value", "aria-expanded": editing,
        title: "Change the limits on this assignment",
        onClick: () => setEditing((current) => !current),
      },
      h(LimitsSummary, { limits }),
    ),
    editing ? h(LimitsEditor, { assignment, saved: limits, onDone: close }) : null,
  );
}

function LimitsEditor({ assignment, saved, onDone }) {
  const [draft, setDraft] = useState(saved);
  const [lastValues, setLastValues] = useState(saved);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const changed = LIMIT_FIELDS.some((field) => draft[field.key] !== saved[field.key]);
  // A cleared box is kept as its raw text rather than coerced: `Number("")`
  // is 0, and a zero limit computes headroom as permanently zero — a total
  // block produced by one backspace.
  const invalid = LIMIT_FIELDS.some((field) => isSet(draft, field.key) && !validLimit(field, draft[field.key]));
  const set = (key, value) => setDraft((current) => ({ ...current, [key]: value }));
  const toggle = (field) => {
    if (isSet(draft, field.key)) {
      setLastValues((current) => ({ ...current, [field.key]: draft[field.key] }));
      set(field.key, null);
    } else {
      set(field.key, lastValues[field.key] ?? defaultLimit(field));
    }
  };
  const change = (field, value) => {
    set(field.key, value);
    if (validLimit(field, value)) {
      setLastValues((current) => ({ ...current, [field.key]: value }));
    }
  };

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
    { className: "limits-editor", onKeyDown: (event) => event.key === "Escape" && onDone() },
    LIMIT_FIELDS.map((field) => h(LimitRow, {
      key: field.key, field, value: draft[field.key],
      onToggle: () => toggle(field),
      onChange: (value) => change(field, value),
    })),
    h("p", { className: "note" }, "Off means no ceiling at all — except run length, which falls back to the system default of 24 hours."),
    invalid ? h("p", { className: "note note--err" }, "Enabled count and deadline limits need whole numbers of at least 1. Daily model cost accepts a positive decimal. Switch a limit off for unlimited.") : null,
    error ? h("p", { className: "note note--err" }, error) : null,
    h("div", { className: "actions" },
      h("button", { type: "button", className: "btn btn--primary", "data-testid": "limits-save", disabled: busy || !changed || invalid, onClick: save },
        busy ? "Saving…" : "Save limits"),
      h("button", { type: "button", className: "btn", onClick: onDone }, "Cancel"),
      changed ? h("span", { className: "limits-dirty" }, "unsaved changes") : null),
  );
}

function validLimit(field, value) {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
    return false;
  }
  return !field.integer || (Number.isInteger(value) && value <= field.max);
}

function LimitRow({ field, value, onToggle, onChange }) {
  const on = value !== null && value !== undefined;
  const uncapped = !on && !field.defaulted;
  const bad = on && !validLimit(field, value);
  return h(
    "div",
    { className: `limit-row${uncapped ? " limit-row--off" : ""}` },
    h("button", {
      type: "button", className: "switch", "aria-pressed": on,
      "aria-label": `${field.noun} limit`,
      onClick: onToggle,
    }, on ? "on" : "off"),
    h("span", { className: "limit-name" }, field.noun,
      h("code", { className: "limit-key" }, field.key)),
    on
      ? h("input", {
          type: "number",
          min: field.integer ? "1" : "0.01",
          max: field.max ? String(field.max) : undefined,
          step: field.integer ? "1" : "0.01",
          className: `form-control form-control--mono${bad ? " form-control--invalid" : ""}`,
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
  const trigger = useRef(null);
  const close = () => closeDisclosure(setOpen, trigger);
  const label = `${assignment.work?.forge ?? "?"} · ${assignment.work?.source ?? "?"}`;
  return h(
    "div",
    { className: "field-disclosure" },
    h(
      "button",
      {
        ref: trigger,
        type: "button",
        className: "ws-value",
        "aria-expanded": open,
        title: "Link a board or issues page",
        onClick: () => setOpen((current) => !current),
      },
      label,
    ),
    open ? h(WorkSourceEditor, { assignment, onDone: close }) : null,
  );
}

function closeDisclosure(setOpen, trigger) {
  setOpen(false);
  requestAnimationFrame(() => trigger.current?.focus());
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
    const work = {
      forge: derived.forge,
      source: derived.source,
      filter: derived.filter,
      approval_label: assignment.work?.approvalLabel ?? null,
      abort_label: assignment.work?.abortLabel ?? "",
      escalate_label: assignment.work?.escalateLabel ?? "",
    };
    postIntent({ kind: "set-work-source", input: { assignment: assignment.name, work } }).then((result) => {
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
    { className: "ws-open", onKeyDown: (event) => event.key === "Escape" && onDone() },
    h("span", { className: "detail-label" }, "link a work source"),
    h("input", {
      type: "text",
      className: "form-control form-control--mono",
      autoFocus: true,
      placeholder: "Paste a board, query, or issues URL…",
      "aria-label": "Board, query, or issues URL",
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

/** Repos / roles / pipelines nothing references, surfaced without graph noise. */
function OrphanStrip({ state, view }) {
  if (!view.orphans?.length) {
    return null;
  }
  return h(
    "section",
    { className: "orphan-strip", "aria-label": "Unreferenced config" },
    h("h3", {}, "Unreferenced"),
    h(
      "div",
      { className: "orphan-list" },
      view.orphans.map((orphan) =>
        h("span", { key: `${orphan.kind}:${orphan.name}`, className: "orphan-entry" },
          h("span", { className: `chip orphan-chip orphan-chip--${orphan.kind}` }, `${orphan.kind}: ${orphan.name}`),
          h(DeleteControl, { dir: state.dir, kind: orphan.kind, name: orphan.name })),
      ),
    ),
  );
}

/** The full relation graph, collapsed by default as a secondary section. */
function RelationSection({ state }) {
  return h(
    "details",
    { className: "relation-section" },
    h("summary", {}, "Relation graph"),
    h(
      "div",
      { className: "config-flow", "aria-label": "Bureau config relation graph" },
      h(RelationGraph, { relation: state.config?.relation }),
    ),
  );
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
  if (state.selectedPipeline.missing) {
    return h(MissingPipeline, { notice: state.selectedPipeline.notice, name });
  }
  return h(
    "section",
    { className: "view-shell view-shell--pipeline" },
    h(
      "section",
      { className: "pipeline-main" },
      h(
        "div",
        { className: "pipeline-toolbar" },
        h("button", { className: "btn btn--small", type: "button", "data-testid": "pipeline-back", onClick: backToConfig }, "← Assignments"),
        h("h2", {}, name),
        h(ModeSwitcher, { mode, onMode: setMode }),
        h("a", { className: "btn btn--small editor-link", href: `./editor.html?pipeline=${encodeURIComponent(name)}` }, "Edit pipeline"),
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

/**
 * A pipeline the config no longer has. The editor already said so; the viewer
 * used to draw an empty graph instead, which reads as "this pipeline has no
 * steps" rather than "this pipeline is gone".
 */
function MissingPipeline({ notice, name }) {
  return h(
    "section",
    { className: "view-shell view-shell--config" },
    h(
      "div",
      { className: "pipeline-toolbar" },
      h("button", { className: "btn btn--small", type: "button", "data-testid": "pipeline-back", onClick: backToConfig }, "← Assignments"),
      h("h2", {}, name),
    ),
    h("p", { className: "status" }, notice ?? `No pipeline named \`${name}\` in this config.`),
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
  const labels = labelsForPipeline(state, layout.name);
  const terminals = layout.terminals.map((terminal) =>
    flowTerminal(terminal, handles.items[terminal.id], labels[terminal.name]));
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

function flowTerminal(terminal, handles, label) {
  return {
    id: terminal.id,
    type: "terminalPill",
    position: { x: terminal.x, y: terminal.y + 26 },
    data: { terminal, handles: handles ?? emptyHandles(), label },
    style: { width: 176 },
    draggable: false,
  };
}

function labelsForPipeline(state, pipeline) {
  const assignments = (state.config?.view?.assignments ?? [])
    .filter((assignment) => assignment.pipeline === pipeline);
  const one = (key) => {
    const labels = [...new Set(assignments.map((assignment) => assignment.work?.[key]).filter(Boolean))];
    return labels.length === 1 ? labels[0] : null;
  };
  return { abort: one("abortLabel"), escalate: one("escalateLabel") };
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
  const copy = terminalCopy(data.terminal.name);
  return h(
    "article",
    { className: `flow-card terminal-pill terminal-pill--${data.terminal.name}` },
    h(Handles, { handles: data.handles }),
    h("h2", {}, copy.label),
    h("p", { className: "terminal-detail" }, copy.detail),
    data.label ? h("code", { className: "terminal-forge-label" }, data.label) : null,
    h("code", { className: "terminal-key" }, data.terminal.name),
  );
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