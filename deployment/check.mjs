import { lstat, realpath, statfs } from "node:fs/promises";
import { isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { requireValue } from "../scripts/maintenance-contract.mjs";
import { loadPolicy } from "../scripts/maintenance-policy.mjs";
import { admit } from "../scripts/maintenance-resources.mjs";
import { requireReadOnlyTools } from "../scripts/maintenance-tools.mjs";

export function durableFilesystem(type) {
  return [0xef53, 0x58465342, 0x9123683e].includes(type);
}

export async function deploymentCheck(home, root = process.cwd()) {
  requireValue(process.platform === "linux" && process.getuid() !== 0, "deployment requires a dedicated non-root Linux account");
  requireValue(typeof home === "string" && isAbsolute(home), "choose an explicit new absolute BUREAU_HOME");
  requireValue(await realpath(home) === resolve(home), "durable state root cannot be a symlink");
  const metadata = await lstat(home);
  requireValue(metadata.isDirectory() && (metadata.mode & 0o077) === 0, "state root must be a private directory");
  requireValue(durableFilesystem((await statfs(home)).type),
    "durable state requires a native persistent ext4, XFS or Btrfs filesystem; not tmpfs or a Windows mount");
  const settings = await lstat(resolve(home, "settings.yaml"));
  requireValue(settings.isFile() && !settings.isSymbolicLink() && (settings.mode & 0o077) === 0,
    "provision private non-symlink settings.yaml before starting");
  const policy = await loadPolicy(root);
  await requireReadOnlyTools(policy);
  await admit({ cwd: root, backingPaths: policy.backing_paths,
    extraPaths: [home, policy.cargo_target, policy.site_tools, policy.browser_path] });
  return { schema: "bureau-deployment-admission-v1", admitted: true,
    config_subdir: ".bureau/maintenance", state: home };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: { home: { type: "string" } } });
    console.log(JSON.stringify(await deploymentCheck(values.home)));
  } catch (error) {
    console.log(JSON.stringify({ schema: "bureau-deployment-admission-v1", admitted: false, message: error.message }));
    process.exitCode = 1;
  }
}
