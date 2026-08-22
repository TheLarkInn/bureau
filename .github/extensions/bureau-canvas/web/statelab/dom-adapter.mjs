// The driver adapter the lab uses: real events against the real page, inside
// an iframe.
//
// Nothing here reaches into React. Every verb is a DOM event a user would
// have produced, which is the whole point — if the lab could set component
// state directly, "reachable" would stop meaning anything. Controlled inputs
// go through the native value setter because that is the only way a synthetic
// value reaches React's onChange.

const MOUNT_TIMEOUT = 8000;
const POLL_MS = 25;

import { assertAdapter, PUBLISH_EVENT } from "./driver.mjs";

export function domAdapter(frame) {
  const win = () => frame.contentWindow;
  const doc = () => frame.contentDocument;
  // What the SSE barrier actually saw, so the lab can say "this render was not
  // proved settled" rather than presenting a possibly-raced screen as verified.
  const channel = { observed: null, reason: null };

  const find = async (selector, mustSee = false) => {
    const node = await waitFor(doc, selector, false, mustSee);
    if (!node) {
      throw new Error(`no ${mustSee ? "visible " : ""}element matched ${selector}`);
    }
    return node;
  };

  return assertAdapter({
    channel,
    async goto(page, op) {
      if (op?.intercept) {
        throw new Error(`this state needs request interception (${op.intercept}); the browser suite renders it`);
      }
      // The assignment stack remembers its expanded card in `sessionStorage`,
      // which is shared across same-origin frames. Without clearing it a
      // replayed path would toggle a card closed instead of opening it, and
      // "reachable" would depend on what the reviewer clicked ten minutes ago.
      freshSession();
      await load(frame, page === "editor" ? "./editor.html" : "./index.html");
      // Wait for the payload the page fetches itself; publishing a fixture
      // before it arrives would be overwritten the moment it did.
      await waitFor(doc, page === "editor" ? ".editor-tabs" : ".app-header");
      Object.assign(channel, await sseDelivered(win));
    },
    publish(state) {
      win().dispatchEvent(new (win().CustomEvent)(PUBLISH_EVENT, { detail: state }));
      return settle();
    },
    async click(selector) {
      (await find(selector)).click();
      return settle();
    },
    async fill(selector, value) {
      setNativeValue(win(), await find(selector), value, "input");
      return settle();
    },
    async select(selector, value) {
      setNativeValue(win(), await find(selector), value, "change");
      return settle();
    },
    async press(selector, key) {
      const node = await find(selector);
      node.dispatchEvent(new (win().KeyboardEvent)("keydown", { key, bubbles: true }));
      return settle();
    },
    async drag(selector, dx, dy) {
      await dragNode(win(), await find(selector), dx, dy);
      return settle();
    },
    wait: (selector) => find(selector, true),
    present: (selector) => find(selector),
    async waitGone(selector) {
      if (!await waitFor(doc, selector, true)) {
        throw new Error(`${selector} was still visible after ${MOUNT_TIMEOUT}ms`);
      }
    },
  });
}

function load(frame, url) {
  return new Promise((resolve) => {
    frame.addEventListener("load", () => resolve(), { once: true });
    frame.src = url;
    armSse(frame);
  });
}

/**
 * Both pages take state from two channels: a `fetch("./state")` on mount and
 * an `EventSource("./events")` that the host answers with the current state
 * the instant it connects. The surface appears as soon as *either* lands, so
 * waiting for the surface proves only that one of them arrived — and the other
 * can then overwrite a fixture published in between, leaving the lab claiming
 * to show one state while the host's own payload is on screen. It is a narrow
 * window, tens of milliseconds wide, and it is why lab checks failed at random.
 *
 * The SSE channel never completes, so resource timing cannot see it; the only
 * honest signal is the delivery itself. A same-origin frame's new document is
 * reachable from here while it is still parsing, which is before its deferred
 * module scripts run, so the observer is installed there. It only watches: it
 * neither swallows the event nor changes the order anything arrives in.
 */
const SSE_STATE = "__bureauLabSseState";
const CHANNEL_TIMEOUT = 3000;

function armSse(frame) {
  const deadline = Date.now() + CHANNEL_TIMEOUT;
  const spin = () => {
    const win = frame.contentWindow;
    if (frame.contentDocument?.readyState === "loading" && win?.EventSource && win[SSE_STATE] === undefined) {
      observe(win);
      return;
    }
    if (Date.now() < deadline) {
      setTimeout(spin, 0);
    }
  };
  spin();
}

function observe(win) {
  win[SSE_STATE] = false;
  const Native = win.EventSource;
  win.EventSource = class extends Native {
    constructor(...args) {
      super(...args);
      super.addEventListener("state", () => {
        win[SSE_STATE] = true;
      });
    }
  };
}

