// Replay mode: fetch one run's full event log once, then scrub it on a
// timeline. Scrubbing to time T applies every event with at_ms <= T through
// the same pure reducer (web/live/overlay.js) live mode uses — there is no
// second code path for historical runs.

import React, { useEffect, useMemo, useRef, useState } from "react";
import { RunPicker } from "../modes.js";
import { emptyOverlay, stateUpTo } from "../live/overlay.js";

const h = React.createElement;
const SPEEDS = [1, 4, 16];
const TICK_MS = 100;

export function useReplayOverlay() {
  const [runId, setRunId] = useState(null);
  const [events, setEvents] = useState([]);
  const [range, setRange] = useState({ start: 0, end: 0 });
  const [position, setPosition] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [collapsed, setCollapsed] = useState(new Set());
  const positionRef = useRef(0);

  useEffect(() => {
    if (!runId) {
      setEvents([]);
      setPlaying(false);
      return undefined;
    }
    let alive = true;
    setCollapsed(new Set());
    fetch(`./runs/${encodeURIComponent(runId)}/events`, { cache: "no-store" })
      .then((response) => (response.ok ? response.json() : { events: [] }))
      .then((payload) => {
        if (!alive) {
          return;
        }
        const list = payload.events ?? [];
        const start = list.length ? list[0].at_ms ?? 0 : 0;
        const end = list.length ? (list.at(-1).at_ms ?? start) : start;
        setEvents(list);
        setRange({ start, end });
        positionRef.current = start;
        setPosition(start);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [runId]);

  useEffect(() => {
    if (!playing) {
      return undefined;
    }
    const timer = setInterval(() => {
      positionRef.current += TICK_MS * speed;
      if (positionRef.current >= range.end) {
        positionRef.current = range.end;
        setPlaying(false);
      }
      setPosition(positionRef.current);
    }, TICK_MS);
    return () => clearInterval(timer);
  }, [playing, speed, range.end]);

  const overlay = useMemo(() => (events.length ? stateUpTo(events, position) : emptyOverlay()), [events, position]);
  const decoration = useMemo(() => (runId ? {
    overlay,
    collapsed,
    onToggleGroup: (group) => setCollapsed((current) => toggle(current, group)),
  } : null), [runId, overlay, collapsed]);

  const stepBy = (direction) => {
    const stamps = events.map((event) => event.at_ms ?? 0).filter((at) => (direction > 0 ? at > position : at < position));
    const target = direction > 0 ? Math.min(...stamps) : Math.max(...stamps);
    if (Number.isFinite(target)) {
      positionRef.current = target;
      setPosition(target);
      setPlaying(false);
    }
  };

  const controls = h(
    "div",
    { className: "replay-controls" },
    h(RunPicker, { liveOnly: false, value: runId, onChange: setRunId }),
    runId ? h(Timeline, { range, position, playing, speed, onScrub, onPlay: () => setPlaying(!playing), onSpeed: setSpeed, onStep: stepBy }) : null,
  );

  function onScrub(value) {
    positionRef.current = value;
    setPosition(value);
    setPlaying(false);
  }

  return { runId, setRunId, decoration, controls, events, until: position };
}

function Timeline({ range, position, playing, speed, onScrub, onPlay, onSpeed, onStep }) {
  return h(
    "div",
    { className: "replay-timeline" },
    h("button", { type: "button", className: "run-control", onClick: () => onStep(-1), "aria-label": "Step back" }, "◂"),
    h("button", { type: "button", className: "run-control", onClick: onPlay }, playing ? "Pause" : "Play"),
    h("button", { type: "button", className: "run-control", onClick: () => onStep(1), "aria-label": "Step forward" }, "▸"),
    SPEEDS.map((option) =>
      h("button", {
        key: option,
        type: "button",
        className: `run-control${speed === option ? " run-control--active" : ""}`,
        onClick: () => onSpeed(option),
      }, `${option}x`),
    ),
    h("input", {
      type: "range",
      className: "replay-scrubber",
      min: range.start,
      max: range.end,
      value: position,
      "aria-label": "Replay position",
      onChange: (event) => onScrub(Number(event.target.value)),
    }),
    h("span", { className: "run-status mono" }, `+${((position - range.start) / 1000).toFixed(1)}s`),
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
