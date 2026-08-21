// lib/worksource.mjs: deriving `work: { forge, source, filter }` from a
// pasted forge URL. Offline and pure — no network, no DOM.

import assert from "node:assert/strict";
import test from "node:test";

import { deriveWorkSource } from "../lib/worksource.mjs";

const ADO_BOARD = "https://onedrive.visualstudio.com/EFun/_boards/board/t/Web/Backlog%20items?System.AssignedTo=%40me%2Cselarkin%40microsoft.com";
const GITHUB_ISSUES = "https://github.com/TheLarkInn/bureau/issues?q=is%3Aopen+label%3Aagent-eligible";

test("a GitHub issues URL carries its search query through verbatim", () => {
  const derived = deriveWorkSource(GITHUB_ISSUES);

  assert.deepEqual(
    { forge: derived.forge, source: derived.source, filter: derived.filter, exact: derived.exact },
    { forge: "github", source: "TheLarkInn/bureau", filter: "is:open label:agent-eligible", exact: true },
  );
});

test("an Azure DevOps board URL yields project/team and WIQL from its filters", () => {
  const derived = deriveWorkSource(ADO_BOARD);

  assert.deepEqual(
    { forge: derived.forge, source: derived.source, filter: derived.filter },
    {
      forge: "ado",
      source: "EFun/Web",
      filter: "([System.AssignedTo] = @Me OR [System.AssignedTo] = 'selarkin@microsoft.com')",
    },
  );
});

test("a board URL says plainly that column rules are not in the link", () => {
  const derived = deriveWorkSource(ADO_BOARD);

  assert.deepEqual(
    { exact: derived.exact, mentionsColumns: derived.notes.some((note) => note.includes("column")) },
    { exact: false, mentionsColumns: true },
  );
});

test("dev.azure.com puts the organization in the path, not the host", () => {
  const derived = deriveWorkSource("https://dev.azure.com/onedrive/EFun/_boards/board/t/Web/Stories");

  assert.equal(derived.source, "EFun/Web");
});

test("a project-only Azure DevOps URL keeps the project as the source", () => {
  const derived = deriveWorkSource("https://onedrive.visualstudio.com/EFun/_workitems/edit/2841193");

  assert.deepEqual({ source: derived.source, forge: derived.forge }, { source: "EFun", forge: "ado" });
});

test("field operators follow the field rather than defaulting to equality", () => {
  const cases = [
    ["System.AreaPath=ODSP%5CWeb", "[System.AreaPath] UNDER 'ODSP\\Web'"],
    ["System.Tags=agent-eligible", "[System.Tags] CONTAINS 'agent-eligible'"],
    ["System.State=Active,New", "[System.State] IN ('Active', 'New')"],
    ["System.AssignedTo=%40me", "[System.AssignedTo] = @Me"],
  ];
  const derived = cases.map(([query]) => deriveWorkSource(`https://dev.azure.com/org/EFun/_boards/board?${query}`).filter);

  assert.deepEqual(derived, cases.map(([, expected]) => expected));
});

test("view-state parameters are not mistaken for work item fields", () => {
  const derived = deriveWorkSource("https://dev.azure.com/org/EFun/_boards/board/t/Web/Stories?_a=board&type=story");

  assert.deepEqual(
    { filter: derived.filter, warned: derived.notes.some((note) => note.includes("no field filters")) },
    { filter: "[System.State] = 'Active'", warned: true },
  );
});

test("a saved query URL admits that its clauses are not in the link", () => {
  const derived = deriveWorkSource("https://dev.azure.com/org/EFun/_queries/query/6f1a2b3c-0000-4444-8888-abcdefabcdef");

  assert.equal(derived.notes.some((note) => note.includes("saved query")), true);
});

test("a GitHub label page becomes a label filter, flagged as inferred", () => {
  const derived = deriveWorkSource("https://github.com/TheLarkInn/bureau/labels/agent-eligible");

  assert.deepEqual(
    { filter: derived.filter, exact: derived.exact },
    { filter: "is:open label:agent-eligible", exact: false },
  );
});

test("a bare GitHub repository URL warns that it matches everything open", () => {
  const derived = deriveWorkSource("https://github.com/TheLarkInn/bureau");

  assert.deepEqual(
    { filter: derived.filter, warned: derived.notes.some((note) => note.includes("every open item")) },
    { filter: "is:open", warned: true },
  );
});

test("input this cannot derive is refused with a reason instead of a guess", () => {
  const refusals = [
    deriveWorkSource(""),
    deriveWorkSource("not a url"),
    deriveWorkSource("https://gitlab.com/owner/repo/-/issues"),
    deriveWorkSource("https://github.com/orgs/github/projects/3"),
    deriveWorkSource("https://github.com/TheLarkInn"),
  ];

  assert.deepEqual(
    refusals.map((result) => ({ ok: result.ok, hasReason: typeof result.reason === "string" && result.reason.length > 0 })),
    refusals.map(() => ({ ok: false, hasReason: true })),
  );
});

test("a quote in a filter value cannot break out of the WIQL string", () => {
  const derived = deriveWorkSource("https://dev.azure.com/org/EFun/_boards/board?System.State=it%27s%20odd");

  assert.equal(derived.filter, "[System.State] = 'it''s odd'");
});

test("a label containing a space stays one GitHub search term", () => {
  const derived = deriveWorkSource("https://github.com/TheLarkInn/bureau/labels/needs%20triage");

  assert.equal(derived.filter, 'is:open label:"needs triage"');
});
