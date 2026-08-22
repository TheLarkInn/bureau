// Why a combination of dimension values is or is not a real Bureau Canvas
// state.
//
// Every exclusion in the matrix is attributable to exactly one named rule
// here. Nothing is dropped quietly: `registry.mjs` walks the whole Cartesian
// product past this list and reports, per rule, how many tuples that rule was
// the first to reject and a worked prune point. First to reject is not the
// same as "only rule that would have rejected" — see `enumerate.mjs` — so the
// per-rule figure is read as "this much of the walk was cut here", while
// `violations()` below answers the order-free question for a given tuple.
//
// Each rule declares the dimensions it reads. That lets the enumeration prune
// a whole subtree the moment a rule's inputs are all assigned, instead of
// materialising 653 million tuples to throw nearly all of them away.
//
// Two kinds of rule live here, and the distinction matters:
//
//   structural — the combination cannot be rendered at all, because the
//                controls that would produce it do not exist on that surface.
//   scoping    — the combination is renderable, but the two axes draw into
//                subtrees that share no React state, so crossing them
//                multiplies screenshots without adding information. Every
//                scoping rule is paired with a crossing probe in `probes.mjs`
//                that renders the cross anyway and holds the claim to account.

import { FIELD_LIFECYCLE, SAMPLE_STEPS } from "./paths.mjs";
const BOOT_DATA = ["loading", "render-error"];
const BOOT_SURFACES = ["boot", "boot-editor"];
const INDEX_SURFACES = ["config", "pipeline"];

/** Dimensions that must read `n/a` when the surface has no such region. */
const REGIONS = ["draft", "section", "disclosure", "card", "field", "fieldState", "mode", "run", "tab", "pick", "edit"];

/** The body values that count as "at rest" for the scoping rules. */
const BODY_BASELINE = {
  section: ["n/a", "stack"],
  disclosure: ["n/a", "none"],
  card: ["n/a", "collapsed"],
  field: ["n/a"],
  fieldState: ["n/a"],
  mode: ["n/a", "design"],
  run: ["n/a", "none"],
  tab: ["n/a", "pipeline"],
  pick: ["n/a", "none"],
  edit: ["n/a", "rest"],
};

const BODY = Object.keys(BODY_BASELINE);

function bodyAtRest(combo) {
  return BODY.every((dimension) => BODY_BASELINE[dimension].includes(combo[dimension]));
}

function na(combo, dimensions) {
  return dimensions.every((dimension) => combo[dimension] === "n/a");
}

/** Selection-bearing mutations: these controls exist only for a selected step. */
const NEEDS_SELECTION = ["created", "renamed", "delete-confirm", "invalid", "layout-moved"];

