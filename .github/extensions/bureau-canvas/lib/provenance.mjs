import { execFile } from "node:child_process";
import { promisify } from "node:util";

const exec = promisify(execFile);

async function git(dir, args) {
  const { stdout } = await exec("git", ["-C", dir, ...args], {
    windowsHide: true, timeout: 5000, maxBuffer: 1024 * 1024,
    env: { ...process.env, GIT_OPTIONAL_LOCKS: "0", GIT_TERMINAL_PROMPT: "0" },
  });
  return stdout;
}

export function changedPaths(porcelain) {
  const entries = porcelain.split("\0");
  const paths = [];
  for (let index = 0; index < entries.length; index += 1) {
    const entry = entries[index];
    if (!entry) continue;
    paths.push(entry.slice(3));
    if (/[RC]/u.test(entry.slice(0, 2))) index += 1;
  }
  return paths;
}

export async function authoringProvenance(dir, options = {}) {
  if (options.sample) return { state: "sample", commit: null, changes: null, message: "Bundled sample, not loaded authoring files." };
  const read = options.git ?? git;
  try {
    const [commit, status] = await Promise.all([
      read(dir, ["rev-parse", "--verify", "HEAD"]),
      read(dir, ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."]),
    ]);
    return { state: "known", commit: commit.trim(), changes: changedPaths(status), message: null };
  } catch (error) {
    return { state: "unavailable", commit: null, changes: null,
      message: `Authoring Git state unavailable (${error.code ?? error.message}); no clean-tree claim is made.` };
  }
}
