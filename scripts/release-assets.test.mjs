import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { assetNames, publish, readAssets } from "./release-assets.mjs";

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), "bureau-release-assets-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  for (const name of assetNames("0.1.0").filter((name) => name.endsWith(".tar.gz"))) {
    const bytes = Buffer.from(name);
    const hash = createHash("sha256").update(bytes).digest("hex");
    await writeFile(join(directory, name), bytes);
    await writeFile(join(directory, `${name}.sha256`), `${hash}  ${name}\n`);
  }
  return directory;
}

function api({ draft = true, sha = "tested", existing = [], failure = false } = {}) {
  const calls = [];
  let remote = [...existing];
  return {
    calls, paginate: async () => remote,
    rest: {
      git: { getRef: async () => ({ data: { object: { type: "commit", sha } } }) },
      repos: {
        getReleaseByTag: async () => ({ data: { id: 7, tag_name: "v0.1.0", draft } }),
        listReleaseAssets: () => {},
        deleteReleaseAsset: async ({ asset_id }) => {
          calls.push("delete");
          remote = remote.filter(({ id }) => id !== asset_id);
        },
        uploadReleaseAsset: async ({ name, data }) => {
          calls.push(name);
          if (failure) throw new Error("upload failed");
          remote.push({ name, size: data.length, state: "uploaded" });
          return { data: { size: data.length } };
        },
        updateRelease: async (input) => calls.push(input),
      },
    },
  };
}

function input(github, directory) {
  return {
    github, directory, version: "0.1.0", sha: "tested",
    context: { repo: { owner: "owner", repo: "bureau" } }, core: { info: () => {} },
  };
}

test("requires the complete exact target set with valid checksums", async (t) => {
  const directory = await fixture(t);
  assert.equal((await readAssets(directory, "0.1.0")).size, 4);
  const name = assetNames("0.1.0")[0];
  await writeFile(join(directory, name), "corrupted");
  await assert.rejects(readAssets(directory, "0.1.0"), /mismatch/);
  await rm(join(directory, name));
  await assert.rejects(readAssets(directory, "0.1.0"), /every supported target/);
});

test("rejects extra artifacts and invalid versions", async (t) => {
  const directory = await fixture(t);
  await writeFile(join(directory, "unexpected"), "bytes");
  await assert.rejects(readAssets(directory, "0.1.0"), /exactly/);
  assert.throws(() => assetNames("../invalid"), /Invalid/);
});

test("publishes only after every archive and checksum is uploaded", async (t) => {
  const github = api();
  await publish(input(github, await fixture(t)));
  assert.deepEqual(github.calls.slice(0, -1), assetNames("0.1.0"));
  assert.equal(github.calls.at(-1).draft, false);
});

test("an upload failure leaves the release draft", async (t) => {
  const github = api({ failure: true });
  await assert.rejects(publish(input(github, await fixture(t))), /upload failed/);
  assert.equal(github.calls.some((call) => call.draft === false), false);
});

test("refuses to attach artifacts from a different commit", async (t) => {
  const github = api({ sha: "untested" });
  await assert.rejects(publish(input(github, await fixture(t))), /another commit/);
  assert.deepEqual(github.calls, []);
});

test("resumes a partially uploaded draft and does not modify published assets", async (t) => {
  const directory = await fixture(t);
  const existing = [{ id: 1, name: assetNames("0.1.0")[0] }];
  const draft = api({ existing });
  await publish(input(draft, directory));
  assert.equal(draft.calls[0], "delete");
  const released = api({ draft: false, existing: assetNames("0.1.0").map((name) => ({ name, size: 1 })) });
  await publish(input(released, directory));
  assert.deepEqual(released.calls, []);
});

test("unknown release assets and incomplete public releases fail closed", async (t) => {
  const directory = await fixture(t);
  await assert.rejects(publish(input(api({ existing: [{ name: "unexpected" }] }), directory)), /unexpected assets/);
  await assert.rejects(publish(input(api({ draft: false }), directory)), /missing/);
});

test("missing release output and API failures never masquerade as publication", async () => {
  for (const status of [404, 403, 500]) {
    const github = api();
    github.rest.repos.getReleaseByTag = async () => { throw Object.assign(new Error("API failed"), { status }); };
    github.rest.git.getRef = async () => { throw Object.assign(new Error("API failed"), { status }); };
    const operation = publish(input(github, "unused"));
    await assert.rejects(operation, status === 404 ? /did not create/ : /API failed/);
  }
});

test("recovers a tag created before draft creation failed", async (t) => {
  const github = api();
  github.rest.repos.getReleaseByTag = async () => { throw Object.assign(new Error("missing"), { status: 404 }); };
  github.rest.repos.createRelease = async (input) => {
    assert.equal(input.target_commitish, "tested");
    assert.equal(input.draft, true);
    return { data: { id: 7, tag_name: input.tag_name, draft: true } };
  };
  await publish(input(github, await fixture(t)));
  assert.equal(github.calls.at(-1).draft, false);
});
