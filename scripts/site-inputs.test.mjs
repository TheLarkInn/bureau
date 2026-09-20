import assert from "node:assert/strict";
import { appendFile, lstat, mkdir, mkdtemp, open, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import test from "node:test";
import { build, maximumBuildBytes } from "../site/build.mjs";
import { siteDirectory } from "../site/paths.mjs";
import { serve } from "../site/serve.mjs";

async function fixture(t) {
  const cache = join(siteDirectory, ".cache");
  await mkdir(cache, { recursive: true });
  const directory = await mkdtemp(join(cache, "input-test-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  return directory;
}

function observedOpen(target, { replacement = target, growBy = 0 } = {}) {
  const observed = { bufferBytes: [], bytesRead: 0, closed: false };
  return {
    observed,
    async open(path, flags) {
      const handle = await open(path === target ? replacement : path, flags);
      if (path !== target) return handle;
      return {
        async stat() {
          const metadata = await handle.stat();
          if (growBy) await appendFile(replacement, Buffer.alloc(growBy, 32));
          return metadata;
        },
        async read(buffer, offset, length, position) {
          observed.bufferBytes.push(buffer.length);
          const result = await handle.read(buffer, offset, length, position);
          observed.bytesRead += result.bytesRead;
          return result;
        },
        async close() {
          await handle.close();
          observed.closed = true;
        },
      };
    },
  };
}

async function openAndClose(directory, openFile) {
  const preview = await serve({ directory }, openFile);
  await preview.close();
}

test("oversized source is rejected before reading or rendering its bytes", async (t) => {
  const directory = await fixture(t);
  const replacement = join(directory, "oversized.html");
  await writeFile(replacement, Buffer.alloc(maximumBuildBytes + 1, 32));
  const input = observedOpen(join(siteDirectory, "src", "index.html"), { replacement });
  const out = join(directory, "output");
  await assert.rejects(build({ out }, input.open), /byte limit/u);
  assert.deepEqual(input.observed, { bufferBytes: [], bytesRead: 0, closed: true });
  await assert.rejects(lstat(out), { code: "ENOENT" });
});

test("source growth after descriptor stat cannot read beyond the ceiling plus one byte", async (t) => {
  const directory = await fixture(t);
  const replacement = join(directory, "growing.html");
  await writeFile(replacement, "small");
  const input = observedOpen(join(siteDirectory, "src", "index.html"), {
    replacement, growBy: maximumBuildBytes + 1,
  });
  await assert.rejects(build({ out: join(directory, "output") }, input.open), /grew beyond/u);
  assert.deepEqual(input.observed, {
    bufferBytes: [maximumBuildBytes + 1], bytesRead: maximumBuildBytes + 1, closed: true,
  });
});

for (const [name, maximum, oversized] of [
  ["site-manifest.json", 16 * 1024, /Invalid build manifest file/u],
  ["assets/base.css", maximumBuildBytes, /Invalid public file/u],
]) {
  test(`preview rejects oversized ${name} before listening`, async (t) => {
    const directory = await fixture(t);
    await build({ out: directory });
    await writeFile(join(directory, name), Buffer.alloc(maximum + 1, 32));
    await assert.rejects(openAndClose(directory), oversized);
  });

  test(`preview bounds ${name} growing after descriptor stat`, async (t) => {
    const directory = await fixture(t);
    await build({ out: directory });
    const input = observedOpen(join(directory, name), { growBy: maximum + 1 });
    await assert.rejects(openAndClose(directory, input.open), /grew beyond/u);
    assert.deepEqual(input.observed, {
      bufferBytes: [maximum + 1], bytesRead: maximum + 1, closed: true,
    });
  });
}

test("preview rejects invalid UTF-8 in a manifest rather than replacing its bytes", async (t) => {
  const directory = await fixture(t);
  await build({ out: directory });
  const path = join(directory, "site-manifest.json");
  const json = (await readFile(path, "utf8")).trimEnd();
  await writeFile(path, Buffer.concat([
    Buffer.from(`${json.slice(0, -1)},"note":"`), Buffer.from([0xff]), Buffer.from('"}'),
  ]));
  await assert.rejects(openAndClose(directory), /encoded data.*not valid/u);
});
