import { execFileSync } from "node:child_process";

const sourceCommit = execFileSync("git", ["rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();

console.log(JSON.stringify({
  schema: "v2",
  outcome: "success",
  outputs: { source_commit: sourceCommit },
  artifacts: [],
  trust: "derived",
  message: `auditing source commit ${sourceCommit}`,
}));
