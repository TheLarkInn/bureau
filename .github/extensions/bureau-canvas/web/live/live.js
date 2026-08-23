// Live mode: overlay the selected run's SSE events onto the static pipeline
// graph. The overlay itself is the shared pure reducer in ./overlay.js; this
// module only owns the subscription, the run-control intents, and the
// collapsed-group toggle state.

import React, { useEffect, useMemo, useState } from "react";
import { RunPicker } from "../modes.js";
import { applyEvent, emptyOverlay, runActions } from "./overlay.js";

const h = React.createElement;

/*
 * What a refused run control says, and why it names the verb.
 *
 * It used to say "intent failed" — the name of the endpoint the browser posts
 * to, which is this canvas's own plumbing and not a thing the reader has any
 * word for. It also said the same sentence whichever button was pressed, so a
 * pause the host refused and a cancel the host refused were one message. The
 * draft bar already settled both points for writes: name the verb, in the
 * reader's words, because "could not discard" and "could not save" leave the
 * work in opposite places. A run control is the same claim about a run.
 *
 * These are fallbacks. A host that explains itself is quoted instead; this is
 * what is said when the answer carries no reason, or never arrives at all.
 */
const REFUSED = {
  "pause-run": "could not pause this run",
  "resume-run": "could not resume this run",
  "cancel-run": "could not cancel this run",
};

/**
 * Returns `{ runId, setRunId, decoration, controls }` — the decoration feeds
 * `toFlow(pipeline, state, selectedStep, decoration)`; `controls` renders
 * into the pipeline toolbar.
 */
export function useLiveOverlay() {
  const [runId, setRunId] = useState(null);
  const [overlay, setOverlay] = useState(emptyOverlay);
  const [events, setEvents] = useState([]);
  const [collapsed, setCollapsed] = useState(new Set());
  const [controlResult, setControlResult] = useState(null);

  useEffect(() => {
    if (!runId) {
      setOverlay(emptyOverlay());
      setEvents([]);
      return undefined;
    }
    setOverlay(emptyOverlay());
    setEvents([]);
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
        // Kept raw as well: the step log renders the output events, which
        // the overlay reducer deliberately does not retain.
        setEvents(payload.events ?? []);
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
      setEvents((current) => [...current, event]);
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
    const refused = REFUSED[kind] ?? "could not act on this run";
    fetch("./intent", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind, run_id: runId }),
    })
      .then((response) => response.json())
      .then((result) => setControlResult(result?.ok ? result : { ...result, error: result?.error ?? result?.output ?? refused }))
      .catch(() => setControlResult({ ok: false, error: refused }));
  };

  const controls = h(
    "div",
    { className: "run-controls" },
    h(RunPicker, { liveOnly: true, value: runId, onChange: setRunId }),
    runId ? h(RunButtons, { overlay, onAction: send }) : null,
    controlResult && !controlResult.ok ? h("p", { className: "run-control-error" }, controlResult.error) : null,
  );

  return { runId, setRunId, decoration, controls, events, until: Infinity };
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
    h("span", { className: "run-status", "data-status": overlay.status }, overlay.status),
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
