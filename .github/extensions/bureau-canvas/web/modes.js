// Mode switcher: Design (the static config graph), Live (SSE-driven overlay
// of one live run), Replay (timeline-driven overlay of any run). The mode is
// pure UI state — the server knows nothing about it.

import React, { useEffect, useState } from "react";
import { runsOffered } from "./live/overlay.js";

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
 *
 * The run being watched is always listed, even once the filter would drop it.
 * A run is live exactly while its log has no `run_finished` event, so one that
 * ends under the reader leaves the live list on the next poll — and a `<select>`
 * whose `value` matches no `<option>` reports `selectedIndex === -1` and draws
 * blank. The overlay stays on screen, so that is a picker claiming no run while
 * a run is being shown. Keeping the entry also lets its label carry the news:
 * `runLabel` says `finished` for it, which is the only place the chrome says so.
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
            setRuns(payload.runs ?? []);
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
    runsOffered(runs, { liveOnly, watching: value }).map((run) =>
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
