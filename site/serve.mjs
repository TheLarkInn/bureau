import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { lstat, readFile } from "node:fs/promises";
import { extname, join } from "node:path";
import { parseArgs } from "node:util";
import { publicFiles, maximumBuildBytes } from "./build.mjs";
import { isMain, normalizeBase, siteDirectory } from "./paths.mjs";

const types = { ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8", ".mjs": "text/javascript; charset=utf-8", ".svg": "image/svg+xml" };

async function readBuild(directory) {
  for (const name of ["", "assets", "site-manifest.json"]) {
    const info = await lstat(join(directory, name));
    if (info.isSymbolicLink()) throw new Error(`Invalid build symlink: ${name}`);
    if (name === "site-manifest.json" && (!info.isFile() || info.size > 16 * 1024)) {
      throw new Error("Invalid build manifest file.");
    }
  }
  const manifest = JSON.parse(await readFile(join(directory, "site-manifest.json"), "utf8"));
  if (manifest.schema !== "bureau-site-build-v1"
    || JSON.stringify(Object.keys(manifest.files).sort()) !== JSON.stringify([...publicFiles].sort())) {
    throw new Error("Build manifest does not contain the exact public file set.");
  }
  normalizeBase(manifest.base);
  const files = new Map();
  for (const name of publicFiles) {
    const info = await lstat(join(directory, name));
    if (!info.isFile() || info.isSymbolicLink() || info.size > maximumBuildBytes) {
      throw new Error(`Invalid public file: ${name}`);
    }
    const bytes = await readFile(join(directory, name));
    const expected = manifest.files[name];
    if (bytes.length !== expected.bytes || createHash("sha256").update(bytes).digest("hex") !== expected.sha256) {
      throw new Error(`Build changed after validation: ${name}`);
    }
    files.set(name, bytes);
  }
  const total = [...files.values()].reduce((sum, file) => sum + file.length, 0);
  if (total > maximumBuildBytes || total !== manifest.bytes) throw new Error("Build byte budget mismatch.");
  return { files, base: manifest.base };
}

function send(response, status, bytes, type, head) {
  response.writeHead(status, {
    "content-type": type, "content-length": bytes.length,
    "cache-control": "no-store", "x-content-type-options": "nosniff",
    "referrer-policy": "no-referrer", "x-frame-options": "DENY",
  });
  response.end(head ? undefined : bytes);
}

export async function serve({ directory = join(siteDirectory, "dist"), port = 0 } = {}) {
  if (!Number.isSafeInteger(port) || port < 0 || port > 65535) throw new Error("Port must be an integer from 0 to 65535.");
  const { files, base } = await readBuild(directory);
  const server = createServer((request, response) => {
    const head = request.method === "HEAD";
    if (!["GET", "HEAD"].includes(request.method)) {
      response.setHeader("Allow", "GET, HEAD");
      return send(response, 405, Buffer.from("Read-only preview."), "text/plain", head);
    }
    if (![`127.0.0.1:${server.address().port}`, `localhost:${server.address().port}`].includes(request.headers.host)) {
      return send(response, 403, Buffer.from("Loopback preview only."), "text/plain", head);
    }
    let path;
    try { path = new URL(request.url, "http://127.0.0.1").pathname; }
    catch { return send(response, 400, Buffer.from("Invalid request target."), "text/plain", head); }
    if (base !== "/" && path === base.slice(0, -1)) {
      response.writeHead(308, { location: base });
      return response.end();
    }
    const name = path.startsWith(base) ? path.slice(base.length) || "index.html" : null;
    const content = files.get(name);
    const type = content ? types[extname(name)] ?? "text/plain; charset=utf-8" : "text/html; charset=utf-8";
    return send(response, content ? 200 : 404, content ?? files.get("404.html"), type, head);
  });
  server.requestTimeout = 5000;
  server.headersTimeout = 5000;
  server.maxConnections = 16;
  await new Promise((accept, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", accept);
  });
  return {
    url: `http://127.0.0.1:${server.address().port}${base}`,
    close: () => new Promise((accept, reject) => {
      server.close((error) => error ? reject(error) : accept());
      server.closeAllConnections();
    }),
  };
}

if (isMain(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: {
      dir: { type: "string", default: join(siteDirectory, "dist") },
      port: { type: "string", default: "4173" },
    } });
    const preview = await serve({ directory: values.dir, port: Number(values.port) });
    console.log(`Bureau static preview: ${preview.url}`);
    for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, () => { void preview.close(); });
  } catch (error) {
    console.error(`Site preview failed: ${error.message}`);
    process.exitCode = 1;
  }
}
