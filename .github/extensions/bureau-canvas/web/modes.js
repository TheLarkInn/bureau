// Mode switcher: Design (the static config graph), Live (SSE-driven overlay
// of one live run), Replay (timeline-driven overlay of any run). The mode is
// pure UI state — the server knows nothing about it.

import React, { useEffect, useMemo, useRef, useState } from "react";
import { runsForPipeline, runsOffered, unattributedRuns } from "./live/overlay.js";

const h = React.createElement;
export const MODES = ["design", "live", "replay"];
const REFRESH_MS = 4000;

/** Segmented control over the three graph modes. */
export function ModeSwitcher({ mode, onMode, activity }) {
  return h(
    "div",
    { className: "mode-switcher", role: "tablist", "aria-label": "Graph mode" },
    MODES.map((option) => {
      const live = option === "live";
      const label = live ? liveLabel(activity) : option;
      return (
        h("button", {
          key: option,
          type: "button",
          role: "tab",
          "aria-selected": mode === option,
          "aria-label": label,
          "data-testid": `mode-${option}`,
          className: `mode-tab${mode === option ? " mode-tab--active" : ""}`,
          onClick: () => onMode(option),
        }, option, live ? h(LiveCount, { activity }) : null)
      );
    }),
  );
}

/**
 * One read of the run listing, in the shape the badge and pickers consume.
 *
 * Shared by the poll and by `refresh`, so a failed listing is the same
 * `status: "error"` however it was asked for — a caller that raced its own
 * `fetch` here could report zero runs for a listing that was never read.
 */
async function readRuns() {
  try {
    const response = await fetch("./runs", { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`run listing returned ${response.status}`);
    }
    const payload = await response.json();
    return { status: "ready", runs: payload.runs ?? [] };
  } catch {
    return { status: "error", runs: [] };
  }
}

/**
 * One poll shared by the mode badge and both run pickers.
 *
 * `refresh` re-reads the listing on demand and answers with this pipeline's
 * runs. It exists because `bureau reconcile --now` finishes before it answers:
 * whatever it started is already in the log by then, and waiting up to a whole
 * poll interval to say so would make the button's report arrive after the fact
 * it reports on.
 */
export function useRunActivity(pipeline, assignments) {
  const [snapshot, setSnapshot] = useState({ status: "loading", runs: [] });
  // One clock over both writers. The poll and `refresh` read the same endpoint
  // concurrently, and a poll issued *before* a refresh can resolve *after* it —
  // publishing the older listing last. That is how a just-started run
  // disappears from the picker while its overlay is on screen, which is exactly
  // the blank-select defect `RunPicker` below is written to prevent.
  const clock = useRef({ issued: 0, published: 0, alive: true });
  const publish = (next, seq) => {
    if (!clock.current.alive || seq < clock.current.published) {
      return;
    }
    clock.current.published = seq;
    setSnapshot(next);
  };
  useEffect(() => {
    const state = clock.current;
    state.alive = true;
    const load = () => {
      const seq = ++state.issued;
      return readRuns().then((next) => {
        publish(next, seq);
        return next;
      });
    };
    load();
    const timer = setInterval(load, REFRESH_MS);
    return () => {
      state.alive = false;
      clearInterval(timer);
    };
  }, []);
  const runs = useMemo(
    () => runsForPipeline(snapshot.runs, pipeline, assignments),
    [snapshot.runs, pipeline, assignments],
  );
  const orphans = useMemo(
    () => unattributedRuns(snapshot.runs, assignments),
    [snapshot.runs, assignments],
  );
  const refresh = async () => {
    const seq = ++clock.current.issued;
    const next = await readRuns();
    publish(next, seq);
    return { status: next.status, runs: runsForPipeline(next.runs, pipeline, assignments) };
  };
  return {
    status: snapshot.status,
    runs,
    orphans,
    refresh,
    liveCount: runs.filter((run) => run.live).length,
  };
}

