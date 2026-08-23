// One build of the concurrent-group payload, shared by the regenerator and the
// test that pins it — so "what the host would serve" is computed one way and
// the check cannot quietly diverge from the thing it checks.

import { readFile } from "node:fs/promises";

/**
 * The fields that belong to whichever host is running rather than to the
 * fixture. The fixture takes them from the payload it projects over, so the
 * committed module holds none of them and renders the same on every machine.
 */
export const PROJECTED_FIELDS = ["canvasId", "instanceId", "repoRoot", "dir"];

export const MODULE_URL = new URL("../../web/statelab/concurrent-state.mjs", import.meta.url);
const PAYLOAD_URL = new URL("../fixtures/concurrent-payload.json", import.meta.url);

/** The `/state` the host builds from the committed validate payload. */
export async function buildConcurrentState() {
  process.env.BUREAU_CANVAS_TEST = "1";
  const canvas = await import("../../extension.mjs");
  const payload = JSON.parse(await readFile(PAYLOAD_URL, "utf8"));
  const instanceId = "concurrent-state-build";
  const opened = await canvas.openBureauCanvas({ instanceId, input: { pipeline: "review-queue-pipeline" } }, { payload });
  try {
    const served = await fetch(new URL("/state", opened.url)).then((response) => response.json());
    return Object.fromEntries(Object.entries(served).filter(([key]) => !PROJECTED_FIELDS.includes(key)));
  } finally {
    await canvas.closeBureauCanvas({ instanceId });
  }
}
