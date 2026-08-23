import { execFileSync } from "node:child_process";

import { readStepRequest } from "./read-step-request.mjs";

const request = await readStepRequest();
const sourceCommit = request.inputs?.source_commit;
const options = { encoding: "utf8" };
const current = execFileSync("git", ["rev-parse", "HEAD"], options).trim();
const status = execFileSync("git", ["status", "--porcelain"], options).trim();

if (current !== sourceCommit || status) {
  console.error("design review changed the audited worktree");
  process.exitCode = 1;
}
