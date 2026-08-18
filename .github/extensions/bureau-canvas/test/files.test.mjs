import assert from "node:assert/strict";
import { mkdtemp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { createFile, deleteFile, kinds, listNames, pathFor, renameFile } from "../lib/files.mjs";

const ROLE = "name: implementer\nagent: /bureau:implementer\nadapter: copilot\npermissions:\n- repo:read\nmin_trust: maintainer\n";

async function withConfig(fn) {
  const root = await mkdtemp(join(tmpdir(), "bureau-files-test-"));
  try {
    await mkdir(join(root, "roles"), { recursive: true });
    await fn(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("resolves each kind to the path the loader scans", async () => {
  await withConfig(async (root) => {
    assert.deepEqual(
      {
        role: pathFor(root, "role", "implementer").endsWith(join("roles", "implementer.yaml")),
        assignment: pathFor(root, "assignment", "a").endsWith(join("assignments", "a.yaml")),
        pipeline: pathFor(root, "pipeline", "p").endsWith(join("pipelines", "p.yaml")),
        // repos.yaml is one shared file, not a directory of entities.
        repo: pathFor(root, "repo").endsWith("repos.yaml"),
        kinds: kinds().sort(),
      },
      {
        role: true,
        assignment: true,
        pipeline: true,
        repo: true,
        kinds: ["assignment", "pipeline", "repo", "role"],
      },
    );
  });
});

test("refuses names that would escape the config directory", async () => {
  await withConfig(async (root) => {
    const refused = ["../escape", "nested/name", "nested\\name", "..", "", "   "].map((name) => {
      try {
        pathFor(root, "role", name);
        return null;
      } catch (error) {
        return error.message.includes(name.trim()) || error.message.includes("non-empty");
      }
    });

    assert.deepEqual(refused, [true, true, true, true, true, true]);
  });
});

test("creates, lists and deletes without recursing or reading non-YAML", async () => {
  await withConfig(async (root) => {
    await createFile(root, "role", "implementer", ROLE);
    await writeFile(join(root, "roles", "notes.txt"), "ignored");
    await mkdir(join(root, "roles", "nested"), { recursive: true });
    await writeFile(join(root, "roles", "nested", "deep.yaml"), ROLE);

    const listed = await listNames(root, "role");
    await deleteFile(root, "role", "implementer");
    const after = await listNames(root, "role");

    assert.deepEqual({ listed, after }, { listed: ["implementer"], after: [] });
  });
});

test("refuses to create over an existing file", async () => {
  await withConfig(async (root) => {
    await createFile(root, "role", "implementer", ROLE);
    await assert.rejects(() => createFile(root, "role", "implementer", ROLE), /already exists/u);
  });
});

test("renames only when the declared name moves with the file", async () => {
  await withConfig(async (root) => {
    await createFile(root, "role", "implementer", ROLE);

    // The stem and the `name` field are coupled; moving one alone is refused.
    await assert.rejects(() => renameFile(root, "role", "implementer", "builder", ROLE), /declared name updated/u);

    const renamed = ROLE.replace("name: implementer", "name: builder");
    await renameFile(root, "role", "implementer", "builder", renamed);
    await writeFile(join(root, "roles", "builder.yaml"), renamed);

    assert.deepEqual(await listNames(root, "role"), ["builder"]);
  });
});

test("refuses a rename that would overwrite another entity", async () => {
  await withConfig(async (root) => {
    await createFile(root, "role", "implementer", ROLE);
    await createFile(root, "role", "reviewer", ROLE.replace("name: implementer", "name: reviewer"));

    await assert.rejects(
      () => renameFile(root, "role", "implementer", "reviewer", ROLE.replace("name: implementer", "name: reviewer")),
      /already exists/u,
    );
    assert.deepEqual(await listNames(root, "role"), ["implementer", "reviewer"]);
  });
});

test("works against any directory, independent of the process cwd", async () => {
  await withConfig(async (root) => {
    const before = process.cwd();
    process.chdir(tmpdir());
    try {
      await createFile(root, "pipeline", "p", "name: p\nsteps: []\n");
      const text = await readFile(join(root, "pipelines", "p.yaml"), "utf8");
      const entries = await readdir(join(root, "pipelines"));
      assert.deepEqual({ text, entries }, { text: "name: p\nsteps: []\n", entries: ["p.yaml"] });
    } finally {
      process.chdir(before);
    }
  });
});
