import assert from "node:assert/strict";
import test from "node:test";
import { markdownAnchors, verifySourceLinks } from "../site/verify-links.mjs";

test("source anchors follow the headings used by the Bureau guides", () => {
  assert.deepEqual([...markdownAnchors([
    "# Getting started", "## First-time setup: `bureau init`",
    "### Opt-in local Copilot factories", "## Repeat", "## Repeat",
    "```sh", "# Not a heading", "```", '<a id="explicit"></a>',
  ].join("\n"))], ["getting-started", "first-time-setup-bureau-init", "opt-in-local-copilot-factories", "repeat", "repeat-1", "explicit"]);
});

test("source link validation checks real document bytes instead of merely allowing github URLs", async () => {
  const html = '<a href="https://github.com/TheLarkInn/bureau/blob/main/docs/scenarios.md#issue-triage">Triage</a>';
  assert.equal(await verifySourceLinks(html, async (path) => {
    assert.equal(path, "docs/scenarios.md");
    return "## Issue triage";
  }), 1);
  await assert.rejects(verifySourceLinks(html, async () => "## Different heading"), /anchor does not exist/u);
  await assert.rejects(verifySourceLinks(html, async () => { throw new Error("Missing doc."); }), /Missing doc/u);
});

test("unapproved source paths are rejected before any file read", async () => {
  const html = '<a href="https://github.com/TheLarkInn/bureau/blob/main/.env">Not a public document</a>';
  await assert.rejects(verifySourceLinks(html, async () => assert.fail("must not read")), /outside the documented file contract/u);
});