function LiveCount({ activity }) {
  const count = activity.status === "ready" ? String(activity.liveCount) : activity.status === "error" ? "!" : "…";
  const active = activity.status === "ready" && activity.liveCount > 0;
  return h("span", {
    className: `live-count${active ? " live-count--active" : ""}`,
    "data-testid": "live-count",
    "data-state": activity.status,
    "data-count": activity.status === "ready" ? String(activity.liveCount) : undefined,
    "aria-hidden": "true",
  }, count);
}

function liveLabel(activity) {
  if (activity.status === "loading") {
    return "live, checking run activity";
  }
  if (activity.status === "error") {
    return "live, run activity unavailable";
  }
  return `live, ${activity.liveCount} ${activity.liveCount === 1 ? "run" : "runs"} in progress`;
}

/**
 * Dropdown over the shared run-activity snapshot. `liveOnly` filters for the
 * live tab; replay takes any run.
 *
 * The run being watched is always listed, even once the filter would drop it.
 * A run is live exactly while its log has no `run_finished` event, so one that
 * ends under the reader leaves the live list on the next poll — and a `<select>`
 * whose `value` matches no `<option>` reports `selectedIndex === -1` and draws
 * blank. The overlay stays on screen, so that is a picker claiming no run while
 * a run is being shown. Keeping the entry also lets its label carry the news:
 * `runLabel` says `finished` for it, which is the only place the chrome says so.
 */
export function RunPicker({ activity, liveOnly, value, onChange }) {
  const runs = offerWatched(runsOffered(activity.runs, { liveOnly, watching: value }), value);
  const empty = runs.length === 0;
  // Disabled only when there is nothing to choose *and* nothing to leave. A
  // reader watching a run while the listing fails was previously locked in: the
  // control went dead, the run had no option of its own, and the panel above it
  // announced that the run log could not be read while that run's events were
  // visibly streaming from a different endpoint.
  return h(
    "select",
    {
      className: "run-picker",
      value: value ?? "",
      disabled: empty && !value,
      "aria-label": liveOnly ? "Live run" : "Replay run",
      onChange: (event) => onChange(event.target.value || null),
    },
    h("option", { value: "" }, pickerPrompt(activity, liveOnly, empty)),
    runs.map((run) =>
      h("option", { key: run.run_id, value: run.run_id }, runLabel(run)),
    ),
  );
}

/**
 * The watched run always has an option, even when no listing describes it.
 *
 * `runsOffered` can only *retain* an entry that is present; it cannot recover
 * one from a listing that failed and returned nothing. Without this a
 * `<select>` whose `value` names a run the options do not contain reports
 * `selectedIndex === -1` and draws blank over a running overlay.
 */
function offerWatched(runs, value) {
  if (!value || runs.some((run) => run.run_id === value)) {
    return runs;
  }
  return [...runs, { run_id: value, assignment: null, started_at: null, live: false, unlisted: true }];
}

function pickerPrompt(activity, liveOnly, empty) {
  if (activity.status === "loading") {
    return "checking run activity…";
  }
  if (activity.status === "error") {
    return "run list unavailable";
  }
  if (empty) {
    return liveOnly ? "no runs in progress" : orphanPrompt(activity);
  }
  return liveOnly ? "choose a live run" : "choose a run";
}

/**
 * An empty Replay list is only "no runs recorded" when that is true.
 *
 * Runs whose pipeline nothing can name are still runs on disk. Reporting them
 * as an absence is the same dishonest zero as reading a failed listing as `0`.
 */
function orphanPrompt(activity) {
  const orphans = activity.orphans ?? [];
  if (orphans.length === 0) {
    return "no runs recorded";
  }
  return `no runs for this pipeline (${orphans.length} unattributed)`;
}

function runLabel(run) {
  if (run.unlisted) {
    return `${run.run_id} · not in the current run list`;
  }
  const when = run.started_at ? run.started_at.replace("T", " ").replace(/\.\d+Z$/u, "Z") : "unknown start";
  const where = run.assignment ?? "unknown assignment";
  const now = run.live ? `live${run.current_step ? `: ${run.current_step}` : ""}` : "finished";
  return `${run.run_id} · ${where} · ${when} · ${now}`;
}
