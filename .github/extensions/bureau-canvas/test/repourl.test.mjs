// lib/repourl.mjs: resolving a repository URL into a registry entry.
// Offline and pure — no network, no DOM.

import assert from "node:assert/strict";
import test from "node:test";

import { resolveRepoUrl } from "../lib/repourl.mjs";

test("a GitHub repository URL yields name, forge, and clone URL", () => {
  const resolved = resolveRepoUrl("https://github.com/microsoft/rushstack");

  assert.deepEqual(
    { name: resolved.name, forge: resolved.forge, url: resolved.url },
    { name: "rushstack", forge: "github", url: "https://github.com/microsoft/rushstack.git" },
  );
});

test("an Azure DevOps clone URL takes the repository after _git", () => {
  const resolved = resolveRepoUrl("https://onedrive.visualstudio.com/ODSP-Web/_git/odsp-web");

  assert.deepEqual(
    { name: resolved.name, forge: resolved.forge, url: resolved.url },
    { name: "odsp-web", forge: "ado", url: "https://onedrive.visualstudio.com/ODSP-Web/_git/odsp-web" },
  );
});

test("extra path segments after the repository are dropped", () => {
  const cases = [
    "https://dev.azure.com/onedrive/ODSP-Web/_git/odsp-web?path=/README.md",
    "https://dev.azure.com/onedrive/ODSP-Web/_git/odsp-web/commit/abc123",
  ];
  const urls = cases.map((input) => resolveRepoUrl(input).url);

  assert.deepEqual(urls, cases.map(() => "https://dev.azure.com/onedrive/ODSP-Web/_git/odsp-web"));
});

test("a trailing .git is not part of the registry name", () => {
  const names = [
    "https://github.com/microsoft/rushstack.git",
    "git@github.com:microsoft/rushstack.git",
  ].map((input) => resolveRepoUrl(input).name);

  assert.deepEqual(names, ["rushstack", "rushstack"]);
});

test("an ssh GitHub remote resolves to the https clone URL", () => {
  const resolved = resolveRepoUrl("git@github.com:TheLarkInn/bureau.git");

  assert.deepEqual(
    { name: resolved.name, url: resolved.url },
    { name: "bureau", url: "https://github.com/TheLarkInn/bureau.git" },
  );
});

test("a board URL is refused, because it names no repository", () => {
  const resolved = resolveRepoUrl("https://onedrive.visualstudio.com/EFun/_boards/board/t/Web/Backlog%20items");

  assert.deepEqual(
    { ok: resolved.ok, explains: resolved.reason.includes("names no repository") },
    { ok: false, explains: true },
  );
});

test("input this cannot resolve is refused with a reason instead of a guess", () => {
  const refusals = [
    resolveRepoUrl(""),
    resolveRepoUrl("rushstack"),
    resolveRepoUrl("https://gitlab.com/owner/repo"),
    resolveRepoUrl("https://github.com/TheLarkInn"),
  ];

  assert.deepEqual(
    refusals.map((result) => ({ ok: result.ok, hasReason: typeof result.reason === "string" && result.reason.length > 0 })),
    refusals.map(() => ({ ok: false, hasReason: true })),
  );
});

test("access and credential are never guessed from the URL", () => {
  const resolved = resolveRepoUrl("https://github.com/microsoft/rushstack");

  assert.deepEqual(
    { access: resolved.access, credential: resolved.credential },
    { access: undefined, credential: undefined },
  );
});
