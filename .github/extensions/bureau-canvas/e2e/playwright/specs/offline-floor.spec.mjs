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

import { expect, floorTest as test } from "../fixtures.mjs";
import { holdOffline, offlineFindings } from "../offline.mjs";

/**
 * TEST-NET-1 on the discard port: reserved by RFC 5737 for documentation and
 * routed nowhere, so a run that somehow escapes this floor still reaches no
 * host that exists.
 */
const OFFSITE = "ws://192.0.2.1:9/offline-floor-probe";

/** Opens one socket and reports how it ended, without waiting on a timeout. */
function openSocket(page, url) {
  return page.evaluate((address) => new Promise((resolve) => {
    const socket = new WebSocket(address);
    socket.addEventListener("error", () => resolve({ errored: true, clean: false }));
    socket.addEventListener("close", (event) => resolve({ errored: false, clean: event.wasClean }));
  }), url);
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
