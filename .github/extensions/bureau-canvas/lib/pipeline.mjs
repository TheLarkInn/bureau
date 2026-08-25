// The pipeline editor's save path and the layout sidecar.
//
// `savePipeline` is the round-trip guarantee: render the edited view, write
// the file, run `bureau validate --json`, and put the original bytes back if
// the findings touch the edited pipeline. The editor can hint all it wants;
// the file on disk after a save either validates or was never changed.
//
// `layout.json` is a sidecar, not config: node positions keyed by pipeline
// and step, loaded and merged into each pipeline view, and rewritten on the
// same save so the graph a user arranged survives a reload.

import { readFile, rm, writeFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";

import { parse, render } from "./codec.mjs";
import { serializable } from "./edit.mjs";
import { findings } from "./findings.mjs";
import { pathFor } from "./files.mjs";

const LAYOUT_FILE = "layout.json";
const saves = new Map();

/** Fields a decision step's view must express as an `on` map at write time. */
const DECISION_OUTCOMES = ["success", "failure", "blocked", "no-work"];

export function savePipeline(input, deps = {}) {
  // Every pipeline in a config shares layout.json, so the transaction lock is
  // config-wide: two different pipelines must not read and rewrite that sidecar
  // from the same old snapshot.
  const key = resolve(input.dir);
  const previous = saves.get(key) ?? Promise.resolve();
  const pending = previous.catch(() => {}).then(() => savePipelineUnlocked(input, deps));
  saves.set(key, pending);
  return pending.finally(() => {
    if (saves.get(key) === pending) {
      saves.delete(key);
    }
  });
}

async function savePipelineUnlocked(input, deps) {
  const dir = resolve(input.dir);
  const name = input.pipeline;
  if (input.view?.name && input.view.name !== name) {
    throw new Error(`view name \`${input.view.name}\` does not match pipeline \`${name}\``);
  }
  const view = normalizedView(input.view);
  const path = pathFor(dir, "pipeline", name);
  const original = await readText(deps, path);
  if (original == null) {
    throw new Error(`pipeline \`${name}\` has no file at ${path}`);
  }
  const text = renderView(view, original, path, dir);
  const validation = await roundTrip(dir, path, original, text, deps);
  if (validation.findings.length > 0) {
    return { ok: false, saved: false, findings: validation.findings, path };
  }
  await commitLayout(dir, name, input.layout ?? null, path, original, deps);
  return { ok: true, saved: true, findings: [], path };
}

async function commitLayout(dir, name, positions, pipelinePath, original, deps) {
  if (positions == null) {
    return;
  }
  const path = layoutPath(dir);
  const previous = await readText(deps, path);
  try {
    await writeLayout(dir, name, positions, deps);
  } catch (error) {
    await writeText(deps, pipelinePath, original);
    await restoreLayout(deps, path, previous);
    throw error;
  }
}

async function restoreLayout(deps, path, previous) {
  if (previous != null) {
    await writeText(deps, path, previous);
    return;
  }
  const remove = deps.removeText ?? ((candidate) => rm(candidate, { force: true }));
  await remove(path);
}

/**
 * The editor's view keeps decision branches in `fields.on` (edit.mjs) while
 * the codec builds a decision's `on` from its control edges. Translate
 * between the two before rendering so either edit style round-trips.
 */
function normalizedView(view) {
  // The codec's render only diffs views it recognizes as pipelines; the
  // projection the editor works on omits the marker, so put it back.
  const next = { kind: "pipeline", ...serializable(structuredClone(view)) };
  next.edges = next.edges.flatMap((edge) => (decisionFieldEdge(next, edge) ? [] : [edge]));
  for (const step of next.steps) {
    if (step.kind === "decision") {
      next.edges.push(...decisionEdges(step));
    }
  }
  return next;
}

function decisionFieldEdge(view, edge) {
  if (edge.relation !== "control") {
    return false;
  }
  const source = view.steps.find((step) => step.name === edge.source);
  return source?.kind === "decision";
}

function decisionEdges(step) {
  const on = step.fields?.on ?? {};
  return DECISION_OUTCOMES.filter((outcome) => presentTarget(on[outcome])).map((outcome) => ({
    id: `control:${step.name}:${outcome}->${resolveTerminal(on[outcome])}`,
    source: step.name,
    target: resolveTerminal(on[outcome]),
    relation: "control",
    outcome,
  }));
}

function resolveTerminal(target) {
  return ["done", "abort", "escalate"].includes(target) ? `terminal:${target}` : target;
}

function presentTarget(target) {
  return typeof target === "string" && target.length > 0;
}

function renderView(view, original, path, dir) {
  const parsed = parse(original, { path: relative(resolve(dir), path).replaceAll("\\", "/") });
  if (parsed.view.kind !== "pipeline") {
    throw new Error(`${path} is not a pipeline file`);
  }
  return render(view, parsed.doc, parsed.style);
}

/**
 * Write, validate, and keep or revert. A finding belongs to this save when
 * it names the edited pipeline (or a step in it); unrelated findings from
 * other files do not block the write.
 */
async function roundTrip(dir, path, original, text, deps) {
  await writeText(deps, path, text);
  const result = await validatedResult(dir, path, original, deps);
  const relevant = (result.findings ?? []).filter((finding) => touchesPipeline(finding, path, dir));
  if (relevant.length > 0) {
    await writeText(deps, path, original);
  }
  return { findings: relevant };
}

async function validatedResult(dir, path, original, deps) {
  try {
    const result = await validator(deps)(dir);
    if (result.state !== "validated") {
      throw new Error(result.message ?? "pipeline validation did not complete");
    }
    return result;
  } catch (error) {
    await writeText(deps, path, original);
    throw error;
  }
}

function touchesPipeline(finding, path, dir) {
  const target = finding.target ?? {};
  if (target.kind === "step" || target.kind === "pipeline") {
    return target.pipeline === pipelineNameFromPath(path);
  }
  const findingPath = String(finding.path ?? target.path ?? "").replaceAll("\\", "/");
  return sameFile(findingPath, path, dir);
}

function pipelineNameFromPath(path) {
  return path.replaceAll("\\", "/").split("/").at(-1).replace(/\.(ya?ml)$/u, "");
}

function sameFile(findingPath, path, dir) {
  if (findingPath === "") {
    return false;
  }
  const absolute = resolve(dir, findingPath).replaceAll("\\", "/");
  return absolute === resolve(path).replaceAll("\\", "/");
}

function validator(deps) {
  return deps.validate ?? ((dir) => findings(dir, deps.findingsOptions ?? {}));
}

async function readText(deps, path) {
  const read = deps.readText ?? ((candidate) => readFile(candidate, "utf8"));
  return read(path, "utf8").catch(() => null);
}

async function writeText(deps, path, text) {
  const write = deps.writeText ?? ((candidate, contents) => writeFile(candidate, contents, "utf8"));
  await write(path, text, "utf8");
}

/** The sidecar's layout for one pipeline, or `{}` when none is saved. */
export async function readLayout(dir, deps = {}) {
  const text = await readText(deps ?? {}, layoutPath(dir));
  if (text == null) {
    return {};
  }
  try {
    const parsed = JSON.parse(text);
    return parsed?.pipelines ?? {};
  } catch {
    return {};
  }
}

function layoutPath(dir) {
  return join(resolve(dir), LAYOUT_FILE);
}

/**
 * Merges one pipeline's positions into the sidecar and rewrites it. `null`
 * leaves existing content alone; steps absent from the map keep nothing, so
 * a deleted step's position goes away with the step.
 */
export async function writeLayout(dir, pipeline, positions, deps = {}) {
  if (positions == null) {
    return;
  }
  const existing = await readLayout(dir, deps);
  const steps = Object.fromEntries(
    Object.entries(positions).filter(([, position]) => validPosition(position)),
  );
  const next = { pipelines: { ...existing, [pipeline]: { steps } } };
  const write = deps.writeText ?? ((candidate, contents) => writeFile(candidate, contents, "utf8"));
  await write(layoutPath(dir), `${JSON.stringify(next, null, 2)}\n`, "utf8");
}

function validPosition(position) {
  return Number.isFinite(position?.x) && Number.isFinite(position?.y);
}

/**
 * Positions for a pipeline view: saved coordinates where they exist, so
 * `layout.js` only runs for steps the user has never placed.
 */
export function arrangementFor(layouts, pipeline) {
  const steps = layouts?.[pipeline]?.steps ?? {};
  return Object.fromEntries(Object.entries(steps).filter(([, position]) => validPosition(position)));
}
