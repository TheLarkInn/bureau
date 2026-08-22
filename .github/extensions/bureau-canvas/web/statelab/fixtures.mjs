// Deterministic offline fixtures for the state lab.
//
// Every fixture is a pure function over the state the host already served, so
// the lab and the browser suite render the *real* payload shape rather than a
// hand-written imitation that can drift from `buildState`. Nothing here reads
// the network, spawns a process, or calls a model: a fixture is a projection
// of `GET /state` and nothing else.
//
// Fixtures compose, in four layers, and a state names at most one from each:
//
//   status    what `bureau validate` said        validated / invalid / …
//   content   what the config contains           empty / two-assignments / …
//   plan      what is unsaved                    draft-pending / …
//   selection which pipeline is open             pipeline / pipeline-missing
//
// Composition is what keeps states distinguishable: without it, "a pipeline
// viewer with findings" and "a pipeline viewer without" would resolve to the
// same payload and the matrix would quietly render one state twice.

/** Deep clone that keeps the payload a plain JSON value, like the wire does. */
function clone(state) {
  return JSON.parse(JSON.stringify(state));
}

function assignment(state, name = "agent-eligible") {
  return state.config.view.assignments.find((item) => item.name === name)
    ?? state.config.view.assignments[0];
}

// --- status layer ---------------------------------------------------------

/** The served sample, unaltered: no binary, and the status says so. */
const sample = (state) => clone(state);

function validated(state) {
  const next = clone(state);
  next.validation = { ...next.validation, state: "validated", ok: true, errors: [], message: null };
  next.status = "Validated";
  return next;
}

/**
 * A config the CLI rejected. Findings are attached the way `buildState` groups
 * them, so the card, the general strip and the pipeline panel all light up.
 */
function invalid(state) {
  const next = validated(state);
  const findings = [
    {
      severity: "error",
      message: "assignment `agent-eligible` names pipeline `agent-eligible-pipeline`, whose step `verify` has no `run`",
      target: { kind: "assignment", assignment: "agent-eligible" },
    },
    {
      severity: "error",
      message: "step `verify` must set `run` for a deterministic step",
      target: { kind: "step", pipeline: "agent-eligible-pipeline", step: "verify" },
    },
    { severity: "error", message: "repos.yaml: `bureau` has no credential" },
  ];
  next.validation = { ...next.validation, ok: false, errors: findings.map((item) => item.message) };
  next.status = "Validation findings";
  next.findings = findings;
  next.findingsByItem = {
    "assignment:agent-eligible": [findings[0]],
    "pipeline:agent-eligible-pipeline": [findings[1]],
  };
  next.findingsByStep = { "pipeline:agent-eligible-pipeline/verify": [findings[1]] };
  next.generalFindings = [findings[2]];
  return next;
}

/**
 * The advisory class: never blocking, always visually distinct from an error.
 *
 * The shape matters. `findingClass` in `web/app.mjs` classifies on `source`
 * and `marker` — `severity` is not read anywhere in the web layer — so a
 * fixture that carried only a severity would paint the advisory in the
 * validation-error red and the one state whose whole point is the distinction
 * would prove the opposite. This is `lib/advisories.mjs`'s own shape.
 */
function advisoryNote() {
  return {
    source: "advisory",
    marker: "unchecked-write",
    path: ".bureau/pipelines/agent-eligible-pipeline.yaml",
    message: "agent step `implement` can write to the repo and nothing downstream checks it",
    target: { kind: "assignment", assignment: "agent-eligible" },
  };
}

function advisory(state) {
  const next = validated(state);
  const note = advisoryNote();
  next.findings = [note];
  next.findingsByItem = { "assignment:agent-eligible": [note] };
  return next;
}

/**
 * Both classes at once. `mergeAdvisories` in `lib/actions.mjs` concatenates
 * advisories onto whatever `validate` returned, so a config really can hold
 * errors and advisories together — and adjacency is precisely when the two
 * treatments have to stay distinguishable. Leaving `data` a single-valued
 * choice between them would have made this pair unrepresentable rather than
 * excluded, which is the one thing the registry may not do.
 */
