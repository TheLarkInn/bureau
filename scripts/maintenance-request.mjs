import { requireValue } from "./maintenance-contract.mjs";
import { readStepRequest } from "./read-step-request.mjs";

// The reporter agent copies its step request to the helper's stdin, so every
// accepted shape is agent-authored bytes. Normalization only locates `inputs`;
// the helper still validates evidence and live forge state, and later
// deterministic steps re-observe everything.
export const EXPECTED_REQUEST = "request must be the v2 step request "
  + "{schema:\"v2\", inputs:{maintenance_evidence,...}} or the get_step_context result";

export const REQUIRED_INPUTS = Object.freeze({
  draft: ["maintenance_evidence"],
  clear: ["maintenance_evidence"],
  handoff: ["maintenance_evidence", "maintenance_draft", "maintenance_observed_at"],
});

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function kind(value) {
  if (value === null) return "null";
  return Array.isArray(value) ? "array" : typeof value;
}

function refuse(detail) {
  return new Error(`${EXPECTED_REQUEST}; ${detail}`);
}

function notJson(what, error) {
  return error instanceof SyntaxError ? refuse(`${what} is not JSON: ${error.message}`) : error;
}

// bureau-io.get_step_context answers with one MCP text item holding the v2 request.
function toolResultRequest(value) {
  const texts = value.content.filter((item) => item?.type === "text");
  if (value.isError === true || texts.length !== 1 || typeof texts[0].text !== "string") {
    throw refuse("a get_step_context result must be a successful result with exactly one text item");
  }
  try {
    return JSON.parse(texts[0].text);
  } catch (error) {
    throw notJson("the get_step_context text", error);
  }
}

function isToolResult(value) {
  return Array.isArray(value.content) && !("schema" in value) && !("inputs" in value);
}

function hasTopLevelInputs(value) {
  return Object.keys(value).some((key) => key.startsWith("maintenance_"));
}

function wrappedInputs(value) {
  if ("schema" in value && value.schema !== "v2") throw refuse("publisher requires a v2 request");
  if (!isObject(value.inputs)) throw refuse(`inputs must be an object, got ${kind(value.inputs)}`);
  if (hasTopLevelInputs(value)) throw refuse("maintenance_* keys appear both inside and outside inputs");
  return value.inputs;
}

// v2 request or `{inputs}` projection; otherwise the inputs object itself at top level.
function locateInputs(value, allowToolResult) {
  if (!isObject(value)) throw refuse(`got ${kind(value)}`);
  if (allowToolResult && isToolResult(value)) return locateInputs(toolResultRequest(value), false);
  if ("schema" in value || "inputs" in value) return wrappedInputs(value);
  return value;
}

function requiredInputs(mode) {
  requireValue(Object.hasOwn(REQUIRED_INPUTS, mode), "publisher operation must be draft, handoff, or clear");
  return REQUIRED_INPUTS[mode];
}

export function normalizeRequest(value, mode) {
  const required = requiredInputs(mode);
  const inputs = locateInputs(value, true);
  const missing = required.filter((key) => inputs[key] === undefined);
  if (missing.length) throw refuse(`missing inputs: ${missing.join(", ")}`);
  return { schema: "v2", inputs };
}

export async function readPublisherRequest(mode, read = readStepRequest) {
  requiredInputs(mode);
  let value;
  try {
    value = await read({ maximumBytes: 1024 * 1024 });
  } catch (error) {
    throw notJson("stdin", error);
  }
  return normalizeRequest(value, mode);
}
