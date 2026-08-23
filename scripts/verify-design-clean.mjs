import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";

const request = JSON.parse(await readFile(0, "utf8"));
const sourceCommit = request.inputs?.source_commit;
const options = { encoding: "utf8" };
const current = execFileSync("git", ["rev-parse", "HEAD"], options).trim();
const status = execFileSync("git", ["status", "--porcelain"], options).trim();

if (current !== sourceCommit || status) {
  console.error("design review changed the audited worktree");
  process.exitCode = 1;
}
