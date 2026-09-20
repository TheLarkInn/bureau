import { createHash } from "node:crypto";
import { lstat, mkdir, readdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { readBoundedFile } from "../scripts/maintenance-files.mjs";
import { escapeHtml, renderPrinciples, renderScenarios, repository, source } from "./content.mjs";
import { validateLinks } from "./links.mjs";
import { isMain, normalizeBase, normalizeOrigin, siteDirectory } from "./paths.mjs";

export const publicFiles = [
  ".nojekyll", "404.html", "index.html",
  "assets/base.css", "assets/demo.mjs", "assets/layout.css", "assets/mark.svg", "assets/sample.css",
];
export const maximumBuildBytes = 256 * 1024;

function render(template, values) {
  const html = template.replace(/\{\{([A-Z]+)\}\}/gu, (_, key) => {
    if (!(key in values)) throw new Error(`Unknown template token: ${key}`);
    return values[key];
  });
  if (html.includes("{{")) throw new Error("Unresolved template token.");
  return html;
}

async function assertOutput(directory, prefix = "") {
  const entries = await readdir(directory, { withFileTypes: true });
  const allowed = new Set([...publicFiles, "site-manifest.json"]);
  for (const entry of entries) {
    const name = `${prefix}${entry.name}`;
    if (entry.isSymbolicLink()) throw new Error(`Refusing output symlink: ${name}`);
    if (entry.isDirectory() && name === "assets") await assertOutput(join(directory, entry.name), "assets/");
    else if (!entry.isFile() || !allowed.has(name)) throw new Error(`Unexpected output entry: ${name}`);
  }
}

export async function build({ base = "/bureau/", origin, out = join(siteDirectory, "dist") } = {}, openFile) {
  const path = normalizeBase(base);
  const canonical = normalizeOrigin(origin);
  const values = {
    BASE: path, REPOSITORY: repository, SOURCE: source,
    CANONICAL: canonical ? `<link rel="canonical" href="${escapeHtml(canonical + path)}">\n  <meta property="og:url" content="${escapeHtml(canonical + path)}">` : "",
    PRINCIPLES: renderPrinciples(), SCENARIOS: renderScenarios(),
  };
  const files = new Map();
  for (const name of publicFiles) {
    let bytes = name === ".nojekyll" ? Buffer.alloc(0)
      : await readBoundedFile(join(siteDirectory, "src", name), maximumBuildBytes, openFile);
    if (name.endsWith(".html")) bytes = Buffer.from(render(bytes.toString(), values));
    files.set(name, bytes);
  }
  const bytes = [...files.values()].reduce((total, file) => total + file.length, 0);
  if (bytes > maximumBuildBytes) throw new Error(`Site exceeds the ${maximumBuildBytes}-byte build budget.`);
  const links = validateLinks(files, path);
  await mkdir(out, { recursive: true });
  if ((await lstat(out)).isSymbolicLink()) throw new Error("Refusing a symlink output directory.");
  await assertOutput(out);
  for (const [name, content] of files) {
    await mkdir(dirname(join(out, name)), { recursive: true });
    await writeFile(join(out, name), content);
  }
  const manifest = {
    schema: "bureau-site-build-v1", base: path, bytes, links,
    files: Object.fromEntries([...files].map(([name, content]) => [name, {
      bytes: content.length, sha256: createHash("sha256").update(content).digest("hex"),
    }])),
  };
  await writeFile(join(out, "site-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return { directory: resolve(out), manifest, files };
}

if (isMain(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: {
      base: { type: "string", default: "/bureau/" },
      origin: { type: "string" }, out: { type: "string" },
    } });
    const result = await build(values);
    console.log(`Built ${Object.keys(result.manifest.files).length} files (${result.manifest.bytes} bytes), ${result.manifest.links} links; base ${result.manifest.base}`);
  } catch (error) {
    console.error(`Site build failed: ${error.message}`);
    process.exitCode = 1;
  }
}
