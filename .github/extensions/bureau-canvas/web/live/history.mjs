export async function readHistory(response) {
  if (!response.ok) {
    throw new Error(`Run history unavailable (HTTP ${response.status}); inspect the saved log and configured Bureau binary.`);
  }
  const payload = await response.json();
  if (!Array.isArray(payload?.events)) {
    throw new Error("Run history response has no event array; use a compatible Bureau binary.");
  }
  return payload;
}
