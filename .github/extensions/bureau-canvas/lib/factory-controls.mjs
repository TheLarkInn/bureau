import { runBureau } from "./runs.mjs";
import { parseFactoryControls } from "../web/live/factory-controls.mjs";

export async function readFactoryControls(runId, runsDir, options = {}) {
  const run = await runBureau(["show", runId, "--json", "--runs", runsDir], options);
  if (!run) throw new Error("Bureau binary unavailable; local factory resume cannot be verified.");
  if (run.code !== 0) {
    throw new Error(`Bureau could not verify local factory resume: ${run.stderr || run.stdout || `exit ${run.code}`}`);
  }
  let projection;
  try {
    projection = JSON.parse(run.stdout);
  } catch {
    throw new Error("Bureau returned invalid state JSON; local factory resume cannot be verified.");
  }
  const payload = { run_id: projection?.state?.run_id, local_factory_resume: projection?.local_factory_resume };
  parseFactoryControls(payload, runId);
  return payload;
}
