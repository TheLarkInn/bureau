import { lstat, readFile, readlink, realpath, symlink, unlink } from "node:fs/promises";
import { join, resolve } from "node:path";

import { requireValue } from "./maintenance-contract.mjs";

export const TOOL_PACKAGES = ["site", ".github/extensions/bureau-canvas/e2e/playwright"];

function unescapeMount(value) {
  return value.replace(/\\([0-7]{3})/gu, (_, octal) => String.fromCharCode(Number.parseInt(octal, 8)));
}

export function readOnlyMount(mountinfo, path) {
  const mounts = mountinfo.trim().split("\n").map((line) => {
    const fields = line.split(" ");
    return { path: unescapeMount(fields[4] ?? ""), options: (fields[5] ?? "").split(",") };
  }).filter((mount) => path === mount.path || path.startsWith(mount.path === "/" ? "/" : `${mount.path}/`));
  mounts.sort((left, right) => right.path.length - left.path.length);
  return mounts.length > 0 && mounts[0].options.includes("ro");
}

export async function requireReadOnlyTools(policy) {
  const mounts = await readFile("/proc/self/mountinfo", "utf8");
  for (const path of [policy.site_tools, policy.browser_path]) {
    const canonical = await realpath(path);
    requireValue(canonical === path && readOnlyMount(mounts, canonical),
      `prepared tools must be on an explicitly read-only mount: ${path}`);
  }
}

export async function linkPreparedTools(root, policy) {
  await requireReadOnlyTools(policy);
  const owned = [];
  try {
    for (const packagePath of TOOL_PACKAGES) {
      for (const name of ["package.json", "package-lock.json"]) {
        const expected = await readFile(join(root, packagePath, name));
        const observed = await readFile(join(policy.site_tools, packagePath, name));
        requireValue(expected.equals(observed), `prepared ${packagePath}/${name} differs from the checkout`);
      }
      const target = await realpath(join(policy.site_tools, packagePath, "node_modules"));
      requireValue(target.startsWith(`${policy.site_tools}/`), "prepared dependencies escaped the tool root");
      const path = resolve(root, packagePath, "node_modules");
      let metadata;
      try {
        metadata = await lstat(path);
      } catch (error) {
        if (error.code !== "ENOENT") throw error;
      }
      if (metadata) {
        requireValue(metadata.isSymbolicLink() && await readlink(path) === target,
          "existing worktree dependencies are not the prepared read-only tools");
      } else {
        await symlink(target, path, "dir");
        owned.push({ path, target });
      }
    }
  } catch (error) {
    await unlinkPreparedTools(owned);
    throw error;
  }
  return owned;
}

export async function unlinkPreparedTools(owned) {
  for (const { path, target } of owned) {
    requireValue((await lstat(path)).isSymbolicLink() && await readlink(path) === target,
      "owned dependency link changed; preserve it for inspection");
    await unlink(path);
  }
}
