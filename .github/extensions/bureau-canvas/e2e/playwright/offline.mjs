// The floor that makes "offline" a check rather than a claim.
//
// Every suite header in this directory says the run is offline, and until now
// nothing in the run could contradict them. The claim rested on arrangement:
// `BUREAU_CANVAS_TEST=1` points the binary lookup at a path that does not
// exist, the runs root is a committed directory, and no fixture asks for the
// network. All true, and none of it answerable — a `fetch` added to
// `web/app.mjs`, reaching a public host and swallowing its own rejection, left
// the whole browser suite green. A page is entitled to make requests, and
// nothing was reading where they went.
//
// So the run reads them. Two mechanisms, because they fail in different
// directions:
//
//   `page.on("request")` sees every request the page makes, including ones a
//   spec's own `route.continue()` sends straight to the network without
//   consulting the handlers underneath it. It cannot stop anything, and it
//   cannot be got round either — which is what makes it the *detector*.
//
//   `page.route("**/*")` is the *refusal*. Registered before any spec's routes,
//   so Playwright — which runs handlers newest-first — consults it last, it is
//   the floor rather than a lid: a state that models a failing request keeps its
//   own handler, and anything no state claimed falls through to here and is
//   aborted before it leaves the machine.
//
// Recording and refusing are kept apart on purpose. Aborting alone would
// surface as some unrelated control failing its checks, which is a bug report
// nobody can read; the recorded host is what turns it into a sentence naming
// where the run tried to go.
//
// Both mechanisms are HTTP-shaped, and a WebSocket is not an HTTP request: it
// raises no `request` event and is not routed by `page.route`. So the same two
// jobs are done again over `page.on("websocket")` and `page.routeWebSocket`,
// because a socket opened to a remote host leaves this machine exactly as a
// fetch does — and until they were added, one did so in silence.

/**
 * Addresses that are this machine. A port is not part of the question.
 *
 * The IPv6 loopback appears only in its bracketed form because that is the only
 * form a `URL` ever reports: `new URL("http://[::1]:9000/").hostname` is
 * `[::1]`, and a bare `::1` cannot be parsed as a host at all. Carrying the bare
 * spelling here would be a member of the set that nothing can ever match — a
 * line that looks like coverage and is not.
 */
export const LOOPBACK = new Set(["127.0.0.1", "localhost", "[::1]"]);

/** Schemes that never leave the page, so there is nothing to refuse. */
const IN_PAGE = new Set(["data:", "blob:", "about:", "chrome-error:"]);

/**
 * Schemes that address a host, so the loopback set is the question asked of
 * them rather than the scheme itself.
 *
 * `ws:` and `wss:` are here because a WebSocket leaves this machine exactly as
 * an `https:` fetch does, and because the alternative spelling is wrong in the
 * direction that matters: refusing every non-HTTP scheme outright would have
 * reported a `ws://localhost` — this machine, by the same rule every other line
 * here uses — as traffic that left it.
 */
const NETWORK = new Set(["http:", "https:", "ws:", "wss:"]);

/**
 * Where this URL goes if it is not the loopback, and `null` when it is.
 *
 * Fails closed, and the unparseable branch is the reason to say so. Returning
 * "local" for a URL this cannot read would make the one input nobody
 * anticipated the one input that passes, which is the shape of hole this
 * function exists to close.
 */
export function offsite(url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return `an unreadable URL (${url})`;
  }
  if (IN_PAGE.has(parsed.protocol)) {
    return null;
  }
  if (!NETWORK.has(parsed.protocol)) {
    return `a ${parsed.protocol.replace(":", "")} request (${url})`;
  }
  return LOOPBACK.has(parsed.hostname) ? null : `${parsed.protocol}//${parsed.host}`;
}

/**
 * Refuses every request that leaves this machine, and records where it meant to
 * go. Install before a spec adds routes of its own, so this stays the floor.
 *
 * The returned array is live: a caller reads it after the page has finished,
 * and an empty one is the proof the run was offline rather than the assumption
 * that it was.
 */
export async function holdOffline(page) {
  const reached = [];
  const record = (url) => {
    const away = offsite(url);
    if (away) {
      reached.push(away);
    }
    return away;
  };
  page.on("request", (request) => record(request.url()));
  // A WebSocket is neither, and both mechanisms above were blind to one. It is
  // not a `request`, so the detector never saw it; it is not routed by
  // `page.route`, so the floor never refused it. A page opening
  // `ws://192.0.2.1:9/` left the whole suite green with nothing recorded —
  // the same hole this file was written to close, through the one door the
  // HTTP APIs cannot see.
  //
  // Both WebSocket paths record, deliberately. Whether `websocket` still fires
  // for a connection `routeWebSocket` has taken over is Playwright's business
  // and not a thing this floor should depend on; `offlineFindings` reports one
  // sentence per destination, so hearing it twice costs nothing and hearing it
  // from only one of them is still hearing it.
  page.on("websocket", (socket) => record(socket.url()));
  await page.routeWebSocket("**/*", (socket) => {
    if (record(socket.url())) {
      socket.close();
      return;
    }
    socket.connectToServer();
  });
  await page.route("**/*", (route) => {
    if (offsite(route.request().url())) {
      route.abort("blockedbyclient");
      return;
    }
    route.fallback();
  });
  return reached;
}

/**
 * What a recorded list of destinations says, as one sentence per distinct host.
 *
 * Distinct rather than per request: a page that retries a lost fetch eight times
 * has one defect, and eight identical lines bury it.
 */
export function offlineFindings(reached) {
  return [...new Set(reached)].sort().map((where) => ({
    kind: "left-the-machine",
    detail: `a request was made to ${where}, so this run is not offline`,
  }));
}
