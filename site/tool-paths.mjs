import { readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { isAbsolute, join, relative, resolve } from "node:path";
import { repositoryDirectory } from "./paths.mjs";

export const packageDirectories = {
  browser: ".github/extensions/bureau-canvas/e2e/playwright",
  audit: "site",
};

export function toolPaths(external = process.env.BUREAU_SITE_TOOLS) {
  if (external !== undefined && (!external || !isAbsolute(external))) {
    throw new Error("BUREAU_SITE_TOOLS must be an absolute prepared tools directory.");
  }
  const root = external === undefined ? repositoryDirectory : realpathSync(external);
  if (external !== undefined) {
    for (const directory of Object.values(packageDirectories)) {
      for (const name of ["package.json", "package-lock.json"]) {
        const file = join(directory, name);
        if (!readFileSync(join(root, file)).equals(readFileSync(join(repositoryDirectory, file)))) {
          throw new Error(`Prepared site tools do not match the protected checkout: ${file}`);
        }
      }
    }
  }
  return {
    root,
    browser: join(root, packageDirectories.browser, "package.json"),
    audit: join(root, packageDirectories.audit, "package.json"),
  };
}

export function confineTool(directory, file) {
  const path = relative(resolve(directory), resolve(file));
  if (path.startsWith("..") || isAbsolute(path)) throw new Error("Site tools resolved outside their prepared package directory.");
  return file;
}

export function resolveTool(kind, name) {
  const paths = toolPaths();
  const require = createRequire(paths[kind]);
  const file = realpathSync(require.resolve(name));
  return confineTool(realpathSync(join(paths.root, packageDirectories[kind], "node_modules")), file);
}

export function loadTool(kind, name) {
  return createRequire(import.meta.url)(resolveTool(kind, name));
}

export function requireToolVersion(kind, name) {
  const lock = JSON.parse(readFileSync(join(repositoryDirectory, packageDirectories[kind], "package-lock.json"), "utf8"));
  const expected = lock.packages[`node_modules/${name}`].version;
  if (loadTool(kind, `${name}/package.json`).version !== expected) {
    throw new Error(`Installed ${name} does not match the protected lockfile.`);
  }
}
