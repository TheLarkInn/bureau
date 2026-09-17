import assert from "node:assert/strict";
import { copyFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import { repositoryDirectory, siteDirectory } from "../site/paths.mjs";
import { confineTool, packageDirectories, toolPaths } from "../site/tool-paths.mjs";

async function prepared(t) {
  const cache = join(siteDirectory, ".cache");
  await mkdir(cache, { recursive: true });
  const root = await mkdtemp(join(cache, "tools-test-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const directory of Object.values(packageDirectories)) {
    await mkdir(join(root, directory), { recursive: true });
    for (const name of ["package.json", "package-lock.json"]) {
      await copyFile(join(repositoryDirectory, directory, name), join(root, directory, name));
    }
  }
  return root;
}

test("prepared tool roots match both existing package manifests and lockfiles exactly", async (t) => {
  const root = await prepared(t);
  const paths = toolPaths(root);
  assert.equal(paths.root, root);
  assert.equal(paths.audit, join(root, "site", "package.json"));
  assert.equal(paths.browser, join(root, packageDirectories.browser, "package.json"));
});

test("external tools never fall back to mutable worktree dependencies", async (t) => {
  const root = await prepared(t);
  await writeFile(join(root, "site", "package-lock.json"), "{}");
  assert.throws(() => toolPaths(root), /do not match the protected checkout/u);
  for (const path of ["", "relative-tools", join(root, "missing")]) assert.throws(() => toolPaths(path));
});

test("resolved tool modules must remain inside the selected package's node_modules", () => {
  const root = resolve("site", ".cache", "prepared", "node_modules");
  assert.equal(confineTool(root, join(root, "axe-core", "axe.js")), join(root, "axe-core", "axe.js"));
  assert.throws(() => confineTool(root, resolve(root, "..", "elsewhere", "axe.js")), /outside/u);
});
