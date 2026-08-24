// The step log panel: what one step actually did, as blocks.
//
// The CLI's drawing characters are parsed away in ./transcript.js; this
// module renders the structure they encoded — a tool call is a block with a
// name, its argument and its result, not a line beginning with a glyph.

import React from "react";
import { blocks } from "./transcript.js";

const h = React.createElement;
const MINUTE = 60;

/**
 * `step` is the step name, `record` its overlay entry (state, outcome,
 * timings) and `text` its captured output up to the current position.
 */
export function StepLog({ step, kind, record, text, expectedAgent }) {
  if (!step) {
    return h("section", { className: "step-log step-log--idle" },
      h("p", { className: "step-log-empty" }, "Select a step to see what it did."));
  }
  return h(
    "section",
    { className: "step-log" },
    h(LogHead, { step, kind, record, expectedAgent }),
    h(LogBody, { text }),
  );
}

function LogHead({ step, kind, record, expectedAgent }) {
  const span = elapsed(record);
  return h(
    "header",
    { className: "step-log-head" },
    h("h3", { className: "step-log-title" }, step),
    kind ? h("span", { className: `kind-pill kind-pill--${kind}` }, kind) : null,
    record?.outcome ? h("span", { className: `outcome-pill outcome-pill--${record.outcome}` }, record.outcome) : null,
    record?.state === "running" ? h("span", { className: "outcome-pill outcome-pill--running" }, "running") : null,
    h(AgentIdentity, { record, expectedAgent }),
    span ? h("span", { className: "step-log-elapsed mono" }, span) : null,
  );
}

/**
 * The two agent identities a step can be read against, and what their
 * disagreement actually means.
 *
 * Both sides are projections of config, not observations of a spawn: the log
 * records what the role selected when the run started, and `expectedAgent`
 * comes from `bureau validate --json` reading the config as it stands now.
 * `crates/bureau/src/config/validate.rs` is what keeps the logged name equal to
 * the one the adapter passes to `--agent` — an `agent` that is neither a plugin
 * reference nor a `.md` path does not validate, and that is the only shape for
 * which the pure and the resolving forms differ.
 *
 * So the badge fires when the config has moved since the run, which is a real
 * and readable thing. It is not evidence that a spawn used the wrong agent, and
 * the copy may not say it is: "invoked X" claimed an observation nothing here
 * made, on a comparison that against an unchanged config cannot fail at all.
 */
function AgentIdentity({ record, expectedAgent }) {
  if (!record?.resolvedAgent) {
    return null;
  }
  const mismatch = Boolean(expectedAgent && record.resolvedAgent !== expectedAgent);
  const text = mismatch
    ? `this run used ${record.resolvedAgent}; the config now selects ${expectedAgent}`
    : `agent ${record.resolvedAgent}`;
  return h("span", {
    className: `step-agent${mismatch ? " step-agent--mismatch" : ""}`,
    "data-testid": "step-agent",
    "data-mismatch": String(mismatch),
  }, text);
}

function LogBody({ text }) {
  const parsed = blocks(text);
  if (parsed.length === 0) {
    return h("p", { className: "step-log-empty" }, "No output captured for this step yet.");
  }
  return h("div", { className: "step-log-body" }, parsed.map((block, index) =>
    h(Block, { key: `${block.kind}-${index}`, block })));
}

function Block({ block }) {
  const renderer = BLOCKS[block.kind] ?? Note;
  return h(renderer, { block });
}

/** One tool call: what ran, what it was given, what came back. */
function Tool({ block }) {
  return h(
    "article",
    { className: "log-tool" },
    h("p", { className: "log-tool-name mono" }, block.title),
    block.detail.length ? h("p", { className: "log-tool-arg mono" }, block.detail.join("")) : null,
    block.result ? h("p", { className: "log-tool-result mono" }, block.result) : null,
  );
}

/** A deterministic step's contract: outcome, message, outputs, artifacts. */
function Result({ block }) {
  const outputs = Object.entries(block.outputs ?? {});
  return h(
    "article",
    { className: "log-result" },
    h(
      "p",
      { className: "log-result-head" },
      h("span", { className: `outcome-pill outcome-pill--${block.outcome ?? "no-work"}` }, block.outcome ?? "no outcome"),
      block.message ? h("span", { className: "log-result-message" }, block.message) : null,
    ),
    outputs.length ? h("dl", { className: "log-outputs mono" }, outputs.flatMap(([name, value]) => [
      h("dt", { key: `name-${name}` }, name),
      h("dd", { key: `value-${name}` }, String(value)),
    ])) : null,
    block.artifacts?.length ? h("ul", { className: "log-artifacts" }, block.artifacts.map((artifact) =>
      h("li", { key: artifact.path ?? artifact.name, className: "chip", title: artifact.path ?? "" }, artifact.name))) : null,
  );
}

function Warning({ block }) {
  return h("p", { className: "log-warning" }, block.text);
}

/** Raw process output — a stack trace, usually. Never reflowed. */
function Raw({ block }) {
  return h("pre", { className: "log-raw mono" }, block.text);
}

function Note({ block }) {
  return h("p", { className: "log-note" }, block.text);
}

const BLOCKS = { tool: Tool, result: Result, warning: Warning, output: Raw, note: Note };

function elapsed(record) {
  if (typeof record?.startedAt !== "number" || typeof record?.finishedAt !== "number") {
    return null;
  }
  const seconds = (record.finishedAt - record.startedAt) / 1000;
  if (seconds < MINUTE) {
    return `${seconds.toFixed(1)}s`;
  }
  return `${Math.floor(seconds / MINUTE)}m ${Math.round(seconds % MINUTE)}s`;
}

/**
 * The step the log follows: the one clicked, else whatever the run is inside
 * now, else the last one it finished.
 */
export function focusStep(selected, overlay) {
  return selected ?? overlay?.current ?? overlay?.transitions?.at(-1)?.to ?? null;
}
