// Live mode: overlay the selected run's SSE events onto the static pipeline
// graph. The overlay itself is the shared pure reducer in ./overlay.js; this
// module only owns the subscription, the run-control intents, and the
// collapsed-group toggle state.

import React, { useEffect, useMemo, useRef, useState } from "react";
import { RunPicker } from "../modes.js";
import { RECONCILE_REFUSED, applyEvent, emptyOverlay, newRunSince, reconcileReason, runActions } from "./overlay.js";

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


/** One SSE frame, or `null` when it is not the JSON this channel promises. */
function parseFrame(data) {
  try {
    return JSON.parse(data);
  } catch {
    return null;
  }
}

/** Posts the pass and reduces every ending to `ok`, or a reason not to report one. */
function postReconcile() {
  return fetch("./intent", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "reconcile-now" }),
  })
    .then((response) => response.json())
    .then((result) => (result?.ok
      ? { ok: true }
      : { ok: false, message: reconcileReason(result) }))
    .catch(() => ({ ok: false, message: RECONCILE_REFUSED }));
}

/**
 * Returns `{ runId, setRunId, decoration, controls }` — the decoration feeds
 * `toFlow(pipeline, state, selectedStep, decoration)`; `controls` renders
 * into the pipeline toolbar.
 */
export function useLiveOverlay(activity, onOpenReplay) {
  const [runId, setRunId] = useState(null);
  const [overlay, setOverlay] = useState(emptyOverlay);
  const [events, setEvents] = useState([]);
  const [collapsed, setCollapsed] = useState(new Set());
  const [controlResult, setControlResult] = useState(null);
  const [reconciling, setReconciling] = useState(false);
  const [reconcileResult, setReconcileResult] = useState(null);
  const controlTicket = useRef(0);

  useEffect(() => {
    // A refusal and a fold both belong to the run that was selected when they
    // were raised, so they are withdrawn here rather than inside the branch
    // that loads one. Clearing them only on the `runId` path left a red
    // "could not pause this run" beside a picker naming no run, describing a
    // request that no longer has a subject and that nothing can retry.
    setOverlay(emptyOverlay());
    setEvents([]);
    setCollapsed(new Set());
    setControlResult(null);
    // Bumped on every change of selection, so a reply that was in flight when
    // the reader moved on cannot install itself over the run they moved to.
    controlTicket.current += 1;
    if (!runId) {
      return undefined;
    }
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
    // A frame this cannot parse is dropped, not thrown. An exception raised
    // inside an `EventSource` listener escapes as an uncaught error on the
    // page, which the state matrix reads as a broken render — so one malformed
    // frame would condemn every state that streams a run.
    source.addEventListener("run-event", (message) => {
      const frame = parseFrame(message.data);
      if (!frame) {
        return;
      }
      const { run_id, event } = frame;
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
    const mine = controlTicket.current;
    // A reply is only about the run that was selected when it was asked for.
    const settle = (result) => {
      if (mine === controlTicket.current) {
        setControlResult(result);
      }
    };
    fetch("./intent", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind, run_id: runId }),
    })
      .then((response) => response.json())
      .then((result) => settle(result?.ok ? result : { ...result, error: result?.error || result?.output || refused }))
      .catch(() => settle({ ok: false, error: refused }));
  };

  /*
   * One reconcile pass, and an honest report of what it did.
   *
   * `bureau reconcile --now` does not return when the pass is issued — it
   * drains every run the pass started first (`after_first` -> `daemon.drain`).
   * So by the time this answer arrives the work is over: the run exists, and it
   * is already finished. Two things follow, and both were wrong before.
   *
   * The listing is re-read here rather than waited for, because the fact is
   * already true and a poll interval would only delay reporting it.
   *
   * And the run is handed to Replay, not to Live. Selecting a finished run into
   * a tab defined as "one live run, streamed" put a completed run under live
   * transport controls, and telling the reader to "choose it" pointed at a run
   * the live-only picker will not offer. Replay is where a finished run is
   * read, so that is where the button sends it — by offering the move, never by
   * performing it, so nothing the reader is watching is taken away.
   *
   * `known` and `since` together are the attribution. The pass can be open for
   * minutes, and a background reconciler's run that appears in that window is
   * absent from `known` while being none of this click's doing; requiring it to
   * have started after the click is what keeps the sentence true.
   */
  const reconcileNow = () => {
    const known = new Set(activity.runs.map((run) => run.run_id));
    const since = Date.now();
    setReconciling(true);
    setReconcileResult(null);
    postReconcile()
      .then((outcome) => (outcome.ok ? reportPass(known, since) : outcome))
      .then(setReconcileResult)
      .catch(() => setReconcileResult({ ok: false, message: RECONCILE_REFUSED }))
      .finally(() => setReconciling(false));
  };

  const reportPass = async (known, since) => {
    const next = await activity.refresh();
    if (next.status !== "ready") {
      return { ok: true, message: "Reconcile pass finished. The run list could not be read." };
    }
    const started = newRunSince(next.runs, known, since);
    if (!started) {
      return { ok: true, message: "Reconcile pass finished. It claimed no work for this pipeline." };
    }
    return {
      ok: true,
      run: started.run_id,
      message: `Reconcile pass finished. It ran ${started.run_id}, which has already finished.`,
    };
  };

  const controls = h(
    "div",
    { className: "run-controls" },
    h(RunPicker, { activity, liveOnly: true, value: runId, onChange: setRunId }),
    h("button", {
      type: "button",
      className: "btn btn--small btn--primary",
      "data-testid": "reconcile-now",
      disabled: reconciling,
      onClick: reconcileNow,
    }, reconciling ? "Reconciling…" : "Run reconcile now"),
    runId ? h(RunButtons, { overlay, onAction: send }) : null,
    controlResult && !controlResult.ok ? h("p", { className: "run-control-error" }, controlResult.error) : null,
    reconcileResult ? h("p", {
      className: reconcileResult.ok ? "run-control-result" : "run-control-error",
      role: "status",
    }, reconcileResult.message) : null,
    reconcileResult?.run ? h("button", {
      type: "button",
      className: "run-control",
      "data-testid": "open-run-replay",
      onClick: () => onOpenReplay?.(reconcileResult.run),
    }, "Open in Replay") : null,
  );

  /**
   * Withdraws what this visit to Live said. The hook outlives the surface —
   * it is owned by `PipelineView`, and leaving Live only stops rendering
   * `controls` — so without this a refusal raised before a trip to Design is
   * still on screen on the way back, describing a request this visit never
   * made. The selected run is deliberately kept: returning to Live should
   * return to the run you were watching.
   */
  const dismissControls = () => {
    controlTicket.current += 1;
    setControlResult(null);
    setReconcileResult(null);
  };

  return { runId, setRunId, decoration, controls, events, dismissControls, until: Infinity };
}

