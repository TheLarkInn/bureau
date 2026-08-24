// Run-event overlay: the one reducer both live (SSE-driven) and replay
// (timeline-driven) modes feed. `applyEvent(state, event)` is pure — same
// event stream in, same overlay out — so the headless test suite exercises
// this module directly (test/overlay.test.mjs).
//
// Overlays key by step NAME, never by layout node id: collapsing a
// concurrent step's members out of the graph changes node ids, while the
// run log only ever names steps. resolveOverlay() turns the projection into
// per-node classes, per-edge animation flags, and expansion hints a React
// Flow view applies to the static pipeline graph.

export const STEP_PENDING = "pending";
export const STEP_RUNNING = "running";
export const STEP_COMPLETED = "completed";
export const STEP_SKIPPED = "skipped";
export const MEMBER_RUNNING = "running";
export const MEMBER_CANCELLED = "cancelled";
export const PAUSED_MESSAGE = /paused at a step boundary/u;

export const EVENTS = {
  runStarted: "run_started",
  stepStarted: "step_started",
  output: "output",
  stepFinished: "step_finished",
  groupStarted: "group_started",
  groupMemberStarted: "group_member_started",
  groupMemberFinished: "group_member_finished",
  groupMemberCancelled: "group_member_cancelled",
  groupFinished: "group_finished",
  runFinished: "run_finished",
};

/**
 * Which run controls a status can still act on.
 *
 * A run that has reached a terminal cannot be paused, resumed or cancelled —
 * there is nothing left to act on — so the buttons go and the status stays.
 * `RunPicker` lists only live runs, so this is not a screen the picker can
 * open: it is the one a reader is left with after watching a run they picked
 * while it was running reach its end. Branching on `paused` alone offered
 * Pause on a finished run, which is a control that could only fail.
 *
 * Here rather than in `live.js` because it is a pure fact about a status, and
 * this is the module the offline suite can hold without a browser.
 */
export function runActions(status) {
  if (status === "finished") {
    return { transport: null, cancel: false };
  }
  return { transport: status === "paused" ? "resume" : "pause", cancel: true };
}

/**
 * Which runs a picker offers, given its filter and the run being watched.
 *
 * The live tab lists live runs, and a run is live exactly while its log holds
 * no `run_finished` event — so a run that ends under the reader leaves the list
 * on the next poll. A `<select>` whose `value` matches no `<option>` reports
 * `selectedIndex === -1` and draws blank, while the overlay it belongs to is
 * still on screen: a picker claiming no run beside a run being shown. So the
 * watched run is always offered, whatever the filter says, and its label is
 * then the one place the chrome reports that it finished.
 *
 * Here beside `runActions` for the same reason: it is a pure fact about a
 * listing, and this is the module the offline suite can hold without a browser.
 */
export function runsOffered(runs, { liveOnly, watching }) {
  return (runs ?? []).filter((run) => !liveOnly || run.live || run.run_id === watching);
}

/** What is said when a reconcile pass could not be started at all. */
export const RECONCILE_REFUSED = "Could not run reconcile now.";

/**
 * The host's reason for refusing a pass, and a sentence when it gave none.
 *
 * Coalescing on *emptiness* rather than on nullish is the whole point.
 * `runBureau` reports a non-zero exit as `` `${stdout}${stderr}`.trim() ``,
 * which is `""` when the command failed silently — and `""` is not nullish, so
 * a `??` chain selects it and the surface draws a refusal as an empty red
 * paragraph that announces nothing at all to a screen reader.
 *
 * Here rather than in `live.js` for the reason `runActions` is: `live.js`
 * imports React, so the offline suite cannot load it. A sentence the reader
 * depends on is decided in the module that can be held without a browser.
 */
export function reconcileReason(result) {
  return result?.error || result?.output || RECONCILE_REFUSED;
}
/** Runs whose recorded pipeline, or assignment fallback, belongs to this view. */
export function runsForPipeline(runs, pipeline, assignments = []) {
  const owners = new Set(
    assignments
      .filter((assignment) => assignment.pipeline === pipeline)
      .map((assignment) => assignment.name),
  );
  return (runs ?? []).filter((run) =>
    run.pipeline ? run.pipeline === pipeline : owners.has(run.assignment),
  );
}

/**
 * Runs no pipeline in this config can claim.
 *
 * A run names its pipeline in the `run_started` snapshot and falls back to the
 * assignment that selected it; a log written before snapshots, or one whose
 * assignment has since been renamed or deleted, has neither. Those are counted
 * rather than discarded - dropping them let Replay report "no runs recorded"
 * over a run directory with runs in it, which is the dishonest zero this
 * surface exists to refuse.
 */
