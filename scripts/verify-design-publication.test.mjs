import assert from "node:assert/strict";
import test from "node:test";

import { publicationProblem } from "./verify-design-publication.mjs";

const inputs = {
  created_issue_url: "https://github.com/TheLarkInn/bureau/issues/42",
  source_commit: "abc123",
  fingerprint: "TheLarkInn/bureau#7:abc123",
};
const request = {
  inputs,
  item: { external_id: "TheLarkInn/bureau#7" },
};
const draft = {
  number: 42,
  state: "open",
  user: { login: "TheLarkInn" },
  labels: [],
  body: "<!-- bureau-design-audit:TheLarkInn/bureau#7:abc123 -->",
};
const active = { ...draft, labels: [{ name: "agent-eligible" }] };
const sourceTriggered = {
  number: 7,
  state: "open",
  user: { login: "TheLarkInn" },
  labels: [{ name: "bureau:design-scan" }],
  body: "Run the design scan.",
};
const sourceCleared = { ...sourceTriggered, labels: [] };
const workspace = { commit: "abc123", status: "" };

test("accepts one observed matching draft", () => {
  assert.equal(
    publicationProblem(request, inputs, [draft, sourceTriggered], workspace),
    null,
  );
});

test("accepts a completed verified handoff", () => {
  assert.equal(
    publicationProblem(request, inputs, [active, sourceCleared], workspace, true),
    null,
  );
});

test("rejects duplicate marker issues", () => {
  const duplicate = { ...draft, number: 43 };

  assert.equal(
    publicationProblem(request, inputs, [draft, duplicate, sourceTriggered], workspace),
    "expected one audit-marker issue, observed 2",
  );
});

test("rejects a prematurely active draft", () => {
  assert.equal(
    publicationProblem(request, inputs, [active, sourceTriggered], workspace),
    "drafted issue is already agent-eligible",
  );
});

test("rejects an untrusted draft author", () => {
  const untrusted = { ...draft, user: { login: "outside-contributor" } };

  assert.equal(
    publicationProblem(request, inputs, [untrusted, sourceTriggered], workspace),
    "drafted issue author is not trusted",
  );
});

test("rejects a closed source after handoff", () => {
  const closed = { ...sourceCleared, state: "closed" };
  assert.equal(
    publicationProblem(request, inputs, [active, closed], workspace, true),
    "source issue was closed",
  );
});
