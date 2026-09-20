import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { build } from "./build.mjs";
import { siteDirectory } from "./paths.mjs";
import { serve } from "./serve.mjs";

export async function temporaryPreview(base) {
  const cache = join(siteDirectory, ".cache");
  await mkdir(cache, { recursive: true });
  const directory = await mkdtemp(join(cache, "preview-"));
  try {
    const result = await build({ base, out: directory });
    const server = await serve({ directory });
    return {
      url: server.url, manifest: result.manifest,
      async close() {
        try { await server.close(); }
        finally { await rm(directory, { recursive: true, force: true }); }
      },
    };
  } catch (error) {
    await rm(directory, { recursive: true, force: true });
    throw error;
  }
}

export async function onlyLocalRequests(context, origin, failures) {
  await context.route("**/*", async (route) => {
    if (new URL(route.request().url()).origin === origin) await route.continue();
    else {
      failures.push("The page attempted a non-local request.");
      await route.abort("blockedbyclient");
    }
  });
}
