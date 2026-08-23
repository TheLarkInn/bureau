import { spawnSync } from "node:child_process";
import { access, mkdir, writeFile } from "node:fs/promises";

const root = ".github/extensions/bureau-canvas/e2e/playwright";
const logPath = "target/bureau-review/playwright.log";
await mkdir("target/bureau-review", { recursive: true });
const options = { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 };
const install = spawnSync("npm", ["ci", "--offline", "--prefix", root], options);
const run = install.status === 0
  ? spawnSync("npm", ["--prefix", root, "run", "capture-review"], options)
  : null;
const log = [install.stdout, install.stderr, run?.stdout, run?.stderr]
  .filter(Boolean)
  .join("\n");
await writeFile(logPath, log, "utf8");
const artifacts = [{ name: "playwright.log", path: logPath }];
for (const name of ["desktop.png", "mobile.png"]) {
  const path = `.impeccable/review/${name}`;
  try {
    await access(path);
    artifacts.push({ name, path });
  } catch {
    // Missing captures are reported by the critique step from the artifact set.
  }
}
const status = typeof run?.status === "number" ? run.status : 1;
console.log(JSON.stringify({
  schema: "v2",
  outcome: status === 0 ? "success" : "failure",
  outputs: { playwright_exit: status },
  artifacts,
  trust: "derived",
  message: status === 0 ? "Playwright captures completed" : "Playwright capture or test failed",
}));
