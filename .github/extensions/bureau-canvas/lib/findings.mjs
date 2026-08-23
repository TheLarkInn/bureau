import { constants } from "node:fs";
import { access, stat } from "node:fs/promises";
import { spawn } from "node:child_process";
import { delimiter, dirname, extname, join, parse, resolve } from "node:path";

export const FINDING_SOURCES = {
  validate: "bureau-validate",
  advisory: "advisory",
};

export async function findings(dir, options = {}) {
  const cwd = options.cwd ?? process.cwd();
  const env = options.env ?? process.env;
  const resolvedDir = resolve(cwd, dir);
  if (!(await isDirectory(resolvedDir))) {
    return distinctState("dir-missing", dir, `Config directory does not exist: ${resolvedDir}`);
  }

  const candidates = await locateCandidates(resolvedDir, { cwd, env, binary: options.binary, candidates: options.candidates });
  if (candidates.length === 0) {
    return distinctState("binary-missing", dir, "Could not find bureau on PATH or at target/debug/bureau.");
  }

  return firstUsable(candidates, resolvedDir, dir, { cwd, env });
}

/**
 * Tries each candidate until one returns JSON.
 *
 * A `bureau` installed on PATH can predate `validate --json` while a current
 * build sits in the workspace. Preferring PATH unconditionally then reports a
 * crash on a machine that has everything it needs, so version skew is detected
 * and skipped rather than surfaced as a failure.
 */
async function firstUsable(candidates, resolvedDir, dir, options) {
  let unsupported = null;
  for (const candidate of candidates) {
    const run = await runValidate(candidate, resolvedDir, options);
    const parsed = parsePayload(run.stdout);
    if (parsed.ok) {
      return validatedState(parsed.value, dir, run.code);
    }
    if (lacksJsonFlag(run)) {
      unsupported = unsupported ?? { candidate, run };
      continue;
    }
    return crashState(dir, run, parsed.error);
  }
  return unsupportedState(dir, unsupported);
}

function lacksJsonFlag(run) {
  return /unexpected argument\s+'?--json'?/u.test(`${run.stderr ?? ""}${run.stdout ?? ""}`);
}

function unsupportedState(dir, unsupported) {
  const where = unsupported?.candidate?.args?.at(-1) ?? unsupported?.candidate?.command ?? "bureau";
  return distinctState(
    "unsupported-binary",
    dir,
    `\`${where}\` does not support \`validate --json\`; rebuild it or set BUREAU_CANVAS_BUREAU to one that does.`,
  );
}

async function locateCandidates(dir, options) {
  if (options.candidates) {
    const resolved = await Promise.all(options.candidates.map((candidate) => commandFor(candidate)));
    return resolved.filter(Boolean);
  }
  return bureauCandidates({ binary: options.binary, cwd: options.cwd, env: options.env, anchor: dir });
}

/**
 * Every usable `bureau` invocation, most-preferred first: an explicit
 * override, then `PATH`, then the workspace's `target/debug/`. Shared by
 * config validation and the run verbs so they all honor the same lookup.
 */
export async function bureauCandidates(options = {}) {
  const env = options.env ?? process.env;
  const cwd = options.cwd ?? process.cwd();
  const explicit = options.binary ?? env.BUREAU_CANVAS_BUREAU;
  if (explicit) {
    const command = await commandFor(explicit);
    return command ? [command] : [];
  }

  const anchor = options.anchor ?? cwd;
  const found = [await findOnPath("bureau", env), await findInWorkspace(anchor, cwd)];
  return found.filter(Boolean).map((command) => wslBridged(command));
}

/**
 * A binary living on a `\\wsl.localhost\<distro>\...` share is a Linux
 * executable that a Windows process cannot exec directly. Invoke it through
 * `wsl.exe` instead, and translate the paths it is handed.
 */
export function wslBridged(command, platform = process.platform) {
  const share = platform === "win32" ? wslShare(command.command) : null;
  if (!share || command.args.length > 0) {
    return command;
  }
  return {
    command: "wsl.exe",
    args: ["-d", share.distro, "--", share.path],
    distro: share.distro,
    translate: (path) => wslShare(path)?.path ?? path,
  };
}

export function wslShare(path) {
  const match = /^\\\\wsl(?:\.localhost|\$)\\([^\\]+)\\(.*)$/u.exec(String(path));
  if (!match) {
    return null;
  }
  return { distro: match[1], path: `/${match[2].replaceAll("\\", "/")}` };
}

/**
 * The inverse of `wslShare`: how this host addresses a distro-local path.
 * `translate` turns the result back into `linuxPath` when it is handed to a
 * bridged binary, so a share path is safe to use as both a filesystem path
 * here and a CLI argument there.
 */
export function wslSharePath(distro, linuxPath) {
  const relative = String(linuxPath).replace(/^\/+/u, "").replaceAll("/", "\\");
  return `\\\\wsl.localhost\\${distro}\\${relative}`;
}