/** The Live surface must distinguish idle, loading, and unavailable. */
export function LiveActivity({ activity, runId }) {
  if (runId && activity.status !== "error") {
    return null;
  }
  const content = activityMessage(activity);
  return h(
    "section",
    {
      className: `run-activity run-activity--${content.state}`,
      "data-testid": "run-activity",
      "data-state": content.state,
      "aria-live": "polite",
    },
    h("span", { className: "run-activity__mark", "aria-hidden": "true" }),
    h("div", {}, h("p", { className: "run-activity__title" }, content.title), h("p", { className: "run-activity__detail" }, content.detail)),
  );
}

function activityMessage(activity) {
  if (activity.status === "loading") {
    return { state: "loading", title: "Checking run activity", detail: "Reading Bureau's run log for this pipeline." };
  }
  if (activity.status === "error") {
    // Scoped to the run *list*, because that is the only thing that failed. A
    // run already selected keeps streaming from its own endpoint, and saying
    // the run log could not be read while its events are visibly arriving is
    // two claims about one screen that cannot both be true.
    return { state: "unavailable", title: "Run list unavailable", detail: "The canvas could not list Bureau's runs. It will retry automatically; a run already open keeps streaming." };
  }
  if (activity.liveCount > 0) {
    return {
      state: "available",
      title: `${activity.liveCount} ${activity.liveCount === 1 ? "run" : "runs"} in progress`,
      detail: "Choose a run to inspect it, or run another reconcile pass now.",
    };
  }
  return {
    state: "idle",
    title: "No runs in progress",
    detail: "A reconcile loop is not itself a run. Live appears here when eligible work is claimed for this pipeline.",
  };
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
