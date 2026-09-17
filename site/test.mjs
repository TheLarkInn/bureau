import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { runNodeTests } from "../scripts/run-node-tests.mjs";
import { repositoryDirectory } from "./paths.mjs";

try {
  const files = (await readdir(join(repositoryDirectory, "scripts")))
    .filter((name) => /^(?:site.*|pages-.*)\.test\.mjs$/u.test(name)).sort()
    .map((name) => join(repositoryDirectory, "scripts", name));
  if (!files.length) throw new Error("No site tests found.");
  runNodeTests(files, {
    label: "Site tests", cwd: repositoryDirectory, allowSkipped: false,
    timeout: 90_000, maxBuffer: 2 * 1024 * 1024,
  });
} catch (error) {
  console.error(`Site tests incomplete: ${error.message}`);
  process.exitCode = 1;
}
