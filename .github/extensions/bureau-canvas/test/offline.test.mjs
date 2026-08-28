// Whether "offline" is a check or a claim.
//
// Every suite header in `e2e/playwright/` says the run reaches no network, and
// until this file existed nothing in the repository could contradict one. The
// claim rested on arrangement — the binary lookup points at a path that does not
// exist, the runs root is committed, no fixture asks for the network — and a
// `fetch("https://example.com/")` added to `web/app.mjs`, swallowing its own
// rejection, left all 147 browser tests green.
//
// The predicate is tested here rather than in a browser because that is where it
// can be asked the questions a real run never poses: an unreadable URL, a
// scheme that is neither http nor a page-local one, an IPv6 loopback. A browser
// suite can only demonstrate that the floor holds for the requests that suite
// happens to make.

import assert from "node:assert/strict";
import test from "node:test";

import { LOOPBACK, offlineFindings, offsite } from "../e2e/playwright/offline.mjs";

/**
 * The whole judgement, in one table.
 *
 * The rows that matter are the last three. A URL the parser cannot read must be
 * *refused*, not waved through: returning "local" for the one input nobody
 * anticipated would make it the one input that passes, which is the shape of
 * hole this file exists to close. A scheme that is neither a network scheme nor
 * page-local is refused for the same reason.
 *
 * The `ws:` rows are the pair, and they are a pair on purpose: a socket to a
 * remote host is traffic that left the machine and must be named as such, and a
 * socket to loopback is this machine by exactly the rule every other row uses.
 * Judging sockets by scheme alone would have got the second one wrong.
 */
test("a request is judged local only when it is provably this machine", () => {
  const cases = [
    ["http://127.0.0.1:4173/state", null],
    ["http://localhost:8080/app.mjs", null],
    ["http://[::1]:9000/events", null],
    ["ws://127.0.0.1:4173/reload", null],
    ["data:text/css,body{}", null],
    ["about:blank", null],
    ["blob:http://127.0.0.1:4173/abcd", null],
    ["https://example.com/telemetry", "https://example.com"],
    ["http://169.254.169.254/latest/meta-data", "http://169.254.169.254"],
    ["https://api.githubcopilot.com/chat/completions", "https://api.githubcopilot.com"],
    ["http://127.0.0.1.evil.test/state", "http://127.0.0.1.evil.test"],
    ["ws://192.0.2.1:9/telemetry", "ws://192.0.2.1:9"],
    ["wss://relay.example.com/socket", "wss://relay.example.com"],
    ["ftp://files.example.com/x", "a ftp request (ftp://files.example.com/x)"],
    ["not a url at all", "an unreadable URL (not a url at all)"],
  ];
  assert.deepStrictEqual(cases.map(([url]) => offsite(url)), cases.map(([, verdict]) => verdict));
});

/**
 * A port is not part of the question, and a host that merely ends in a loopback
 * address is not one. Both are spelled out because both are the kind of thing a
 * later simplification — a `startsWith`, a substring test — would quietly get
 * wrong while every row above still passed.
 *
 * `evil-127.0.0.1` is the third row and is refused by a route worth naming: its
 * final label is numeric, so the URL parser tries to read the whole host as an
 * IPv4 address, fails, and rejects it. It reaches the floor as an *unreadable*
 * URL rather than as a remote host — which is the fail-closed branch doing the
 * work, and the reason that branch refuses rather than waves through.
 */
test("the loopback set is addresses, so neither a port nor a suffix can imitate one", () => {
  assert.deepStrictEqual(
    [
      [...LOOPBACK].every((host) => offsite(`http://${host}:1234/x`) === null),
      offsite("http://evil-127-0-0-1.test/x"),
      offsite("http://localhost.attacker.test/x"),
      offsite("http://evil-127.0.0.1/x")?.startsWith("an unreadable URL"),
    ],
    [true, "http://evil-127-0-0-1.test", "http://localhost.attacker.test", true],
  );
});

/**
 * One sentence per destination, not per request.
 *
 * A page that retries a lost fetch eight times has one defect, and eight
 * identical lines bury it. The count is asserted alongside the wording so that
 * de-duplication cannot be dropped without the test noticing.
 */
test("destinations are reported once each, whatever the traffic was", () => {
  const findings = offlineFindings([
    "https://example.com",
    "https://example.com",
    "https://example.com",
    "http://other.test",
  ]);
  assert.deepStrictEqual(
    [findings.length, findings[0].kind, findings[0].detail, findings[1].detail.includes("example.com")],
    [
      2,
      "left-the-machine",
      "a request was made to http://other.test, so this run is not offline",
      true,
    ],
  );
});

/** Nothing recorded is nothing to report — the state every run should be in. */
test("a run that stayed on this machine reports nothing", () => {
  assert.deepStrictEqual(offlineFindings([]), []);
});
