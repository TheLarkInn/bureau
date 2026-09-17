import { inspectEvents } from "../run-evidence.mjs";

export async function readHistory(response) {
  if (!response.ok) {
  const detail = await response.text();
  let message;
  try { message = JSON.parse(detail)?.error; } catch { /* Non-JSON HTTP failure. */ }
  throw new Error(message || `Run history unavailable (HTTP ${response.status}); inspect the saved log and configured Bureau binary.`);
  }
  const payload = await response.json();
  if (!Array.isArray(payload?.events)) {
    throw new Error("Run history response has no event array; use a compatible Bureau binary.");
  }
  const problem = payload.error ?? inspectEvents(payload.events);
  if (problem) throw new Error(problem);
  return payload;
}
