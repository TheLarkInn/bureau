// The selector vocabulary the state registry is written against.
//
// One name per control, so a registry entry never spells a raw CSS string and
// a production rename breaks one line here instead of fifty. Everything is
// plain CSS on purpose: the same string has to resolve through
// `document.querySelector` in the lab and through a Playwright locator in the
// browser suite, with no engine-specific syntax in between.

export const SELECTORS = {
  shell: ".app-shell",
  header: ".app-header",
  status: ".app-header .status",
  loading: ".app-shell > .status",
  fallback: ".fallback-shell",
  fallbackState: ".fallback-state",

  configView: ".view-shell--config",
  configHeading: ".config-heading",
  assignmentStack: ".assignment-stack",
  assignmentCard: ".assignment-card",
  // The second card, which only exists once the stack really holds two. A
  // plain `.assignment-card` is satisfied by one, so it cannot tell a stack of
  // two from a stack of one.
  assignmentCardSecond: ".assignment-card + .assignment-card",
  assignmentHead: ".assignment-head",
  assignmentDetail: ".assignment-detail",
  assignmentEmpty: ".view-shell--config > .muted",

  createOpen: '[data-testid="create-open"]',
  createBar: '[data-testid="create-bar"]',
  createKind: "#create-kind",
  createName: "#create-name",
  createSubmit: '[data-testid="create-submit"]',
  createCancel: '[data-testid="create-cancel"]',
  // A refused create, as its own treatment rather than only its words: a
  // refusal drawn in the ordinary note class reads as advice.
  createRefused: ".create-bar .note--err",

  draftBar: '[data-testid="draft-bar"]',
  draftSave: '[data-testid="draft-save"]',
  draftDiscard: '[data-testid="draft-discard"]',
  draftList: ".draft-list",
  // A refused `save-plan` or `discard-plan`, as its own treatment. Separate
  // from the words for the same reason as `workSourceRefused`: a refusal
  // rendered as ordinary copy still reads as progress.
  draftRefused: ".draft-bar .note--err",

  workSourceValue: ".ws-value",
  workSourceEditor: ".ws-open",
  workSourceUrl: '[aria-label="Board, query, or issues URL"]',
  workSourceDerived: ".derived",
  workSourceSave: '[data-testid="work-source-save"]',
  // The three answers a paste can get, as three treatments. They are separate
  // selectors because the class is the message: an inferred filter shown in the
  // exact-derivation note, or a refusal shown as ordinary advice, is the defect
  // `lib/worksource.mjs` exists to prevent, and the words alone cannot catch it.
  workSourceRefused: ".ws-open .note--err",
  workSourceInferred: ".derived .note--warn",
  workSourceExact: ".derived > .note:not(.note--warn)",

  workRulesValue: ".runtime-value",
  workRulesEditor: ".assignment-runtime-editor",
  workRulesSave: '[data-testid="work-rules-save"]',

  signalsValue: ".terminal-label-value",
  signalsEditor: ".terminal-label-editor",
  signalsSave: '[data-testid="signals-save"]',

  reposValue: ".repos-value",
  reposEditor: ".repos-editor",
  reposSave: '[data-testid="repos-save"]',
  reposAdd: '[data-testid="repos-add"]',
  reposUrl: '[aria-label="Repository URL"]',
  // The resolved preview: the one place a `.detail-row` is drawn inside
  // another, which is why it is the render that exercises the overlap rule's
  // containment case.
  reposPreview: ".repos-preview",
  repoRow: ".repo-row",

  limitsValue: ".limits-value",
  limitsEditor: ".limits-editor",
  limitsSave: '[data-testid="limits-save"]',
  // The unsaved-changes marker, addressed inside the editor that drew it. The
  // class is shared by all five field editors now, so a bare `.draft-mark`
  // would be satisfied by whichever editor happened to be open beside it.
  limitsDirty: ".limits-editor .draft-mark",
  limitRow: ".limit-row",

  deleteStart: '[data-testid="delete-start"]',
  deleteConfirm: '[data-testid="delete-confirm"]',
  deleteCancel: '[data-testid="delete-cancel"]',
  preflight: '[data-testid="preflight"]',
  // The refusal a confirmed delete comes back with, scoped to the preflight so
  // it cannot be satisfied by an unrelated note elsewhere on the card.
  deleteRefused: '[data-testid="preflight"] .note--err',

  orphanStrip: ".orphan-strip",
  relationSection: ".relation-section",
  relationSummary: ".relation-section > summary",
  relationFlow: ".relation-flow",
  // Collapsed is read from the disclosure's own `open`, not from the graph's
  // presence: a closed `<details>` keeps its subtree mounted and still reports
  // client rects for it, so counting `.relation-flow` cannot tell the two
  // apart. This can, and it fails the moment the section ships `open`.
  relationOpen: ".relation-section[open]",

  pipelineView: ".view-shell--pipeline",
  pipelineToolbar: ".pipeline-toolbar",
  pipelineFlow: ".pipeline-flow",
  pipelineBack: '[data-testid="pipeline-back"]',
  pipelineEditLink: ".editor-link",
  sidePanel: ".side-panel",
  modeSwitcher: ".mode-switcher",
  modeDesign: '[data-testid="mode-design"]',
  modeLive: '[data-testid="mode-live"]',
  modeReplay: '[data-testid="mode-replay"]',
  runPickerLive: '[aria-label="Live run"]',
  runPickerReplay: '[aria-label="Replay run"]',
  runControls: ".run-controls",
  runStatus: ".run-controls .run-status",
  runPause: '[data-testid="run-pause"]',
  runResume: '[data-testid="run-resume"]',
  // Cancel is offered beside pause and resume for as long as the run can still
  // be acted on, and it is the one run control that cannot be undone — so a
  // state that draws it has to say so.
  runCancel: '[data-testid="run-cancel"]',
  // The status the run reached, addressable rather than only readable. The
  // span has always carried the word; a state whose whole subject is that the
  // transport went because the run ended has to be able to say *which* status
  // took it, and copy alone cannot — "finished" appears in the picker's own
  // label for the same run.
  runStatusFinished: '.run-status[data-status="finished"]',
  runControlError: ".run-control-error",
  replayControls: ".replay-controls",
  replayTimeline: ".replay-timeline",
  replayScrubber: ".replay-scrubber",
  replayStepBack: '[aria-label="Step back"]',
  replayStepForward: '[aria-label="Step forward"]',
  replayPlay: '[data-testid="replay-play"]',
  // The same button once it is running. The label is the whole difference, and
  // it is what a state asserting "playing" would have to read.
  replayPause: '[data-testid="replay-play"][aria-label="Pause replay"]',
  // The timeline renders the moment a run is picked, with an empty range; the
  // log arrives afterwards. `max` is the only signal that the events landed,
  // so it is what a replay path waits on rather than the timeline itself.
  replayLoaded: '.replay-scrubber:not([max="0"])',
  overlayRunning: ".flow-card.overlay-running",
  overlayPaused: ".flow-card.overlay-paused",
  // A concurrent group's card, the members it lists once it has finished, and
  // the control that folds them away. The fold lives on the card rather than
  // inside the member list, so that collapsing does not take the only control
  // that could undo it — the `aria-expanded` pair is how a state says which
  // side of that toggle it is on.
  groupCard: ".flow-card--concurrent",
  groupMemberCard: ".flow-card--member",
  groupMembers: ".member-list",
  groupMemberRow: ".member-row",
  groupFold: ".member-collapse",
  groupFoldOpen: '.member-collapse[aria-expanded="true"]',
  groupFoldShut: '.member-collapse[aria-expanded="false"]',
  pausedBadge: ".paused-badge",
  // The step log below the graph: the region itself, the head it grows once a
  // step is selected, and the invitation it draws when none is. It is on every
  // overlay screen, so "nothing selected" is a state rather than an absence —
  // a blank strip and a failed render look identical in a screenshot.
  stepLog: ".step-log",
  stepLogIdle: ".step-log--idle",
  stepLogHead: ".step-log-head",
  stepLogEmpty: ".step-log-empty",
  legend: ".legend",
  stepCard: ".flow-card",
  terminalPill: ".terminal-pill",

  editorShell: ".editor-shell",
  editorTabs: ".editor-tabs",
  editorTabPipeline: '[data-testid="editor-tab-pipeline"]',
  editorTabRelations: '[data-testid="editor-tab-relations"]',
  editorToolbar: ".editor-toolbar",
  editorStatus: ".editor-status",
  // The failure treatment specifically. Without it a refusal drawn in the same
  // muted grey as "saved" satisfies every `shows` this state has, which is how
  // it went unremarked: the words were right and the weight said nothing
  // happened.
  editorStatusError: ".editor-status--error",
  editorPanel: ".editor-panel",
  editorEmpty: ".editor-empty",
  editorAddKind: '[aria-label="New step kind"]',
  editorAddStep: '[data-testid="editor-add-step"]',
  editorSave: '[data-testid="editor-save"]',
  editorDiscard: '[data-testid="editor-discard"]',
  editorCard: ".editor-card",
  editorTerminal: ".editor-terminal",
  editorStepName: '[data-testid="editor-step-name"]',
  editorMaxAttempts: '[data-testid="editor-max-attempts"]',
  editorDeleteStep: '[data-testid="editor-delete-step"]',
  editorDeleteConfirm: '[data-testid="editor-delete-confirm"]',
  editorDangerZone: ".editor-danger-zone",
  editorIssues: ".editor-issues",
  // The reverted panel specifically, not the hint list it shares a class with.
  // `.editor-issues` draws both, so asserting it cannot tell "the editor has
  // advice about this draft" apart from a write the host refused.
  editorSaveReverted: '[data-testid="editor-save-reverted"]',
  editorHints: ".editor-hints",
  editorBack: '[data-testid="editor-back"]',
  editorMissing: ".editor-view .status",
  stepRole: '[aria-label="Step role"]',
  stepTrust: '[aria-label="Minimum trust"]',
};

