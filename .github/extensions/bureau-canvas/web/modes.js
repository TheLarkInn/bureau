// Mode switcher: Design (the static config graph), Live (SSE-driven overlay
// of one live run), Replay (timeline-driven overlay of any run). The mode is
// pure UI state — the server knows nothing about it.

import React, { useEffect, useState } from "react";

const h = React.createElement;
export const MODES = ["design", "live", "replay"];
const REFRESH_MS = 4000;

/** Segmented control over the three graph modes. */
export function ModeSwitcher({ mode, onMode }) {
  return h(
    "div",
    { className: "mode-switcher", role: "tablist", "aria-label": "Graph mode" },
    MODES.map((option) =>
      h("button", {
        key: option,
        type: "button",
        role: "tab",
        "aria-selected": mode === option,
        "data-testid": `mode-${option}`,
        className: `mode-tab${mode === option ? " mode-tab--active" : ""}`,
        onClick: () => onMode(option),
      }, option),
    ),
  );
}

/**
 * Dropdown over `GET /runs`, refreshed on a poll so live runs appear and
 * disappear while the panel is open. `liveOnly` filters for the live tab;
 * replay takes any run.
 */
export function RunPicker({ liveOnly, value, onChange }) {
  const [runs, setRuns] = useState([]);
  useEffect(() => {
    let alive = true;
    const load = () => {
      fetch("./runs", { cache: "no-store" })
        .then((response) => (response.ok ? response.json() : { runs: [] }))
        .then((payload) => {
          if (alive) {
            setRuns((payload.runs ?? []).filter((run) => !liveOnly || run.live));
          }
        })
        .catch(() => {});
    };
    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [liveOnly]);
  return h(
    "select",
    {
      className: "run-picker",
      value: value ?? "",
      "aria-label": liveOnly ? "Live run" : "Replay run",
      onChange: (event) => onChange(event.target.value || null),
    },
    h("option", { value: "" }, liveOnly ? "no live run selected" : "no run selected"),
    runs.map((run) =>
      h("option", { key: run.run_id, value: run.run_id }, runLabel(run)),
    ),
  );
}

function runLabel(run) {
  const when = run.started_at ? run.started_at.replace("T", " ").replace(/\.\d+Z$/u, "Z") : "unknown start";
  const where = run.assignment ?? "unknown assignment";
  const now = run.live ? `live${run.current_step ? `: ${run.current_step}` : ""}` : "finished";
  return `${run.run_id} · ${where} · ${when} · ${now}`;
}
