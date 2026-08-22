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

  const find = async (selector, mustSee = false) => {
    const node = await waitFor(doc, selector, false, mustSee);
    if (!node) {
      throw new Error(`no ${mustSee ? "visible " : ""}element matched ${selector}`);
    }
    return node;
  };

  return assertAdapter({
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
      await waitFor(doc, selector, true);
    },
  });
}

function load(frame, url) {
  return new Promise((resolve) => {
    frame.addEventListener("load", () => resolve(), { once: true });
    frame.src = url;
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

/** One animation frame plus a macrotask: enough for React to commit. */
function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/**
 * Polls for a selector. `mustSee` adds the visibility requirement `wait`
 * carries, so the lab holds a path to the same standard the browser suite
 * does rather than passing on a control that is attached but not drawn.
 */
function waitFor(doc, selector, gone = false, mustSee = false) {
  const deadline = Date.now() + MOUNT_TIMEOUT;
  const drawn = (node) => node && (!mustSee || node.getClientRects().length > 0);
  return new Promise((resolve) => {
    const tick = () => {
      const node = doc()?.querySelector(selector);
      if (gone ? !node : drawn(node)) {
        resolve(node ?? null);
        return;
      }
      if (Date.now() > deadline) {
        resolve(null);
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