async function findOnPath(name, env) {
  for (const directory of pathDirectories(env)) {
    for (const candidate of commandNames(name, env)) {
      const command = await commandFor(join(directory, candidate));
      if (command) {
        return command;
      }
    }
  }
  return null;
}

async function findInWorkspace(dir, cwd) {
  for (const root of uniqueAncestors([dir, cwd])) {
    for (const name of commandNames("bureau", process.env)) {
      const command = await commandFor(join(root, "target", "debug", name));
      if (command) {
        return command;
      }
    }
  }
  return null;
}

function pathDirectories(env) {
  return String(env.PATH ?? env.Path ?? "")
    .split(delimiter)
    .filter(Boolean);
}

function commandNames(name, env) {
  if (process.platform !== "win32") {
    return [name, `${name}.mjs`];
  }
  const extensions = String(env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD")
    .split(";")
    .filter(Boolean)
    .map((value) => value.toLowerCase());
  return [name, `${name}.mjs`, ...extensions.map((extension) => `${name}${extension}`)];
}

function uniqueAncestors(paths) {
  const seen = new Set();
  return paths.flatMap((path) => {
    const roots = ancestors(path);
    return roots.filter((root) => {
      const known = seen.has(root);
      seen.add(root);
      return !known;
    });
  });
}

function ancestors(path) {
  const roots = [];
  for (let current = resolve(path); current !== dirname(current); current = dirname(current)) {
    roots.push(current);
  }
  roots.push(dirname(roots.at(-1) ?? path));
  return roots;
}

async function commandFor(path) {
  if (!(await canRun(path, extname(path) === ".mjs"))) {
    return null;
  }
  return extname(path) === ".mjs" ? { command: process.execPath, args: [path] } : { command: path, args: [] };
}

async function canRun(path, isModule) {
  try {
    const mode = process.platform === "win32" || isModule ? constants.F_OK : constants.F_OK | constants.X_OK;
    await access(path, mode);
    return true;
  } catch {
    return false;
  }
}

async function isDirectory(path) {
  const info = await stat(path).catch(() => null);
  return Boolean(info?.isDirectory());
}

function runValidate(bureau, dir, options) {
  const target = bureau.translate ? bureau.translate(dir) : dir;
  const args = [...bureau.args, "validate", target, "--json"];
  const child = spawn(bureau.command, args, { cwd: options.cwd, env: options.env, windowsHide: true });
  return collectRun(child);
}

function collectRun(child) {
  return new Promise((resolveRun) => {
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8").on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => resolveRun({ code: null, stdout, stderr, error }));
    child.on("close", (code) => resolveRun({ code, stdout, stderr }));
  });
}

function parsePayload(stdout) {
  try {
    return { ok: true, value: JSON.parse(stdout) };
  } catch (error) {
    return { ok: false, error };
  }
}

function validatedState(payload, fallbackDir, exitCode) {
  const errors = Array.isArray(payload.errors) ? payload.errors : [];
  return {
    ok: Boolean(payload.ok),
    state: "validated",
    dir: payload.dir ?? fallbackDir,
    errors,
    config: payload.config ?? null,
    findings: errors.map(errorFinding),
    exitCode,
  };
}

function distinctState(state, dir, message) {
  return { ok: false, state, dir, message, errors: [], config: null, findings: [] };
}

function crashState(dir, run, error) {
  return {
    ...distinctState("crash", dir, `bureau validate did not return JSON: ${error.message}`),
    exitCode: run.code,
    stderr: run.stderr,
  };
}

function errorFinding(error) {
  return {
    source: FINDING_SOURCES.validate,
    marker: "validation",
    path: error.path ?? "",
    message: error.message ?? "",
    target: targetFor(error),
  };
}

function targetFor(error) {
  const pathTarget = targetFromPath(error.path ?? "");
  const step = stepFromMessage(error.message ?? "");
  if (pathTarget.kind === "pipeline" && step?.pipeline === pathTarget.pipeline) {
    return { kind: "step", pipeline: pathTarget.pipeline, step: step.step };
  }
  return pathTarget;
}

function targetFromPath(path) {
  const normalized = path.replaceAll("\\", "/");
  const parts = normalized.split("/");
  if (parts.length === 2 && ["pipelines", "roles", "assignments"].includes(parts[0])) {
    return namedTarget(parts[0], parts[1]);
  }
  if (normalized === "repos.yaml" || normalized === "repos.yml") {
    return { kind: "repos", path };
  }
  return { kind: "file", path };
}

function namedTarget(directory, file) {
  const kinds = { pipelines: "pipeline", roles: "role", assignments: "assignment" };
  return { kind: kinds[directory], [kinds[directory]]: parse(file).name, path: `${directory}/${file}` };
}

function stepFromMessage(message) {
  const match = /^pipeline `([^`]+)` step `([^`]+)`: /.exec(message);
  return match ? { pipeline: match[1], step: match[2] } : null;
}
