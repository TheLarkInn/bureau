import { spawnSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";

const output = "target/bureau-review/detector.json";
await mkdir("target/bureau-review", { recursive: true });
const run = spawnSync(
  process.execPath,
  [
    ".github/skills/impeccable/scripts/detect.mjs",
    "--json",
    ".github/extensions/bureau-canvas",
  ],
  { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
);
await writeFile(output, run.stdout ?? "", "utf8");
const accepted = run.status === 0 || run.status === 2;
console.log(JSON.stringify({
  schema: "v2",
  outcome: accepted ? "success" : "failure",
  outputs: { detector_exit: run.status },
  artifacts: [{ name: "detector.json", path: output }],
  trust: "derived",
  message: accepted ? "design detector completed" : (run.stderr || run.error?.message || "design detector failed"),
}));