export function unattributedRuns(runs, assignments = []) {
  const known = new Set((assignments ?? []).map((assignment) => assignment.name));
  return (runs ?? []).filter((run) => !run.pipeline && !known.has(run.assignment));
}

/**
 * The newest run in this listing that the caller had not already seen.
 *
 * This is what "the run a reconcile pass started" means, and it is stated as a
 * difference on purpose. Picking the newest *live* run instead would let one
 * click on **Run reconcile now** move the reader onto a run that was already
 * in progress — including one the pass had nothing to do with, and including
 * one that was already being watched at a different point in its own log. A
 * pass that starts nothing must move nothing, and a refused pass most of all.
 *
 * `since` bounds it in time as well as by identity. `reconcile --now` drains
 * before it returns and can be open for minutes, so a background reconciler's
 * run can appear in the fresh listing while being none of this click's doing;
 * a run that started before the click was never started by it.
 */
export function newRunSince(runs, known, since = 0) {
  return [...(runs ?? [])]
    .filter((run) => !known.has(run.run_id) && runTime(run) >= since)
    .sort((left, right) => runTime(right) - runTime(left) || right.run_id.localeCompare(left.run_id))[0] ?? null;
}

function runTime(run) {
  const parsed = Date.parse(run.started_at ?? "");
  return Number.isFinite(parsed) ? parsed : 0;
}

export function emptyOverlay() {
  return {
    runId: null,
    assignment: null,
    status: "idle",
    steps: {},
    groups: {},
    current: null,
    transitions: [],
    lastSeq: -1,
  };
}

/** Scrubbing rebuilds from `emptyOverlay()`: any prefix of a log replays clean. */
export function applyEvents(events) {
  return (events ?? []).reduce(applyEvent, emptyOverlay());
}

/**
 * A run's history, plus whatever the tail delivered while it was in flight.
 *
 * Live opens its subscription in the same tick as the backfill, so a run
 * writing right now can deliver a frame *before* the history answers. Folding
 * the history in on top of that frame replays the run backwards — `run_started`
 * puts a run that has just finished back to `running` — and replacing the raw
 * list drops the frame outright. Neither is recoverable: the tail begins at
 * end-of-file and nothing replays it, so a `run_finished` that lost this race
 * left a finished run drawn as running for the rest of the visit.
 *
 * `seq` is the run log's own order, so it decides both questions. A frame the
 * history already carries is the same event rather than a second one, and the
 * result is ordered by it so the reducer sees the run in the order it happened.
 * An event without a `seq` keeps its position: the sort is stable, so a log
 * that predates sequencing replays exactly as it did before.
 *
 * Here beside the other pure facts about a run, so the offline suite can hold
 * it without a browser and without a clock.
 */
export function mergeRunEvents(history, tailed) {
  const carried = new Set((history ?? []).map(seqOf).filter((seq) => seq !== null));
  const missed = (tailed ?? []).filter((event) => {
    const seq = seqOf(event);
    return seq === null || !carried.has(seq);
  });
  return [...(history ?? []), ...missed].sort((left, right) => (seqOf(left) ?? 0) - (seqOf(right) ?? 0));
}

function seqOf(event) {
  return typeof event?.seq === "number" ? event.seq : null;
}

/** Replay position T: every event with at_ms <= T applied in log order. */
export function stateUpTo(events, atMs) {
  return applyEvents((events ?? []).filter((event) => eventMs(event) <= atMs));
}

export function applyEvent(overlay, event) {
  const handler = HANDLERS[event?.kind];
  return handler ? handler(overlay, event) : overlay;
}

const HANDLERS = {
  [EVENTS.runStarted]: runStarted,
  [EVENTS.stepStarted]: stepStarted,
  [EVENTS.output]: output,
  [EVENTS.stepFinished]: stepFinished,
  [EVENTS.groupStarted]: groupStarted,
  [EVENTS.groupMemberStarted]: groupMemberStarted,
  [EVENTS.groupMemberFinished]: groupMemberFinished,
  [EVENTS.groupMemberCancelled]: groupMemberCancelled,
  [EVENTS.groupFinished]: groupFinished,
  [EVENTS.runFinished]: runFinished,
};

function runStarted(overlay, event) {
  return withEvent(overlay, event, {
    runId: event.data?.run_id ?? overlay.runId,
    assignment: event.data?.assignment ?? overlay.assignment,
    status: "running",
  });
}

function stepStarted(overlay, event) {
  const step = stepName(event);
  const parent = parentOf(overlay, step);
  let next = withEvent(overlay, event, {
    steps: {
      ...overlay.steps,
      [step]: {
        outcome: null,
        state: STEP_RUNNING,
        role: event.data?.role ?? null,
        configuredAgent: event.data?.configured_agent ?? null,
        resolvedAgent: event.data?.resolved_agent ?? null,
        attempts: (overlay.steps[step]?.attempts ?? 0) + 1,
        startedAt: eventMs(event),
      },
    },
    current: step,
    transitions: [...overlay.transitions, { from: previousStep(overlay, step), to: step, outcome: lastOutcome(overlay) }],
  });
  if (parent) {
    next = { ...next, groups: { ...next.groups, [parent]: { ...next.groups[parent], state: "running" } } };
  }
  return next;
}

