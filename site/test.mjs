import { spawnSync } from "node:child_process";
import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { repositoryDirectory } from "./paths.mjs";

try {
  const files = (await readdir(join(repositoryDirectory, "scripts")))
    .filter((name) => /^(?:site.*|pages-.*)\.test\.mjs$/u.test(name)).sort()
    .map((name) => join(repositoryDirectory, "scripts", name));
  if (!files.length) throw new Error("No site tests found.");
  const result = spawnSync(process.execPath, ["--test", "--test-concurrency=1", "--test-reporter=tap", ...files], {
    cwd: repositoryDirectory, encoding: "utf8", timeout: 90_000, maxBuffer: 2 * 1024 * 1024,
  });
  process.stdout.write(result.stdout ?? "");
  process.stderr.write(result.stderr ?? "");
  if (result.error) throw result.error;
  if (result.status !== 0 || /^not ok /mu.test(result.stdout)
    || !/^ok 1 /mu.test(result.stdout) || /^# (?:skipped|todo) [1-9]/mu.test(result.stdout)) {
    throw new Error("Site tests failed, were skipped, or did not run.");
  }
} catch (error) {
  console.error(`Site tests incomplete: ${error.message}`);
  process.exitCode = 1;
}
