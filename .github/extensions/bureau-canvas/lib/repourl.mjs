// Resolving a repository URL into a `repos.yaml` registry entry.
//
// An assignment may only name repos the registry already lists, so adding an
// unlisted repo means writing the registry too. This module is the part that
// can be decided from the URL alone: the short name, the forge, and the clone
// URL. Access and credential are deliberately NOT guessed — they are grants,
// and a wrong grant is a security problem rather than a typo (DESIGN.md
// section 10). The caller must choose them.
//
// Pure and offline: no network, no DOM, no filesystem.

const SSH = /^(?:ssh:\/\/)?git@([^:/]+)[:/](.+?)(?:\.git)?$/u;

/**
 * Resolves a repository URL.
 *
 * Returns `{ ok: true, name, forge, url }` or `{ ok: false, reason }`. The
 * name is a suggestion — registry keys are the operator's to choose, and the
 * caller is expected to let them edit it before writing.
 */
export function resolveRepoUrl(input) {
  const text = String(input ?? "").trim();
  if (!text) {
    return { ok: false, reason: "paste a repository URL" };
  }
  const ssh = SSH.exec(text);
  if (ssh) {
    return fromSsh(ssh[1].toLowerCase(), ssh[2]);
  }
  let url;
  try {
    url = new URL(text);
  } catch {
    return { ok: false, reason: `not a repository URL: ${text}` };
  }
  const host = url.hostname.toLowerCase();
  if (host === "github.com" || host === "www.github.com") {
    return fromGithub(url);
  }
  if (host === "dev.azure.com" || host.endsWith(".visualstudio.com")) {
    return fromAzureDevOps(url);
  }
  return { ok: false, reason: `unrecognized host \`${url.hostname}\` — expected github.com, dev.azure.com, or an *.visualstudio.com organization` };
}

function fromSsh(host, path) {
  const parts = path.split("/").filter(Boolean);
  if (host === "github.com" && parts.length >= 2) {
    const name = stripGit(parts[1]);
    return found(name, "github", `https://github.com/${parts[0]}/${name}.git`);
  }
  if (host.endsWith("visualstudio.com") || host === "ssh.dev.azure.com") {
    const name = parts.at(-1);
    return name ? found(stripGit(name), "ado", `git@${host}:${path}`) : missingRepo();
  }
  return { ok: false, reason: `cannot tell which repository \`git@${host}:${path}\` names` };
}

function fromGithub(url) {
  const parts = segments(url);
  if (parts.length < 2) {
    return { ok: false, reason: "no owner/repository in that GitHub URL" };
  }
  const name = stripGit(parts[1]);
  return found(name, "github", `https://github.com/${parts[0]}/${name}.git`);
}

/**
 * Azure DevOps clone URLs put the repository after `_git`. A board or work
 * item URL has no `_git` segment and names no repository at all, which is a
 * refusal rather than a guess — the work source and the repos are separate
 * settings (DESIGN.md section 6).
 */
function fromAzureDevOps(url) {
  const parts = segments(url);
  const at = parts.indexOf("_git");
  const name = at >= 0 ? parts[at + 1] : null;
  if (!name) {
    return missingRepo();
  }
  return found(stripGit(name), "ado", `${url.origin}${trimmedPath(url, parts, at, name)}`);
}

/** The clone path, dropping anything after the repository name. */
function trimmedPath(url, parts, at, name) {
  const kept = parts.slice(0, at + 1).concat(name);
  return `/${kept.map(encodeURIComponent).join("/")}`;
}

function missingRepo() {
  return { ok: false, reason: "that Azure DevOps URL names no repository — open the repository itself and copy its clone URL" };
}

function segments(url) {
  return url.pathname.split("/").filter(Boolean).map(decode);
}

function decode(part) {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

function stripGit(name) {
  return name.replace(/\.git$/u, "");
}

function found(name, forge, url) {
  return { ok: true, name, forge, url };
}
