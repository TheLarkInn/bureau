// The filesystem side of config CRUD.
//
// Mirrors what `crates/bureau/src/config/mod.rs` does when it loads: `roles/`,
// `assignments/` and `pipelines/` are scanned non-recursively for `.yaml` and
// `.yml`, `repos.yaml` is a single file at the root rather than a directory of
// entities, and a file's stem must equal the `name` declared inside it
// (`insert_named`), or the loader reports a mismatch.
//
// Names arriving here come from a person or an agent and become filesystem
// paths, so an unsafe name is refused rather than sanitized: rewriting a name
// to something safe produces a file whose stem no longer matches its declared
// `name`, which is the same loader error arriving later and further from its
// cause.

import { mkdir, readdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { isAbsolute, join, relative, resolve, sep } from "node:path";

/** Which subdirectory each kind lives in; `repo` is a map inside one file. */
const DIRECTORIES = {
  role: "roles",
  assignment: "assignments",
  pipeline: "pipelines",
};

const REPOS_FILE = "repos.yaml";
const YAML_EXTENSIONS = [".yaml", ".yml"];

export function kinds() {
  return [...Object.keys(DIRECTORIES), "repo"];
}

/** Absolute path for one config entity. Throws on an unsafe or unknown name. */
export function pathFor(dir, kind, name) {
  const root = resolve(dir);
  if (kind === "repo") {
    return join(root, REPOS_FILE);
  }
  const directory = DIRECTORIES[kind];
  if (!directory) {
    throw new Error(`unknown config kind \`${kind}\``);
  }
  return contained(root, join(root, directory, `${safeName(name)}.yaml`), name);
}

export async function listNames(dir, kind) {
  if (kind === "repo") {
    return (await exists(pathFor(dir, kind))) ? ["repos"] : [];
  }
  const directory = DIRECTORIES[kind];
  if (!directory) {
    throw new Error(`unknown config kind \`${kind}\``);
  }
  const entries = await readdir(join(resolve(dir), directory), { withFileTypes: true }).catch(() => []);
  return entries
    .filter((entry) => entry.isFile() && YAML_EXTENSIONS.some((extension) => entry.name.endsWith(extension)))
    .map((entry) => entry.name.replace(/\.(ya?ml)$/u, ""))
    .sort();
}

export async function createFile(dir, kind, name, text) {
  const path = pathFor(dir, kind, name);
  if (await exists(path)) {
    throw new Error(`\`${name}\` already exists at ${path}`);
  }
  await writeText(path, text);
  return path;
}

export async function writeExisting(dir, kind, name, text) {
  const path = pathFor(dir, kind, name);
  await writeText(path, text);
  return path;
}

export async function deleteFile(dir, kind, name) {
  const path = pathFor(dir, kind, name);
  await refuseSymlink(path);
  await rm(path, { force: true });
  return path;
}

/**
 * Moves one entity's file. The caller owns rewriting the `name` field, so the
 * declared name in `text` is checked against the destination stem here — a
 * move without the field rewrite produces config the loader refuses.
 */
export async function renameFile(dir, kind, from, to, text) {
  if (kind === "repo") {
    throw new Error("`repos.yaml` is a single file and cannot be renamed");
  }
  const source = pathFor(dir, kind, from);
  const target = pathFor(dir, kind, to);
  await refuseSymlink(source);
  if (await exists(target)) {
    throw new Error(`cannot rename \`${from}\` to \`${to}\`: \`${to}\` already exists`);
  }
  await requireDeclaredName(source, to, text);
  await mkdir(join(resolve(dir), DIRECTORIES[kind]), { recursive: true });
  await rename(source, target);
  return target;
}

async function requireDeclaredName(source, to, text) {
  const contents = text ?? (await readFile(source, "utf8"));
  const declared = declaredName(contents);
  if (declared !== null && declared !== to) {
    throw new Error(`renaming to \`${to}\` needs the declared name updated too; it is \`${declared}\``);
  }
}

/** The `name:` field, read without a YAML parse so this module stays standalone. */
function declaredName(text) {
  const match = /^name:[ \t]*(?:"([^"]*)"|'([^']*)'|([^\s#]+))/mu.exec(text.replaceAll("\r\n", "\n"));
  if (!match) {
    return null;
  }
  return match[1] ?? match[2] ?? match[3];
}

async function writeText(path, text) {
  await refuseSymlink(path);
  await mkdir(resolve(path, ".."), { recursive: true });
  await writeFile(path, text);
}

function safeName(name) {
  if (typeof name !== "string" || name.trim() === "") {
    throw new Error("config name must be a non-empty string");
  }
  if (name.includes("/") || name.includes("\\") || name.split(/[/\\]/u).includes("..") || isAbsolute(name)) {
    throw new Error(`config name \`${name}\` must not contain a path separator or \`..\``);
  }
  return name;
}

function contained(root, candidate, name) {
  const away = relative(root, candidate);
  if (away.startsWith("..") || isAbsolute(away) || away.split(sep).includes("..")) {
    throw new Error(`config name \`${name}\` escapes the config directory`);
  }
  return candidate;
}

async function refuseSymlink(path) {
  const info = await stat(path).catch(() => null);
  if (info && !info.isFile()) {
    throw new Error(`config path is not a regular file: ${path}`);
  }
}

async function exists(path) {
  return Boolean(await stat(path).catch(() => null));
}