/**
 * Bounded on purpose: if a browser ever denies the frame early enough for the
 * observer to miss its window, the walk carries on and the verdict still
 * reports whatever actually rendered. A review tool that stalls tells a
 * reviewer less than one that shows the wrong thing and says what it checked.
 *
 * But it must not call that success. `undefined` means the observer never
 * armed, which is exactly the case where the race is still live — so it is
 * reported as unobserved rather than counted as settled, and the lab prints it
 * next to the state instead of letting a reviewer read a possibly-raced render
 * as a verified one.
 */
function sseDelivered(win) {
  const deadline = Date.now() + CHANNEL_TIMEOUT;
  return new Promise((resolve) => {
    const tick = () => {
      const flag = win()?.[SSE_STATE];
      if (flag === true) {
        resolve({ observed: true });
        return;
      }
      if (Date.now() > deadline) {
        resolve({ observed: false, reason: flag === undefined ? "the SSE observer never armed" : "no state event arrived" });
        return;
      }
      setTimeout(tick, POLL_MS);
    };
    tick();
  });
}

/** Same origin as the frame, so clearing here clears the surface's memory. */
function freshSession() {
  try {
    window.sessionStorage.clear();
  } catch {
    // A browser with storage disabled has nothing to clear.
  }
}

/** A macrotask: the queue React's commit is already scheduled on. */
function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/**
 * Polls for a selector. `mustSee` adds the visibility requirement `wait`
 * carries, so the lab holds a path to the same standard the browser suite
 * does rather than passing on a control that is attached but not drawn.
 *
 * "Drawn" is a box *and* a computed `visibility` that is not `hidden`, which
 * is Playwright's own standard — and the distinction is load-bearing rather
 * than pedantic, because an unmeasured React Flow node is laid out at
 * `visibility: hidden`. Counting boxes alone would let the blank graph
 * `graph-measure.mjs` exists to repair satisfy a wait.
 *
 * `gone` means *not visible*, not *detached* — Playwright's `hidden` state and
 * the verdict's own visibility test both mean that, and the editor's tabs hide
 * a subtree with the `hidden` attribute rather than unmounting it. A `gone`
 * that meant detached would wait out the full timeout on the one path that
 * uses it and then pass anyway, so it resolves false on timeout and the caller
 * throws.
 */
function waitFor(doc, selector, gone = false, mustSee = false) {
  const deadline = Date.now() + MOUNT_TIMEOUT;
  const painted = (node) =>
    node.getClientRects().length > 0 &&
    node.ownerDocument.defaultView.getComputedStyle(node).visibility !== "hidden";
  const drawn = (node) => node && (!mustSee || painted(node));
  const hidden = (node) => !node || !painted(node);
  return new Promise((resolve) => {
    const tick = () => {
      const node = doc()?.querySelector(selector);
      if (gone ? hidden(node) : drawn(node)) {
        resolve(gone ? true : node);
        return;
      }
      if (Date.now() > deadline) {
        resolve(gone ? false : null);
        return;
      }
      setTimeout(tick, POLL_MS);
    };
    tick();
  });
}

/**
 * React tracks the last value it wrote on the DOM node; assigning `.value`
 * directly leaves that tracker in step and the change event is swallowed.
 * Going through the prototype setter is the documented way around it.
 */
function setNativeValue(win, node, value, eventName) {
  const prototype = node instanceof win.HTMLSelectElement
    ? win.HTMLSelectElement.prototype
    : node instanceof win.HTMLTextAreaElement
      ? win.HTMLTextAreaElement.prototype
      : win.HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, "value").set.call(node, value);
  node.dispatchEvent(new win.Event(eventName, { bubbles: true }));
  if (eventName === "input") {
    node.dispatchEvent(new win.Event("change", { bubbles: true }));
  }
}

/** A pointer drag, because the flow library listens for pointer events. */
async function dragNode(win, node, dx, dy) {
  const rect = node.getBoundingClientRect();
  const from = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  const send = (type, target, point) => {
    target.dispatchEvent(new win.PointerEvent(type, {
      bubbles: true, cancelable: true, clientX: point.x, clientY: point.y, button: 0, buttons: 1, pointerId: 1, isPrimary: true,
    }));
  };
  send("pointerdown", node, from);
  for (const fraction of [0.34, 0.67, 1]) {
    send("pointermove", win.document, { x: from.x + dx * fraction, y: from.y + dy * fraction });
    await new Promise((resolve) => setTimeout(resolve, 16));
  }
  send("pointerup", win.document, { x: from.x + dx, y: from.y + dy });
}
