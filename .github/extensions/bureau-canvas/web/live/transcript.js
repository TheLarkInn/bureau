// Turns a step's captured output into structured blocks the panel can render.
//
// Agent steps stream a Copilot CLI transcript: a marker line per tool call,
// indented argument lines, and an indented result line. Deterministic steps
// emit one line of the v2 step-result contract. Failures emit whatever the
// process wrote, usually a stack trace.
//
// The glyphs are the CLI's drawing characters, not content, so they are
// consumed here and never rendered — the panel draws real blocks instead.
// Pure and DOM-free, so test/transcript.test.mjs covers it headlessly.

const TOOL = /^\s*(?:[●•]|\uFFFD+)\s+(.*)$/u;
const SEARCH = /^\/\s+(.+)$/u;
const DETAIL = /^\s*\u2502\s?(.*)$/u;
const RESULT = /^\s*\u2514\s?(.*)$/u;
const WARNING = /^Warning:\s*(.*)$/u;
const CONTRACT = /^\s*\{.*"schema"\s*:\s*"v2".*\}\s*$/u;

/**
 * Every `output` event for one step, in log order, concatenated. `until`
 * bounds it to a replay position; live passes `Infinity`.
 */
export function stepOutput(events, step, until = Infinity) {
  return (events ?? [])
    .filter((event) => event.kind === "output" && event.data?.step === step)
    .filter((event) => (event.at_ms ?? 0) <= until)
    .map((event) => event.data?.data ?? "")
    .join("");
}

/** Run-level output, which carries the engine's own notices. */
export function runOutput(events, until = Infinity) {
  return (events ?? [])
    .filter((event) => event.kind === "output" && event.data?.stream === "run")
    .filter((event) => (event.at_ms ?? 0) <= until)
    .map((event) => event.data?.data ?? "")
    .join("");
}

/** Structured blocks for one step's output, in order. */
export function parseTranscript(text) {
  const blocks = [];
  for (const line of String(text ?? "").split("\n")) {
    consume(blocks, line);
  }
  return blocks;
}

function consume(blocks, line) {
  const contract = asContract(line);
  if (contract) {
    blocks.push(contract);
    return;
  }
  if (appendToTool(blocks, line)) {
    return;
  }
  for (const part of splitEmbedded(line)) {
    blocks.push(...opening(part));
  }
}

/**
 * The capture can land a process warning and the start of a tool call on one
 * line. Split them, or the tool call is swallowed and its result line has
 * nothing to attach to.
 */
function splitEmbedded(line) {
  if (TOOL.test(line)) {
    return [line];
  }
  const at = line.search(/[●•]\s/u);
  return at > 0 ? [line.slice(0, at).trimEnd(), line.slice(at)] : [line];
}

/** Detail and result lines belong to the tool call above them. */
function appendToTool(blocks, line) {
  const open = blocks.at(-1);
  if (open?.kind !== "tool") {
    return false;
  }
  const detail = DETAIL.exec(line);
  if (detail) {
    open.detail.push(detail[1]);
    return true;
  }
  const result = RESULT.exec(line);
  if (result) {
    open.result = open.result ? `${open.result}\n${result[1]}` : result[1];
    return true;
  }
  return false;
}

function opening(line) {
  const tool = TOOL.exec(line) ?? SEARCH.exec(line);
  if (tool) {
    return [{ kind: "tool", title: tool[1].trim(), detail: [], result: "" }];
  }
  const warning = WARNING.exec(line);
  if (warning) {
    return [{ kind: "warning", text: warning[1] }];
  }
  return [note(line)];
}

/**
 * Prose from the agent. A blank line is kept as an empty note so `blocks()`
 * can tell a wrapped paragraph from two separate ones.
 */
function note(line) {
  return { kind: "note", text: line.trim() };
}

function asContract(line) {
  if (!CONTRACT.test(line)) {
    return null;
  }
  try {
    const value = JSON.parse(line);
    return {
      kind: "result",
      outcome: value.outcome ?? null,
      message: value.message ?? "",
      outputs: value.outputs ?? {},
      artifacts: value.artifacts ?? [],
      trust: value.trust ?? null,
    };
  } catch {
    return null;
  }
}

function isEmpty(block) {
  return block.kind === "note" && block.text === "";
}

/**
 * Presentation blocks: wrapped prose merged into paragraphs, blank lines
 * dropped once they have done their job of separating them.
 *
 * Output with no tool call and no contract line is not a transcript at all —
 * it is a deterministic step's raw stream, usually a stack trace — so it is
 * kept verbatim for a preformatted block instead of being reflowed as prose.
 */
export function blocks(text) {
  const parsed = parseTranscript(text);
  if (!parsed.some((block) => block.kind === "tool" || block.kind === "result")) {
    const raw = String(text ?? "").replace(/\s+$/u, "");
    return raw ? [{ kind: "output", text: raw }] : [];
  }
  const merged = [];
  for (const block of parsed) {
    mergeInto(merged, block);
  }
  return merged.map(({ closed, ...block }) => block);
}

function mergeInto(merged, block) {
  const previous = merged.at(-1);
  if (isEmpty(block)) {
    if (previous?.kind === "note") {
      previous.closed = true;
    }
    return;
  }
  if (block.kind === "note" && previous?.kind === "note" && !previous.closed) {
    previous.text = `${previous.text} ${block.text}`;
    return;
  }
  merged.push(block);
}
