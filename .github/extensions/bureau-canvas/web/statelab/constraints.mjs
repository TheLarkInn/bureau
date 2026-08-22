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
import { PAIRABLE_FIELDS } from "./dimensions.mjs";
const BOOT_DATA = ["loading", "render-error"];
const BOOT_SURFACES = ["boot", "boot-editor"];
const INDEX_SURFACES = ["config", "pipeline"];
/** Findings that hang off an assignment, so only the stack can draw them. */
const ASSIGNMENT_SCOPED_DATA = ["advisory", "invalid-advisory"];

/** Dimensions that must read `n/a` when the surface has no such region. */
const REGIONS = ["draft", "section", "orphans", "disclosure", "card", "field", "fieldState", "fieldPair", "mode", "run", "tab", "pick", "edit"];

/** The body values that count as "at rest" for the scoping rules. */
const BODY_BASELINE = {
  section: ["n/a", "stack"],
  orphans: ["n/a", "none"],
  disclosure: ["n/a", "none"],
  card: ["n/a", "collapsed"],
  field: ["n/a"],
  fieldState: ["n/a"],
  fieldPair: ["n/a"],
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

/**
 * The two lifecycle values that only a performed save can reach. Shared by the
 * draft, field and editor axes because it is one fact about the harness, not
 * three: each save mutates the host — the plan bar writes the config directory,
 * the pipeline editor writes the pipeline file, a field records a plan on the
 * shared instance — and the matrix host is read-only and worker-scoped.
 */
const SAVE_STATES = ["saving", "save-error"];

export const CONSTRAINTS = [
  {
    /*
     * A field save is the one act on a card that leaves the state alone but
     * not the host. `set-work-source` and its siblings do not write the config
     * directory — `lib/crud.mjs` `record()` only calls `setPlan` and returns a
     * `pending` summary, which `specs/assignment-runtime.spec.mjs` pins by
     * reading the file back unchanged. What they do is record a plan on the
     * host, and `serve.mjs` boots one instance per host process while
     * `extension.mjs` keys plans on it. The matrix runs every state against one
     * such host per worker, fully parallel, so a plan recorded by one state
     * outlives it and raises the draft bar over every later state on that
     * worker — including the ones whose `draft: none` requires it absent.
     *
     * So the values exist and the rule excludes them, rather than the axis
     * quietly stopping at `invalid`: `saving` and `save-error` are real screens
     * — the "Saving…" label, `.note--err` from a refused `set-work-source` —
     * and an axis that omitted them would be the unrepresentable-versus-
     * excluded mistake this registry keeps making rules for. The successful
     * round trip is owned per field by `specs/worksource.spec.mjs`,
     * `specs/limits.spec.mjs`, `specs/repos.spec.mjs` and
     * `specs/assignment-runtime.spec.mjs`, each against its own scratch config.
     */
    id: "a-field-save-would-write-the-config",
    kind: "structural",
    reads: ["fieldState"],
    title: "The matrix cannot review a field save it is not allowed to perform",
    why: "A `set-*` intent records a pending plan on the shared host (`lib/crud.mjs` `record` → `setPlan`), and the matrix shares one worker-scoped host across every state — so the plan would leak the draft bar into every later state on that worker. `saving` and `save-error` are real screens the harness may not produce; `specs/worksource.spec.mjs`, `specs/limits.spec.mjs`, `specs/repos.spec.mjs` and `specs/assignment-runtime.spec.mjs` own the round trip against their own scratch configs.",
    holds: (combo) => !SAVE_STATES.includes(combo.fieldState),
  },
  {
    /*
     * The draft bar's own two buttons, and the only one of the three rules in
     * this family whose act really does reach the disk: `save-plan` runs
     * `applyPlan` against the host's config directory (`extension.mjs`), and
     * the matrix host is pointed at the repository's own `.bureau/`. Discard
     * is no safer to perform, because it clears the plan the fixture published
     * and the states sharing that worker are still being judged against it.
     */
    id: "a-plan-save-would-write-the-config",
    kind: "structural",
    reads: ["draft"],
    title: "The matrix cannot review a plan save it is not allowed to perform",
    why: "`save-plan` calls `applyPlan` on the host's config directory and `discard-plan` clears the shared plan, and the matrix runs every state against one read-only host per worker pointed at the repository's own `.bureau/`. The in-flight and refused bars are real renders of `DraftBar` and are excluded here for that reason, not absent; `specs/controls.spec.mjs` and `specs/worksource.spec.mjs` walk the successful save and discard against their own scratch configs.",
    holds: (combo) => !SAVE_STATES.includes(combo.draft),
  },
  {
    id: "an-editor-save-would-write-the-config",
    kind: "structural",
    reads: ["edit"],
    title: "The matrix cannot review a pipeline save it is not allowed to perform",
    why: "`save-pipeline` writes the pipeline file, re-runs `bureau validate --json` and reverts on a finding — three writes to the shared host's config. The in-flight and reverted-with-findings screens are real and excluded here for that reason, not absent; `specs/editor.spec.mjs` walks the round trip against a scratch config and `test/pipeline-roundtrip.test.mjs` owns the revert-on-findings half offline.",
    holds: (combo) => !SAVE_STATES.includes(combo.edit),
  },
  {
    id: "boot-is-pre-surface",
    kind: "structural",
    reads: ["surface", "data"],
    title: "Boot is the only surface without data",
    why: "`loading` and `render-error` are what a page shows *instead of* a surface; a mounted surface always has a payload. Both pages boot, so both have a boot surface.",
    holds: (combo) => BOOT_SURFACES.includes(combo.surface) === BOOT_DATA.includes(combo.data),
  },
  {
    id: "a-config-status-needs-a-surface-that-reports-it",
    kind: "structural",
    reads: ["surface", "data"],
    title: "Only the surfaces that draw a status line have a data status",
    why: "`Header` and `Findings` are `App` chrome, which editor.html does not mount: it draws its own header and reports findings through the save result. A config status has nowhere to appear there, so the axis is absent rather than merely uninteresting — and `n/a` records that instead of an unasserted render.",
    holds: (combo) => (combo.data === "n/a") === (combo.surface === "editor"),
  },
  {
    id: "an-advisory-sits-on-the-item-it-names",
    kind: "structural",
    reads: ["surface", "data"],
    title: "An advisory is only visible where the item it names is drawn",
    why: "`lib/advisories.mjs` targets an assignment, and a finding is attached to the item it names. The pipeline viewer draws no assignment cards, so an advisory there has nothing to sit on and the render is indistinguishable from a clean one.",
    holds: (combo) => !ASSIGNMENT_SCOPED_DATA.includes(combo.data) || combo.surface === "config",
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
    id: "the-orphan-strip-is-a-landing-region",
    kind: "structural",
    reads: ["surface", "orphans"],
    title: "Unreferenced config is surfaced only on the config landing",
    why: "`OrphanStrip` is a child of `ConfigView`, drawn beneath the stack. The pipeline viewer and the editor never render it, so there is nothing there for it to say.",
    holds: (combo) => (combo.orphans !== "n/a") === (combo.surface === "config"),
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
    why: "`dirty` and `invalid` describe an open editor's draft. Delete is a confirmation and carries none; the repo adder's save state belongs to the repo list it returns to. The work source has both: a pasted URL the deriver refuses leaves the draft open, the preview gone and the save withheld.",
    holds: (combo) => lifecycleAllows(combo.field, combo.fieldState),
  },
  {
    id: "a-preflight-answers-with-the-hosts-own-config",
    kind: "structural",
    reads: ["field", "section"],
    title: "The delete preflight cannot be reviewed over an injected config",
    why: "Opening the preflight is a real intent, and `runCrudIntent` answers even a read-only one by refreshing and republishing the host's own state — which replaces the injected payload outright. The host serves a single-assignment sample, so a second card cannot survive to be reviewed here. This is an exclusion rather than a suppressed axis on purpose: suppressing would let the host's one-card screen pass under the name `two-cards`.",
    holds: (combo) => combo.field !== "delete" || combo.section !== "two-cards",
  },
  {
    id: "a-second-field-needs-a-first",
    kind: "structural",
    reads: ["field", "fieldPair"],
    title: "A second disclosure needs a first one to sit beside",
    why: "The axis says whether another field editor is open *as well*. With every field at rest there is no first, so there is nothing to be second to. Delete is excluded because it is not a field editor but a preflight, and answering it makes the host republish its own state over the payload the pair was assembled from.",
    holds: (combo) => (combo.fieldPair !== "n/a") === PAIRABLE_FIELDS.includes(combo.field),
  },
  {
    id: "the-second-disclosure-is-not-the-first",
    kind: "structural",
    reads: ["field", "fieldPair"],
    title: "The second editor cannot be the one already under review",
    why: "`second-open` is a concrete screen, not an abstraction: it opens the limits disclosure beside whatever field is being reviewed. With limits itself under review there is no second editor, only the first counted twice.",
    holds: (combo) => combo.fieldPair !== "second-open" || combo.field !== "limits",
  },
  {
    id: "a-second-open-field-is-probed-not-crossed",
    kind: "scoping",
    reads: ["fieldPair"],
    title: "Two open field editors are reviewed once, not once per pair",
    why: "Each disclosure owns its open state, so opening one leaves the others exactly as they were — which is why the pair is a real screen and gets an axis rather than being quietly unrepresentable. What it is not is one screen per pair: the fields write into separate subtrees and share no state, so every pair shows the same fact — two disclosures coexisting in one column without printing over each other. `probe--two-disclosures-open` renders the crossing and is what holds the claim to account.",
    holds: (combo) => combo.fieldPair !== "second-open",
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
    id: "a-move-needs-a-step-the-fixture-already-draws",
    kind: "structural",
    reads: ["edit", "pick"],
    title: "Only an existing step can be moved and nothing else",
    why: "`layout-moved` is the one edit that changes nothing but a position, which is why its status may read a clean `unsaved edits`. A decision or concurrent step has to be *added* before it can be dragged, so the state there would be `created` and then moved — a different thing, already enumerated, and one whose new step is unwired and reports an issue.",
    holds: (combo) => combo.edit !== "layout-moved" || Boolean(SAMPLE_STEPS[combo.pick]),
  },
  {
    id: "chrome-is-orthogonal-to-body",
    kind: "scoping",
    reads: ["data", ...BODY],
    title: "Data status is enumerated against a resting body",
    why: "`Header`, `Findings` and the index.html fallback render into subtrees that share no state with the card body, the graph or the editor. Crossing them multiplies renders without adding information; the crossings that could still interact through layout are rendered by `probes.mjs`. `n/a` is exempt because there is no status region to cross — the editor draws none.",
    holds: (combo) => ["n/a", "validated"].includes(combo.data) || bodyAtRest(combo),
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
    reads: ["orphans", "card"],
    title: "The orphan strip is reviewed against a resting card",
    why: "`OrphanStrip` is a sibling of the stack with its own local state, so crossing unreferenced config with an open card multiplies screenshots without adding information. Its layout interaction with a tall card is a crossing probe instead.",
    holds: (combo) => combo.orphans !== "present" || ["n/a", "collapsed"].includes(combo.card),
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