function invalidAdvisory(state) {
  const next = invalid(state);
  const note = advisoryNote();
  const onAssignment = next.findingsByItem["assignment:agent-eligible"] ?? [];
  next.findings = [...next.findings, note];
  next.findingsByItem = { ...next.findingsByItem, "assignment:agent-eligible": [...onAssignment, note] };
  return next;
}

// --- content layer --------------------------------------------------------

/** Nothing configured yet: the landing has to say so rather than render blank. */
function empty(state) {
  const next = clone(state);
  next.config.view = { ...next.config.view, assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
  next.config.relation = { nodes: [], edges: [] };
  next.pipelines = {};
  return next;
}

/** Config nothing references, which the strip surfaces without graph noise. */
function orphans(state) {
  const next = clone(state);
  next.config.view = {
    ...next.config.view,
    orphans: [
      { kind: "role", name: "reviewer" },
      { kind: "pipeline", name: "retired-pipeline" },
    ],
  };
  return next;
}

/** A second assignment, so the stack is a stack and not a single card. */
function twoAssignments(state) {
  const next = clone(state);
  const extra = clone(assignment(next));
  extra.name = "docs-triage";
  extra.work = { ...extra.work, source: "TheLarkInn/bureau-docs", filter: "is:open label:docs" };
  next.config.view.assignments = [...next.config.view.assignments, extra];
  return next;
}

/**
 * A second repo that can itself take a branch, so *reordering* is a valid edit
 * rather than one the editor has to refuse. A read-only second repo would make
 * every reorder illegal, and `repos: dirty` would render the refusal instead of
 * the offered save it claims.
 */
function multiRepo(state) {
  const next = clone(state);
  next.config.view.repos = [
    ...next.config.view.repos,
    { name: "bureau-docs", url: "https://github.com/TheLarkInn/bureau-docs.git", forge: "github", access: "pr", credential: "github-main" },
  ];
  assignment(next).repos = ["bureau", "bureau-docs"];
  return next;
}

/**
 * The primary repo is registered read-only, so no branch can land there.
 *
 * The order stays the one `multi-repo` ships, because the refusal has to be
 * the *result of the edit*: with the assignment already reordered, `Save`
 * would be disabled by `sameOrder` alone and the assertion would hold even if
 * the read-only gate were deleted outright.
 */
function readOnlyPrimary(state) {
  const next = multiRepo(state);
  // Stated here rather than inherited: `multi-repo` grants `pr` on purpose, so
  // the read-only grant is this fixture's own claim and survives a change there.
  const docs = next.config.view.repos.find((repo) => repo.name === "bureau-docs");
  docs.access = "read";
  return next;
}

/** The primary repo is not in repos.yaml at all — a different fix entirely. */
function unknownPrimary(state) {
  const next = clone(state);
  assignment(next).repos = ["not-registered", "bureau"];
  return next;
}

/** No repo carries a credential, so registering a new one has to refuse. */
function noCredential(state) {
  const next = clone(state);
  next.config.view.repos = next.config.view.repos.map((repo) => ({ ...repo, credential: null }));
  return next;
}

/** Every limit off: the summary must say unbounded rather than fall silent. */
function noLimits(state) {
  const next = clone(state);
  assignment(next).limits = {};
  return next;
}

/** Every limit capped, including the one whose absence is a default. */
function allLimits(state) {
  const next = clone(state);
  assignment(next).limits = {
    maxConcurrent: 2,
    maxRunsPerHour: 4,
    maxRunsPerDay: 20,
    maxOpenPrs: 5,
    maxCostPerDayUsd: 25,
    maxRunHours: 12,
  };
  return next;
}

/** An assignment with no forge signals configured yet. */
function noSignals(state) {
  const next = clone(state);
  const item = assignment(next);
  item.work = { ...item.work, abortLabel: "", escalateLabel: "", approvalLabel: null };
  return next;
}

// --- plan layer -----------------------------------------------------------

/** Unsaved work: the draft bar must read as unsaved and stay discardable. */
function draftPending(state) {
  const next = clone(state);
  next.plan = {
    writes: [".bureau/assignments/agent-eligible.yaml", ".bureau/pipelines/agent-eligible-pipeline.yaml"],
    removals: [".bureau/roles/reviewer.yaml"],
  };
  return next;
}

/** One pending write, so the singular copy is exercised as well as the plural. */
function draftSingle(state) {
  const next = clone(state);
  next.plan = { writes: [".bureau/roles/implementer.yaml"], removals: [] };
  return next;
}

// --- selection layer ------------------------------------------------------

function selectPipeline(state) {
  const next = clone(state);
  const name = Object.keys(next.pipelines ?? {})[0] ?? "agent-eligible-pipeline";
  next.selectedPipeline = { name, missing: false };
  next.pipeline = name;
  return next;
}

/** The pipeline the URL names is gone: the surface must say so, not blank. */
function missingPipeline(state) {
  const next = clone(state);
  const name = "deleted-pipeline";
  next.selectedPipeline = { name, missing: true, notice: `No pipeline named \`${name}\` in this config.` };
  next.pipeline = name;
  return next;
}

/** No pipeline chosen at all, which the editor page has to explain. */
function noPipeline(state) {
  const next = clone(state);
  next.selectedPipeline = null;
  next.pipeline = null;
  return next;
}

function entry(id, layer, summary, build) {
  return [id, { id, layer, summary, build }];
}

export const FIXTURES = Object.fromEntries([
  entry("sample", "status", "the bundled sample exactly as the host serves it", sample),
  entry("validated", "status", "bureau validate ran and accepted the config", validated),
  entry("invalid", "status", "bureau validate rejected it; findings sit on what they name", invalid),
  entry("advisory", "status", "an advisory that must never block a save", advisory),
  entry("invalid-advisory", "status", "validation errors and an advisory reported together", invalidAdvisory),
  entry("empty", "content", "no assignments, roles, repos or pipelines yet", empty),
  entry("orphans", "content", "a role and a pipeline nothing references", orphans),
  entry("two-assignments", "content", "two assignment cards in the stack", twoAssignments),
  entry("multi-repo", "content", "two repos, so rank and reorder are meaningful", multiRepo),
  entry("read-only-primary", "content", "the primary repo is registered read-only", readOnlyPrimary),
  entry("unknown-primary", "content", "the primary repo is not in repos.yaml", unknownPrimary),
  entry("no-credential", "content", "no registered repo names a credential", noCredential),
  entry("no-limits", "content", "every limit off", noLimits),
  entry("all-limits", "content", "every limit capped", allLimits),
  entry("no-signals", "content", "abort and escalate labels unset", noSignals),
  entry("draft-pending", "plan", "three unsaved changes waiting in the plan", draftPending),
  entry("draft-single", "plan", "one unsaved change, for the singular copy", draftSingle),
  entry("pipeline", "selection", "a pipeline selected", selectPipeline),
  entry("pipeline-missing", "selection", "the selected pipeline no longer exists", missingPipeline),
  entry("no-pipeline", "selection", "nothing selected", noPipeline),
]);

/**
 * Applies one fixture id or an ordered list of them. Unknown ids throw: a typo
 * must fail the suite, never render a quietly different state.
 */
export function applyFixture(ids, base) {
  const list = Array.isArray(ids) ? ids : [ids];
  return list.filter(Boolean).reduce((state, id) => {
    const fixture = FIXTURES[id];
    if (!fixture) {
      throw new Error(`unknown fixture: ${id}`);
    }
    return fixture.build(state);
  }, clone(base));
}

export const FIXTURE_IDS = Object.keys(FIXTURES);

/** A human label for a composed fixture, for the lab and the gallery. */
export function describeFixture(ids) {
  const list = (Array.isArray(ids) ? ids : [ids]).filter(Boolean);
  return list.length ? list.map((id) => `${id} (${FIXTURES[id].summary})`).join(" + ") : "none";
}