function output(overlay, event) {
  // The engine's pause marker lands as a run-level output line, not a
  // dedicated kind; the message text is the only signal in the log.
  const text = event.data?.data ?? "";
  if (event.data?.stream === "run" && PAUSED_MESSAGE.test(text)) {
    return withEvent(overlay, event, { status: "paused" });
  }
  return withEvent(overlay, event, {});
}

function stepFinished(overlay, event) {
  const step = stepName(event);
  const outcome = event.data?.outcome ?? null;
  const parent = parentOf(overlay, step);
  const next = withEvent(overlay, event, {
    steps: {
      ...overlay.steps,
      [step]: { ...(overlay.steps[step] ?? {}), state: STEP_COMPLETED, outcome, finishedAt: eventMs(event) },
    },
    current: overlay.current === step ? null : overlay.current,
  });
  if (parent && next.groups[parent]?.state === "finished") {
    return { ...next, groups: { ...next.groups, [parent]: { ...next.groups[parent], state: "running", outcome: null } } };
  }
  return next;
}

function groupStarted(overlay, event) {
  const group = event.data?.group;
  if (!group) {
    return overlay;
  }
  const members = {};
  for (const name of event.data?.members ?? []) {
    members[name] = { state: STEP_PENDING, outcome: null, attempts: 0 };
  }
  return withEvent(overlay, event, {
    groups: {
      ...overlay.groups,
      [group]: {
        state: "running",
        members,
        maxConcurrent: event.data?.max_concurrent ?? null,
        startedAt: eventMs(event),
      },
    },
    current: group,
  });
}

function groupMemberStarted(overlay, event) {
  return updateMember(overlay, event, (member) => ({
    ...member,
    state: MEMBER_RUNNING,
    attempts: member.attempts + 1,
    startedAt: eventMs(event),
  }));
}

function groupMemberFinished(overlay, event) {
  return updateMember(overlay, event, (member) => ({
    ...member,
    state: STEP_COMPLETED,
    outcome: event.data?.result?.outcome ?? null,
    finishedAt: eventMs(event),
  }));
}

function groupMemberCancelled(overlay, event) {
  return updateMember(overlay, event, (member) => ({
    ...member,
    state: MEMBER_CANCELLED,
    reason: event.data?.reason ?? null,
    finishedAt: eventMs(event),
  }));
}

function groupFinished(overlay, event) {
  const group = event.data?.group;
  if (!overlay.groups[group]) {
    return overlay;
  }
  return withEvent(overlay, event, {
    groups: {
      ...overlay.groups,
      [group]: { ...overlay.groups[group], state: "finished", outcome: event.data?.result?.outcome ?? null, finishedAt: eventMs(event) },
    },
    current: overlay.current === group ? null : overlay.current,
  });
}

function runFinished(overlay, event) {
  return withEvent(overlay, event, { status: "finished", outcome: event.data?.outcome ?? null, current: null });
}

function updateMember(overlay, event, change) {
  const { group, member } = event.data ?? {};
  const record = overlay.groups[group]?.members?.[member];
  if (!record) {
    return overlay;
  }
  return withEvent(overlay, event, {
    groups: {
      ...overlay.groups,
      [group]: { ...overlay.groups[group], members: { ...overlay.groups[group].members, [member]: change(record) } },
    },
  });
}

function withEvent(overlay, event, changes) {
  const seq = typeof event?.seq === "number" ? event.seq : overlay.lastSeq;
  return { ...overlay, ...changes, lastSeq: Math.max(overlay.lastSeq, seq) };
}

function stepName(event) {
  return event.data?.step ?? "";
}

/** The step that just handed off: the current one, else the last completed. */
function previousStep(overlay, nextStep) {
  if (overlay.current) {
    return overlay.current;
  }
  const last = overlay.transitions.at(-1);
  return last?.to ?? null;
}

function parentOf(overlay, step) {
  return Object.keys(overlay.groups).find((name) => overlay.groups[name].members[step]) ?? null;
}

function lastOutcome(overlay) {
  const last = overlay.transitions.at(-1);
  const from = overlay.current ?? last?.to ?? null;
  return from ? (overlay.steps[from]?.outcome ?? null) : null;
}

function eventMs(event) {
  return typeof event?.at_ms === "number" ? event.at_ms : 0;
}

