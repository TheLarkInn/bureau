import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

export const siteDirectory = fileURLToPath(new URL(".", import.meta.url));
export const repositoryDirectory = resolve(siteDirectory, "..");

export function isMain(url) {
  return Boolean(process.argv[1]) && resolve(process.argv[1]) === fileURLToPath(url);
}

export function normalizeBase(base) {
  if (typeof base !== "string" || base.length > 160 || !/^\/(?:[A-Za-z0-9_-]+\/)*$/u.test(base)) {
    throw new Error("Base must be / or slash-delimited path segments, such as /bureau/.");
  }
  return base;
}

export function normalizeOrigin(value) {
  if (value === undefined) return null;
  const origin = new URL(value);
  if (origin.protocol !== "https:" || origin.username || origin.password
    || origin.pathname !== "/" || origin.search || origin.hash) {
    throw new Error("Origin must be an HTTPS origin without credentials, a path, query, or fragment.");
  }
  return origin.origin;
}
