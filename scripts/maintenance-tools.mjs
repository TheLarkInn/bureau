import { lstat, readlink, realpath, symlink, unlink } from "node:fs/promises";
import { join, resolve } from "node:path";

import { requireValue } from "./maintenance-contract.mjs";
import { readBoundedFile } from "./maintenance-files.mjs";
import { filesystemSnapshot } from "./maintenance-mount.mjs";

export { readOnlyMount } from "./maintenance-mount.mjs";

export const TOOL_PACKAGES = ["site", ".github/extensions/bureau-canvas/e2e/playwright"];

export async function requireReadOnlyTools(policy) {
  for (const path of [policy.site_tools, policy.browser_path]) {
    const canonical = await realpath(path);
    requireValue(canonical === path && (await filesystemSnapshot(canonical)).readOnly,
      `prepared tools must be on an explicitly read-only mount: ${path}`);
  }
}

export async function linkPreparedTools(root, policy) {
  await requireReadOnlyTools(policy);
  const owned = [];
  try {
    for (const packagePath of TOOL_PACKAGES) {
      for (const name of ["package.json", "package-lock.json"]) {
        const expected = await readBoundedFile(join(root, packagePath, name), 1024 * 1024);
        const observed = await readBoundedFile(join(policy.site_tools, packagePath, name), 1024 * 1024);
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
