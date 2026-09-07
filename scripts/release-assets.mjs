import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

export const targets = ["aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl"];

export function assetNames(version) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error("Invalid release version.");
  return targets.flatMap((target) => {
    const archive = `bureau-v${version}-${target}.tar.gz`;
    return [archive, `${archive}.sha256`];
  });
}

export async function readAssets(directory, version) {
  const names = assetNames(version);
  const actual = (await readdir(directory)).sort();
  if (JSON.stringify(actual) !== JSON.stringify([...names].sort())) {
    throw new Error("Release assets must contain exactly one archive and checksum for every supported target.");
  }
  const assets = new Map(await Promise.all(names.map(async (name) =>
    [name, await readFile(join(directory, name))])));
  for (const name of names.filter((name) => name.endsWith(".tar.gz"))) {
    const bytes = assets.get(name);
    const digest = createHash("sha256").update(bytes).digest("hex");
    if (bytes.length === 0 || assets.get(`${name}.sha256`).toString().trim() !== `${digest}  ${name}`) {
      throw new Error(`Release archive/checksum mismatch: ${name}`);
    }
  }
  return assets;
}

export async function tagCommit(github, repo, tag) {
  const { data: ref } = await github.rest.git.getRef({ ...repo, ref: `tags/${tag}` });
  let object = ref.object;
  if (object.type === "tag") {
    const { data } = await github.rest.git.getTag({ ...repo, tag_sha: object.sha });
    object = data.object;
  }
  if (object.type !== "commit") throw new Error("Release tag does not resolve to a commit.");
  return object.sha;
}

async function releaseForTag(github, repo, tag) {
  try {
    const { data } = await github.rest.repos.getReleaseByTag({ ...repo, tag });
    return data;
  } catch (error) {
    if (error.status === 404) return null;
    throw error;
  }
}

async function restoreDraft(github, repo, tag, sha) {
  let commit;
  try {
    commit = await tagCommit(github, repo, tag);
  } catch (error) {
    if (error.status === 404) return null;
    throw error;
  }
  if (commit !== sha) throw new Error("Orphaned release tag belongs to another commit.");
  const { data } = await github.rest.repos.createRelease({
    ...repo, tag_name: tag, target_commitish: sha, name: `bureau ${tag}`, draft: true,
    body: `Recovered automated release. See [the changelog](https://github.com/${repo.owner}/${repo.repo}/blob/${sha}/CHANGELOG.md).`,
  });
  return data;
}

async function uploadAssets(github, repo, release, assets) {
  const existing = await github.paginate(github.rest.repos.listReleaseAssets, {
    ...repo, release_id: release.id, per_page: 100,
  });
  if (existing.some(({ name }) => !assets.has(name))) {
    throw new Error("Draft release contains unexpected assets; refusing to publish it.");
  }
  for (const [name, data] of assets) {
    const old = existing.find((asset) => asset.name === name);
    if (old) await github.rest.repos.deleteReleaseAsset({ ...repo, asset_id: old.id });
    const { data: uploaded } = await github.rest.repos.uploadReleaseAsset({
      ...repo, release_id: release.id, name, data,
      headers: { "content-type": "application/octet-stream", "content-length": data.length },
    });
    if (uploaded.size !== data.length) throw new Error(`Incomplete upload: ${name}`);
  }
  const uploaded = await github.paginate(github.rest.repos.listReleaseAssets, {
    ...repo, release_id: release.id, per_page: 100,
  });
  if (uploaded.length !== assets.size
      || uploaded.some(({ name, size, state }) => size !== assets.get(name)?.length || state !== "uploaded")) {
    throw new Error("Remote release assets are incomplete; leaving the release draft.");
  }
}

export async function publish({ github, context, directory, version, sha, core }) {
  const release = await releaseForTag(github, context.repo, `v${version}`)
    ?? await restoreDraft(github, context.repo, `v${version}`, sha);
  if (!release) {
    throw new Error("Release-plz did not create the expected version tag/release.");
  }
  const commit = await tagCommit(github, context.repo, release.tag_name);
  if (commit !== sha) {
    if (release.draft) throw new Error("Pending draft belongs to another commit; rerun its original CI run.");
    core.info("This version is already released from an earlier commit.");
    return;
  }
  const assets = await readAssets(directory, version);
  if (!release.draft) {
    const existing = await github.paginate(github.rest.repos.listReleaseAssets, {
      ...context.repo, release_id: release.id, per_page: 100,
    });
    if (existing.length !== assets.size || existing.some(({ name, size }) => !assets.has(name) || !size)) {
      throw new Error("Published release is missing its complete asset set.");
    }
    core.info("Release is already published; leaving its immutable assets unchanged.");
    return;
  }
  await uploadAssets(github, context.repo, release, assets);
  await github.rest.repos.updateRelease({
    ...context.repo, release_id: release.id, draft: false, make_latest: "true",
  });
}
