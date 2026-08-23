// The one interpreter for a state's entry path.
//
// A state is "reachable" only if this driver can walk its operations against a
// real page. The lab drives an iframe holding the production page; the browser
// suite drives a Playwright page. Both call `runPath` with an adapter, so the
// two hosts cannot drift: there is no second notion of how a state is reached.
//
// The adapter is deliberately tiny — nine verbs, all of which a plain DOM and
// a Playwright page can both honour.

import { applyFixture } from "./fixtures.mjs";

/**
 * Executes one entry path.
 *
 * @param ops      the state's `ops` array
 * @param adapter  { goto, publish, click, fill, select, press, drag, wait, waitGone }
 * @param base     the payload `GET /state` returned, which fixtures project
 */
export async function runPath(ops, adapter, base) {
  for (const op of ops) {
    await step(op, adapter, base);
  }
}

async function step(op, adapter, base) {
  const handler = HANDLERS[op.op];
  if (!handler) {
    throw new Error(`unknown driver operation: ${op.op}`);
  }
  await handler(op, adapter, base);
}

const HANDLERS = {
  page: (op, adapter) => adapter.goto(op.value, op),
  fixture: (op, adapter, base) => adapter.publish(applyFixture(op.value, base)),
  click: (op, adapter) => adapter.click(op.selector),
  fill: (op, adapter) => adapter.fill(op.selector, op.value),
  select: (op, adapter) => adapter.select(op.selector, op.value),
  press: (op, adapter) => adapter.press(op.selector, op.value),
  drag: (op, adapter) => adapter.drag(op.selector, op.dx, op.dy),
  wait: (op, adapter) => adapter.wait(op.selector),
  present: (op, adapter) => adapter.present(op.selector),
  waitGone: (op, adapter) => adapter.waitGone(op.selector),
};

/**
 * The verbs an adapter must implement, so a partial one fails loudly.
 *
 * `wait` means "visible"; `present` means "in the DOM". The distinction is
 * load-bearing: an `<option>` is never visible, and waiting for one is how a
 * path stops racing an in-flight fetch instead of relying on a particular
 * host's auto-retry. Both adapters must honour it, or the lab would pass
 * states the browser suite fails.
 */
export const ADAPTER_VERBS = ["goto", "publish", "click", "fill", "select", "press", "drag", "wait", "present", "waitGone"];

/**
 * The verbs that wait for the page rather than advancing the path.
 *
 * The DAG is built by comparing paths one *action* at a time, so it needs to
 * know which operations are not actions — and this is the module that owns the
 * vocabulary, so it is the one that can answer without guessing. It was guessed
 * for a while, as `op.op !== "wait"`, which silently promoted `present` and
 * `waitGone` to actions: a state whose path ends on "wait for this to go away"
 * looked for a parent one operation short of the real one, found none, and
 * joined the graph with no incoming edge at all.
 */
export const WAIT_VERBS = ["wait", "present", "waitGone"];

/** Whether an operation advances the path, as opposed to waiting for it. */
export function isAction(op) {
  return !WAIT_VERBS.includes(op.op);
}

export function assertAdapter(adapter) {
  const missing = ADAPTER_VERBS.filter((verb) => typeof adapter?.[verb] !== "function");
  if (missing.length) {
    throw new Error(`driver adapter is missing: ${missing.join(", ")}`);
  }
  return adapter;
}

/**
 * The event both hosts publish a fixture payload on. `App` in `web/app.mjs`
 * and `EditorApp` in `web/editor/index.mjs` both listen for it, so the name
 * lives here rather than as a string literal in each publisher.
 */
export const PUBLISH_EVENT = "bureau-state";