/**
 * Projection onto the static pipeline graph. Layout steps keep their ids;
 * overlays match on `step.name`. Concurrent members (`parentId` set) collapse
 * into the group card until their group starts; once finished the expansion
 * is sticky — replay scrubbing replays the prefix, so it re-expands — and
 * `collapsed` lets the user fold a finished group away again. Edges touching
 * a hidden member are remapped onto the group node so the fan-out stays
 * visible instead of vanishing.
 */
export function resolveOverlay(pipeline, overlay, options = {}) {
  const layout = pipeline?.layout ?? { steps: [], edges: [], terminals: [] };
  const collapsed = options.collapsed ?? new Set();
  const hidden = new Set();
  for (const step of layout.steps ?? []) {
    if (step.parentId && groupHidden(overlay, collapsed, step.parentId)) {
      hidden.add(step.id);
    }
  }
  const parentById = new Map();
  for (const step of layout.steps ?? []) {
    if (step.parentId) {
      parentById.set(step.id, step.parentId);
    }
  }
  const remap = (id) => {
    let current = id;
    while (hidden.has(current)) {
      current = parentById.get(current) ?? current;
    }
    return current;
  };
  const nodes = (layout.steps ?? [])
    .filter((step) => !hidden.has(step.id))
    .map((step) => ({ id: step.id, name: step.name, className: stepClass(overlay, step), paused: isPausedAt(overlay, step) }));
  const animated = new Set(animatedEdges(overlay, layout.edges ?? []));
  const expanded = new Set(Object.keys(overlay.groups).filter((name) => !groupHidden(overlay, collapsed, name)));
  return {
    nodes,
    animatedEdges: animated,
    expandedGroups: expanded,
    foldableGroups: foldableGroups(overlay),
    overlayGroups: overlay.groups,
    remapEdge: remap,
    onToggleGroup: options.onToggleGroup ?? null,
  };
}

function stepClass(overlay, step) {
  // A concurrent member's state lives under its group record, keyed by name.
  if (step.parentId) {
    const member = overlay.groups[step.parentId]?.members?.[step.name];
    if (member) {
      return member.state === STEP_PENDING ? "overlay-pending" : member.state === MEMBER_RUNNING || member.state === "running" ? "overlay-running" : member.state === MEMBER_CANCELLED ? "overlay-cancelled" : `overlay-${member.outcome ?? "no-work"}`;
    }
  }
  const record = overlay.steps[step.name] ?? groupRecord(overlay, step.name);
  if (!record) {
    return overlay.status === "idle" ? "" : "overlay-pending";
  }
  if (record.state === "running" || record.state === STEP_RUNNING) {
    return "overlay-running";
  }
  return `overlay-${record.outcome ?? "no-work"}`;
}

function groupRecord(overlay, name) {
  const group = overlay.groups[name];
  if (!group) {
    return null;
  }
  if (group.state === "finished") {
    return { state: STEP_COMPLETED, outcome: group.outcome };
  }
  return { state: "running" };
}

/** Paused shows on the last active step — a plain step or the group card. */
function isPausedAt(overlay, step) {
  if (overlay.status !== "paused") {
    return false;
  }
  if (overlay.current === step.name) {
    return true;
  }
  // The engine clears current at a boundary; the paused step is the last one
  // that completed, or the group whose members were mid-flight.
  const last = overlay.transitions.at(-1);
  if (last?.to === step.name) {
    return true;
  }
  return Boolean(overlay.groups[step.name] && overlay.groups[step.name].state === "running");
}

function animatedEdges(overlay, edges) {
  const last = overlay.transitions.at(-1);
  if (!last?.from) {
    return [];
  }
  const candidates = edges
    .filter((edge) => edge.relation === "control")
    .filter((edge) => matchesStep(edges, edge.source, last.from) && matchesStep(edges, edge.target, last.to));
  const named = candidates.filter((edge) => edge.outcome === last.outcome);
  return (named.length ? named : candidates).map((edge) => edge.id);
}

function matchesStep(edges, endpoint, stepName) {
  return endpoint === stepName || endpoint === `step:${stepName}`;
}

function groupHidden(overlay, collapsed, groupName) {
  const group = overlay.groups[groupName];
  if (!group) {
    return true;
  }
  return group.state === "finished" && collapsed.has(groupName);
}

/**
 * The groups whose members can actually be folded away.
 *
 * `groupHidden` only honours `collapsed` once a group has finished, so a
 * running group's toggle was a control that reported nothing back: the click
 * landed, the set grew, and the members stayed exactly where they were. The
 * card asks this before drawing the control, so the only groups offering one
 * are the ones it moves.
 */
function foldableGroups(overlay) {
  return new Set(Object.keys(overlay.groups).filter((name) => overlay.groups[name].state === "finished"));
}
