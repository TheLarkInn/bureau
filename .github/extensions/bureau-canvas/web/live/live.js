// Live mode: overlay the selected run's SSE events onto the static pipeline
// graph. The overlay itself is the shared pure reducer in ./overlay.js; this
// module only owns the subscription, the run-control intents, and the
// collapsed-group toggle state.

import React, { useEffect, useMemo, useState } from "react";
import { RunPicker } from "../modes.js";
import { applyEvent, emptyOverlay, runActions } from "./overlay.js";

const h = React.createElement;

/**
 * Returns `{ runId, setRunId, decoration, controls }` — the decoration feeds
 * `toFlow(pipeline, state, selectedStep, decoration)`; `controls` renders
 * into the pipeline toolbar.
 */
export function useLiveOverlay() {
  const [runId, setRunId] = useState(null);
  const [overlay, setOverlay] = useState(emptyOverlay);
  const [collapsed, setCollapsed] = useState(new Set());
  const [controlResult, setControlResult] = useState(null);

  useEffect(() => {
    if (!runId) {
      setOverlay(emptyOverlay());
      return undefined;
    }
    setOverlay(emptyOverlay());
    setCollapsed(new Set());
    setControlResult(null);
    // Backfill the current log once, then append live: the tail starts at
    // end-of-file, so events that landed before subscription only exist here.
    let alive = true;
    let overlayState = emptyOverlay();
    fetch(`./runs/${encodeURIComponent(runId)}/events`, { cache: "no-store" })
      .then((response) => (response.ok ? response.json() : { events: [] }))
      .then((payload) => {
        if (!alive) {
          return;
        }
        for (const event of payload.events ?? []) {
          overlayState = applyEvent(overlayState, event);
        }
        setOverlay(overlayState);
      })
      .catch(() => {});
    const source = new EventSource("./events");
    source.addEventListener("run-event", (message) => {
      const { run_id, event } = JSON.parse(message.data);
      if (run_id !== runId) {
        return;
      }
      overlayState = applyEvent(overlayState, event);
      setOverlay(overlayState);
    });
    return () => {
      alive = false;
      source.close();
    };
  }, [runId]);

  const decoration = useMemo(() => (runId ? {
    overlay,
    collapsed,
    onToggleGroup: (group) => setCollapsed((current) => toggle(current, group)),
  } : null), [runId, overlay, collapsed]);

  const send = (kind) => {
    fetch("./intent", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind, run_id: runId }),
    })
      .then((response) => response.json())
      .then((result) => setControlResult(result))
      .catch(() => setControlResult({ ok: false, error: "intent failed" }));
  };

  const controls = h(
    "div",
    { className: "run-controls" },
    h(RunPicker, { liveOnly: true, value: runId, onChange: setRunId }),
    runId ? h(RunButtons, { overlay, onAction: send }) : null,
    controlResult && !controlResult.ok ? h("p", { className: "run-control-error" }, controlResult.error ?? controlResult.output ?? "intent failed") : null,
  );

  return { runId, setRunId, decoration, controls };
}

/**
 * Which run controls a status can still act on lives in `overlay.js`, beside
 * the reducer that produces the status — pure, and testable without a browser.
 */
function RunButtons({ overlay, onAction }) {
  const { transport, cancel } = runActions(overlay.status);
  return h(
    React.Fragment,
    null,
    transport === "resume"
      ? h("button", { type: "button", className: "run-control", "data-testid": "run-resume", onClick: () => onAction("resume-run") }, "Resume")
      : null,
    transport === "pause"
      ? h("button", { type: "button", className: "run-control", "data-testid": "run-pause", onClick: () => onAction("pause-run") }, "Pause")
      : null,
    cancel
      ? h("button", { type: "button", className: "run-control run-control--danger", "data-testid": "run-cancel", onClick: () => onAction("cancel-run") }, "Cancel")
      : null,
    h("span", { className: "run-status" }, overlay.status),
  );
}

function toggle(set, name) {
  const next = new Set(set);
  if (next.has(name)) {
    next.delete(name);
  } else {
    next.add(name);
  }
  return next;
}