export const CONSTRAINTS = [
  {
    id: "boot-is-pre-surface",
    kind: "structural",
    reads: ["surface", "data"],
    title: "Boot is the only surface without data",
    why: "`loading` and `render-error` are what a page shows *instead of* a surface; a mounted surface always has a payload. Both pages boot, so both have a boot surface.",
    holds: (combo) => BOOT_SURFACES.includes(combo.surface) === BOOT_DATA.includes(combo.data),
  },
  {
    id: "boot-has-no-regions",
    kind: "structural",
    reads: ["surface", ...REGIONS],
    title: "Boot renders no regions",
    why: "Before the renderer mounts there is no draft bar, no card and no tab to be in a state.",
    holds: (combo) => !BOOT_SURFACES.includes(combo.surface) || na(combo, REGIONS),
  },
  {
    id: "draft-bar-is-index-only",
    kind: "structural",
    reads: ["surface", "draft"],
    title: "The draft bar belongs to index.html",
    why: "`DraftBar` renders inside `App`, which editor.html does not mount; the editor proposes through `save-pipeline` instead.",
    holds: (combo) => (combo.draft !== "n/a") === INDEX_SURFACES.includes(combo.surface),
  },
  {
    id: "config-owns-the-landing",
    kind: "structural",
    reads: ["surface", "section"],
    title: "Sections exist only on the config surface",
    why: "The assignment stack, the create bar, the orphan strip and the relation disclosure are all children of `ConfigView`.",
    holds: (combo) => (combo.section !== "n/a") === (combo.surface === "config"),
  },
  {
    id: "disclosure-is-a-landing-region",
    kind: "structural",
    reads: ["surface", "disclosure"],
    title: "Landing disclosures exist only on the config surface",
    why: "`CreateBar` and `RelationSection` are siblings of the stack inside `ConfigView`; no other surface mounts them.",
    holds: (combo) => (combo.disclosure !== "n/a") === (combo.surface === "config"),
  },
  {
    id: "an-empty-landing-has-no-card",
    kind: "structural",
    reads: ["surface", "section", "card"],
    title: "An empty landing has no card",
    why: "With no assignments configured there is nothing to collapse or expand — the landing says so in copy instead.",
    holds: (combo) => (combo.card === "n/a") === (combo.surface !== "config" || combo.section === "empty"),
  },
  {
    id: "fields-need-an-open-card",
    kind: "structural",
    reads: ["card", "field"],
    title: "Field editors open inside an expanded card",
    why: "Assignment-first: every field disclosure is a child of `AssignmentDetail`, which exists only while the card is expanded.",
    holds: (combo) => (combo.field !== "n/a") === (combo.card === "expanded"),
  },
  {
    id: "field-declares-its-lifecycle",
    kind: "structural",
    reads: ["field", "fieldState"],
    title: "A field only has the lifecycle states it can actually reach",
    why: "`dirty` and `invalid` describe an open editor's draft. Delete is a confirmation and carries none; the work source derives from a pasted URL, so it has no invalid-but-accepted draft; the repo adder's save state belongs to the repo list it returns to.",
    holds: (combo) => lifecycleAllows(combo.field, combo.fieldState),
  },
  {
    id: "mode-is-pipeline-only",
    kind: "structural",
    reads: ["surface", "mode"],
    title: "Graph modes belong to the pipeline viewer",
    why: "`ModeSwitcher` renders in the pipeline toolbar; the config landing and the editor have no run overlay.",
    holds: (combo) => (combo.mode !== "n/a") === (combo.surface === "pipeline"),
  },
  {
    id: "run-needs-an-overlay-mode",
    kind: "structural",
    reads: ["mode", "run"],
    title: "A run selection needs live or replay",
    why: "Design mode draws the config graph and never consults the run log, so it has no run to select.",
    holds: (combo) => (combo.run !== "n/a") === ["live", "replay"].includes(combo.mode),
  },
  {
    id: "live-cannot-show-a-finished-run",
    kind: "structural",
    reads: ["mode", "run"],
    title: "The live picker lists only live runs",
    why: "`RunPicker` filters on `run.live` in live mode, so a finished run is not selectable there — replay is where finished runs are read.",
    holds: (combo) => combo.mode !== "live" || combo.run !== "finished",
  },
  {
    id: "tabs-are-editor-only",
    kind: "structural",
    reads: ["surface", "tab"],
    title: "Tabs belong to editor.html",
    why: "The Pipeline/Relations tab strip is `EditorApp` chrome; index.html navigates by graph instead.",
    holds: (combo) => (combo.tab !== "n/a") === (combo.surface === "editor"),
  },
  {
    id: "selection-needs-the-pipeline-tab",
    kind: "scoping",
    reads: ["tab", "pick"],
    title: "A step selection is reviewed on the tab that draws it",
    why: "The Relations tab is the shared read-only relation renderer and has no step of its own to select. But `EditorApp` keeps `PipelineEditor` mounted and merely `hidden` while Relations is showing, so a selection made on the Pipeline tab does survive the switch — it is simply not on screen. Crossing every selection with the Relations tab therefore repeats one screenshot of the relation graph, so the crossing is rendered by a probe instead, which is also what holds that survival to account.",
    holds: (combo) => (combo.pick !== "n/a") === (combo.tab === "pipeline"),
  },
  {
    id: "mutation-needs-the-pipeline-tab",
    kind: "scoping",
    reads: ["tab", "edit"],
    title: "An edit is reviewed on the tab that draws it",
    why: "Editing happens in the pipeline editor and the existing forms — never on the relation graph. The editor stays mounted behind the Relations tab, though, so an unsaved edit is held rather than discarded: draft safety depends on it. Nothing of that draft is visible while Relations is showing, so the crossing is a probe rather than a row of identical renders.",
    holds: (combo) => (combo.edit !== "n/a") === (combo.tab === "pipeline"),
  },
  {
    id: "mutations-need-a-selected-step",
    kind: "structural",
    reads: ["edit", "pick"],
    title: "Add, rename, delete, move and invalid all act on a selection",
    why: "Those controls live in the side panel's step editor, which renders only for a selected step; adding a step selects it on the spot.",
    holds: (combo) => !NEEDS_SELECTION.includes(combo.edit) || !["n/a", "none"].includes(combo.pick),
  },
  {
    id: "a-clean-editor-can-only-select-what-it-already-draws",
    kind: "structural",
    reads: ["edit", "pick"],
    title: "Selecting a decision or a concurrent step means creating one",
    why: "The fixture pipeline ships a deterministic and an agent step. Reaching a decision or concurrent selection requires adding one, and adding is a mutation — so those kinds cannot be selected while the editor is still clean.",
    holds: (combo) => combo.edit !== "rest" || ["n/a", "none", ...Object.keys(SAMPLE_STEPS)].includes(combo.pick),
  },
  {
    id: "chrome-is-orthogonal-to-body",
    kind: "scoping",
    reads: ["data", ...BODY],
    title: "Data status is enumerated against a resting body",
    why: "`Header`, `Findings` and the index.html fallback render into subtrees that share no state with the card body, the graph or the editor. Crossing them multiplies renders without adding information; the crossings that could still interact through layout are rendered by `probes.mjs`.",
    holds: (combo) => combo.data === "validated" || bodyAtRest(combo),
  },
  {
    id: "draft-is-orthogonal-to-body",
    kind: "scoping",
    reads: ["draft", ...BODY],
    title: "The draft bar is enumerated against a resting body",
    why: "`DraftBar` is a sibling section with its own local state; it neither reads nor writes any field editor. Its layout interaction with an open card is covered by a crossing probe rather than by the product.",
    holds: (combo) => ["n/a", "none"].includes(combo.draft) || bodyAtRest(combo),
  },
  {
    id: "one-body-variation-at-a-time",
    kind: "scoping",
    reads: ["section", "card"],
    title: "The orphan strip is reviewed against a resting stack",
    why: "`OrphanStrip` is a sibling of the stack with its own local state, so crossing unreferenced config with an open card multiplies screenshots without adding information. Its layout interaction with a tall card is a crossing probe instead.",
    holds: (combo) => ["n/a", "stack", "empty", "two-cards"].includes(combo.section) || combo.card === "collapsed",
  },
  {
    id: "a-disclosure-is-reviewed-against-a-resting-card",
    kind: "scoping",
    reads: ["disclosure", "card"],
    title: "A landing disclosure is reviewed against a resting card",
    why: "`CreateBar` and `RelationSection` keep their own local open state and share none with a card, so crossing every disclosure with every card state repeats one screenshot. Both crossings are probed, because the three regions are stacked in one column and layout is not state.",
    holds: (combo) => ["n/a", "none"].includes(combo.disclosure) || ["n/a", "collapsed"].includes(combo.card),
  },
];

function lifecycleAllows(field, fieldState) {
  if (field === "n/a" || field === "none") {
    return fieldState === "n/a";
  }
  return Boolean(FIELD_LIFECYCLE[field]?.[fieldState]);
}

export const CONSTRAINT_IDS = CONSTRAINTS.map((rule) => rule.id);

/** Every rule a fully-assigned combination breaks. Empty means reachable. */
export function violations(combo) {
  return CONSTRAINTS.filter((rule) => !rule.holds(combo)).map((rule) => rule.id);
}

/** Rules whose every input is assigned, so a pruned walk may apply them. */
export function rulesReadyFor(assigned) {
  return CONSTRAINTS.filter((rule) => rule.reads.every((dimension) => assigned.has(dimension)));
}
