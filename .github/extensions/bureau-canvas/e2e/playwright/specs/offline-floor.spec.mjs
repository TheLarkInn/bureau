// Whether the floor's socket half is installed, or only written.
//
// `test/offline.test.mjs` pins the judgement: `offsite()` knows that a `ws://`
// to a remote host has left this machine and that a `ws://127.0.0.1` has not.
// What nothing asked is whether `holdOffline` ever *wires* that judgement to a
// socket. Deleting both `page.on("websocket")` and `page.routeWebSocket` from
// it left the offline suite at 438, the PR suite at 147 and the whole matrix at
// 653 — all green, because no state in the registry opens one. The half of the
// floor that was added to close a hole nothing could see was itself a thing
// nothing could see.
//
// A unit test cannot close that: the judgement was never the part in doubt. The
// missing claim is about the wiring, and only a real page opening a real socket
// makes it. So one does, and the floor is asked for both of the jobs it claims:
//
//   the detector — the destination is recorded, so a run that reaches one says
//   where it went rather than failing as some unrelated control;
//
//   the refusal — the connection is answered here rather than made.
//
// The second is told from an ordinary failure by the *shape* of the ending. A
// route that takes the socket over answers it locally and closes it cleanly,
// before it is ever connected to a server. A socket nothing intercepted goes to
// the network: TEST-NET-1 is unroutable, so it ends in an `error` and an
// unclean close. Asserting "closed cleanly, and never errored" is therefore a
// claim only an installed floor can satisfy — a machine that happens to have no
// route to the address cannot fake it, because failing to reach a host is
// exactly the unclean ending this distinguishes itself from.
//
// Three tests, because the floor makes three separate claims about sockets: that
// the pair records and refuses, that the detector alone still names a socket the
// route did not answer, and — the one it cannot make — that a later route
// replaces it entirely.

import { expect, floorTest as test } from "../fixtures.mjs";
import { holdOffline, offlineFindings } from "../offline.mjs";

/**
 * TEST-NET-1 on the discard port: reserved by RFC 5737 for documentation and
 * routed nowhere, so a run that somehow escapes this floor still reaches no
 * host that exists.
 */
const OFFSITE = "ws://192.0.2.1:9/offline-floor-probe";

/**
 * Opens one socket and reports how it ended, without waiting on a timeout.
 *
 * The bound is not a tolerance: nothing here is allowed to be slow. Both
 * endings this distinguishes are immediate — a route answers in-process, and an
 * unroutable address is refused by the local stack — so the only way to reach
 * the bound is a network that *silently drops* egress instead of rejecting it,
 * on which neither listener ever fires. Unbounded, that read as a 30s Playwright
 * timeout naming no address and no claim; bounded, it resolves to a shape that
 * matches neither expectation and fails saying which socket never settled.
 */
function openSocket(page, url, budgetMs = 10_000) {
  return page.evaluate(([address, budget]) => new Promise((resolve) => {
    const socket = new WebSocket(address);
    const settle = (value) => {
      clearTimeout(timer);
      resolve(value);
    };
    const timer = setTimeout(() => settle({ errored: false, clean: false, settled: false }), budget);
    socket.addEventListener("error", () => settle({ errored: true, clean: false }));
    socket.addEventListener("close", (event) => settle({ errored: false, clean: event.wasClean }));
  }), [url, budgetMs]);
}

test("a socket that would leave this machine is recorded and refused here", async ({ page, canvas }) => {
  const reached = await holdOffline(page);
  await page.goto(canvas.url);

  const ended = await openSocket(page, OFFSITE);

  // One finding, and exactly one: the page load itself is loopback, so a floor
  // that recorded everything rather than everything *offsite* would fail here
  // too. The socket is named by host, which is the sentence a reviewer reads.
  expect(offlineFindings(reached).map((finding) => finding.detail)).toEqual([
    "a request was made to ws://192.0.2.1:9, so this run is not offline",
  ]);
  expect(ended).toEqual({ errored: false, clean: true });
});

// The detector on its own, which the test above cannot speak for.
//
// `holdOffline` claims two socket mechanisms and the test above proves only
// their sum: deleting `page.on("websocket")` left it green, because the route's
// own `record` produced the same sentence. Measuring the two apart settles what
// each is for. With a matching route installed the event never fires — the route
// answers the socket and no event is raised — so through the default glob the
// listener is unreachable and unaskable.
//
// It is reachable through the one case that matters: a socket no route matched.
// That is the failure the route cannot cover, because it *is* the route failing,
// and it is why the glob is a parameter. Narrowed to a pattern this socket does
// not match, the refusal never runs, the connection goes to the network — an
// `error` and an unclean close, TEST-NET-1 being routed nowhere — and the only
// thing left that can name the destination is the listener. That the ending is
// the *unclean* one is the proof the route really stood aside, so a listener
// credited here cannot be a route quietly doing the work.
test("a socket no route answered is still named by the floor", async ({ page, canvas }) => {
  const reached = await holdOffline(page, "**/answered-by-nothing");
  await page.goto(canvas.url);

  const ended = await openSocket(page, OFFSITE);

  expect([offlineFindings(reached).map((finding) => finding.detail), ended]).toEqual([
    ["a request was made to ws://192.0.2.1:9, so this run is not offline"],
    { errored: true, clean: false },
  ]);
});

// The gap, written down so it cannot widen quietly.
//
// Socket routes are consulted newest-first and there is no `fallback()` to reach
// the handler underneath, so a spec that registers its own replaces the floor's
// for sockets — and the event stays silent, because a socket some route answered
// raises none. Neither half reports it.
//
// Asserting the gap rather than a defence against it is the point: the floor
// cannot close this one, and an assumption nobody re-reads is worse than a test
// that fails the day it stops being true. If a future Playwright reverses that
// precedence or grows a fallback, this test goes red and the floor gets its
// cover back. `seen` and the clean close are asserted alongside so a run where
// the spec's own route simply never fired cannot read as supersession.
test("a socket a later route takes over is the floor's own recorded limit", async ({ page, canvas }) => {
  const reached = await holdOffline(page);
  const seen = [];
  await page.routeWebSocket("**/*", (socket) => {
    seen.push(socket.url());
    socket.close();
  });
  await page.goto(canvas.url);

  const ended = await openSocket(page, OFFSITE);

  expect([offlineFindings(reached), seen, ended]).toEqual([[], [OFFSITE], { errored: false, clean: true }]);
});
