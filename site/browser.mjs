import { spawnSync } from "node:child_process";
import { browserCli, playwrightConfig } from "./browser-tools.mjs";
import { repositoryDirectory } from "./paths.mjs";

try {
  const result = spawnSync(process.execPath, [browserCli(), "test", "--config", playwrightConfig], {
    cwd: repositoryDirectory, stdio: "inherit", timeout: 180_000,
    env: { ...process.env, FORCE_COLOR: "0" },
  });
  if (result.error) throw result.error;
  if (result.signal) throw new Error(`Browser checks stopped with ${result.signal}.`);
  process.exitCode = result.status ?? 2;
} catch (error) {
  console.error(`Site browser checks incomplete: ${error.message}`);
  process.exitCode = 2;
}