/** One step card in the editor, addressed by the step it draws. */
export function editorCardFor(step) {
  return `.editor-card[data-ref="${step}"]`;
}

/**
 * One step card in the pipeline *viewer*, addressed by the step it draws.
 *
 * The viewer's cards are React Flow nodes rather than the editor's own list, so
 * the step name is on the node wrapper React Flow renders and not on the card.
 * Selecting one is how a reader reads a step's log, which is the only way to
 * reach a log for a step the run has not written to.
 */
export function viewerCardFor(step) {
  return `.react-flow__node[data-id="${step}"] .flow-card`;
}

/**
 * One card in the relation graph, addressed by the config item it draws.
 * Unreferenced config is still config, so this is what holds the graph to
 * drawing an orphan rather than dropping it.
 */
export function relationCardFor(id) {
  return `.relation-card[data-ref="${id}"]`;
}

/**
 * The replay timeline, addressed by the run it is spanning. `max` comes
 * straight from the last event in the selected run's log, so this is the one
 * selector that ties a replay render to the run it claims to be showing.
 */
export function replaySpanFor(endMs) {
  return `.replay-scrubber[max="${endMs}"]`;
}

/** One speed button, and the one the timeline is actually running at. */
export function replaySpeed(rate) {
  return `[data-testid="replay-speed-${rate}"]`;
}

