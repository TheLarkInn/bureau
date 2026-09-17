import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { source } from "./content.mjs";
import { isMain, repositoryDirectory, siteDirectory } from "./paths.mjs";

const documents = new Set([
  "DESIGN.md", "LICENSE", ".github/extensions/bureau-canvas/README.md",
  "docs/getting-started.md", "docs/scenarios.md", "docs/github-cloud-factories.md",
]);

export function markdownAnchors(markdown) {
  const anchors = new Set();
  const duplicates = new Map();
  let fenced = false;
  for (const line of markdown.split("\n")) {
    if (/^\s*```/u.test(line)) fenced = !fenced;
    const heading = !fenced && /^#{1,6}\s+(.+)$/u.exec(line);
    if (!heading) continue;
    const slug = heading[1].trim().toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu, "").replace(/\s/gu, "-");
    const count = duplicates.get(slug) ?? 0;
    duplicates.set(slug, count + 1);
    anchors.add(count ? `${slug}-${count}` : slug);
  }
  for (const [, anchor] of markdown.matchAll(/<a\s+(?:id|name)="([^"]+)"/gu)) anchors.add(anchor);
  return anchors;
}

export async function verifySourceLinks(html, readSource = (path) => readFile(join(repositoryDirectory, path), "utf8")) {
  const links = [...html.matchAll(/\bhref="([^"]+)"/gu)]
    .map(([, href]) => href).filter((href) => href.startsWith(`${source}/`));
  if (!links.length) throw new Error("No source links were checked.");
  const read = new Map();
  for (const link of links) {
    const url = new URL(link);
    const path = url.pathname.slice("/TheLarkInn/bureau/blob/main/".length);
    if (!documents.has(path)) throw new Error(`Source link is outside the documented file contract: ${path}`);
    if (!read.has(path)) read.set(path, await readSource(path));
    if (url.hash && !markdownAnchors(read.get(path)).has(decodeURIComponent(url.hash.slice(1)))) {
      throw new Error(`Source anchor does not exist: ${path}${url.hash}`);
    }
  }
  return links.length;
}

if (isMain(import.meta.url)) {
  try {
    const html = await readFile(join(siteDirectory, "dist", "index.html"), "utf8");
    console.log(`Verified ${await verifySourceLinks(html)} source links against this checkout, offline.`);
  } catch (error) {
    console.error(`Source link check failed: ${error.message}`);
    process.exitCode = 1;
  }
}
