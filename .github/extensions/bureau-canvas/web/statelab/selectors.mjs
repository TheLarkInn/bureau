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
  assignmentHead: ".assignment-head",
  assignmentDetail: ".assignment-detail",
  assignmentEmpty: ".view-shell--config > .muted",

  createOpen: '[data-testid="create-open"]',
  createBar: '[data-testid="create-bar"]',
  createKind: "#create-kind",
  createName: "#create-name",
  createSubmit: '[data-testid="create-submit"]',
  createCancel: '[data-testid="create-cancel"]',

  draftBar: '[data-testid="draft-bar"]',
  draftSave: '[data-testid="draft-save"]',
  draftDiscard: '[data-testid="draft-discard"]',
  draftList: ".draft-list",

  workSourceValue: ".ws-value",
  workSourceEditor: ".ws-open",
  workSourceUrl: '[aria-label="Board, query, or issues URL"]',
  workSourceDerived: ".derived",

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
  repoRow: ".repo-row",

  limitsValue: ".limits-value",
  limitsEditor: ".limits-editor",
  limitsSave: '[data-testid="limits-save"]',
  limitsDirty: ".limits-dirty",
  limitRow: ".limit-row",

  deleteStart: '[data-testid="delete-start"]',
  deleteConfirm: '[data-testid="delete-confirm"]',
  preflight: '[data-testid="preflight"]',

  orphanStrip: ".orphan-strip",
  relationSection: ".relation-section",
  relationSummary: ".relation-section > summary",
  relationFlow: ".relation-flow",

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
  replayControls: ".replay-controls",
  replayTimeline: ".replay-timeline",
  legend: ".legend",
  stepCard: ".flow-card",
  terminalPill: ".terminal-pill",

  editorShell: ".editor-shell",
  editorTabs: ".editor-tabs",
  editorTabPipeline: '[data-testid="editor-tab-pipeline"]',
  editorTabRelations: '[data-testid="editor-tab-relations"]',
  editorToolbar: ".editor-toolbar",
  editorStatus: ".editor-status",
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

/** The viewport sizes the design system records as its one breakpoint. */
export const VIEWPORTS = {
  desktop: { id: "desktop", width: 1280, height: 900, summary: "above the 56rem breakpoint" },
  compact: { id: "compact", width: 760, height: 900, summary: "at or below the 56rem breakpoint" },
};