export function replaySpeedActive(rate) {
  return `${replaySpeed(rate)}.run-control--active`;
}

/**
 * The scrubber, addressed by the position it is parked at. The transport moves
 * `value`, so this is what separates "stepped forward" and "still at the
 * start" — a step that did nothing leaves the readout and the scrubber where
 * they were.
 */
export function replayPositionAt(ms) {
  return `.replay-scrubber[value="${ms}"]`;
}

/** A field editor that is publishing unsaved work, and one that is not. */
export function dirtyEditor(editor) {
  return `${editor}[data-dirty="true"]`;
}

export function cleanEditor(editor) {
  return `${editor}[data-dirty="false"]`;
}

/** The unsaved-changes marker, scoped to the editor that drew it. */
export function draftMarkIn(editor) {
  return `${editor} .draft-mark`;
}

/** A save button that is offered, and one that is withheld. */
export function offered(save) {
  return `${save}:not([disabled])`;
}

export function withheld(save) {
  return `${save}[disabled]`;
}

/** The viewport sizes the design system records as its one breakpoint. */
export const VIEWPORTS = {
  desktop: { id: "desktop", width: 1280, height: 900, summary: "above the 56rem breakpoint" },
  compact: { id: "compact", width: 760, height: 900, summary: "at or below the 56rem breakpoint" },
};
