// What a deleted step leaves behind in the steps that named it.
//
// This lives in `web/` because `web/` is the only tree the browser can reach:
// the dashboard serves that directory and nothing above it, so a module under
// `lib/` is a 404 to the page. The host has no such limit — it reads files off
// disk — so the reachable side is where a rule both of them must obey can be
// written once. `lib/edit.mjs` imports it from here rather than the other way
// round, which is why the editor's draft transforms and the host's saved-view
// transforms cannot drift apart the way three separate `removeStep`s did.
//
// Pure, with no imports of its own: the offline suite holds the rule without a
// browser and without a host.

/**
 * The three routing terminals, repeated here rather than imported because this
 * module is the one both trees reach and it takes no imports of its own.
 *
 * They matter to this rule because `on:` is the only one of the four fields
 * whose values are not always step names. A bare `done`/`abort`/`escalate` in
 * an `on:` map is the *terminal* of that name everywhere it is read — the
 * engine matches those three arms before it looks at `pipeline.steps`
 * (`crates/bureau/src/engine/edge.rs`), and both canvas resolvers do the same —
 * so a step may be named `abort` without ever being the thing `on: abort`
 * points at. `over`, `members` and `inputs_from` never take a terminal, so a
 * name there is always the step.
 */
export const TERMINAL_NAMES = ["done", "abort", "escalate"];

/**
 * Whether a bare `on:` value naming `name` would resolve to the terminal
 * rather than to the step called `name`.
 *
 * When it does, an edit to the step called `name` must leave that route alone:
 * it was never a reference to the step. Getting this wrong is a silent write —
 * the routes vanish or move, the codec renders the result, and React Flow draws
 * nothing for what is no longer there, so the screen looks clean either way.
 */
function routesToTerminal(name) {
  return TERMINAL_NAMES.includes(name);
}

/**
 * A step with every reference to a departed step dropped, in the same four
 * fields a rename retargets.
 *
 * A step's references to another step do not all live in `view.edges`: a
 * decision routes through its `on:` map, a concurrent step lists `members`, a
 * decision observes `over`, and an agent step reads `inputs_from`. Dropping the
 * edges alone left those naming a step that no longer exists — invisibly,
 * because React Flow draws nothing at all for an edge whose endpoint is
 * missing, so the screen showed a clean pipeline while the document carried a
 * dangling reference that surfaced later out of a validation, with nothing to
 * connect it to the click that caused it. `renameStep` has always retargeted
 * all four; a delete has the same obligation.
 *
 * Dropping rather than blanking, except for `over`, which is written as an
 * explicit `null`. The codec reads `undefined` as "this edit says nothing about
 * that field" and `null` as "remove the key" — the convention a limit already
 * uses when it is turned off — so deleting `over` outright left `over:` in the
 * file still naming the departed step, which is the exact defect this is here
 * to prevent. `on`, `members` and `inputsFrom` need no such mark because a
 * filtered map or array is still a value, and the codec writes it.
 *
 * Every reader of `over` takes `null` and `undefined` the same way — `?? ""`,
 * `?? "not set"`, and a `present()` that demands a non-empty string — so the
 * screen is unchanged and only the write differs.
 *
 * For `on`, dropping is the point: an outcome with no route and an outcome
 * routed to nothing are different states in this editor — the first is a gap
 * the decision panel offers to fill, the second is a value — and the reader
 * deleted a step, not an outcome.
 *
 * `on` is skipped entirely when the departed step shares a terminal's name,
 * because then none of those routes named the step in the first place. Without
 * that guard, deleting a step called `abort` stripped every outcome in the
 * pipeline that failed closed to the `abort` terminal — and a scaffolded
 * decision routes three of its four outcomes there.
 */
export function withoutReferencesTo(step, name) {
  const fields = { ...step.fields };
  if (fields.over === name) {
    fields.over = null;
  }
  if (fields.on && typeof fields.on === "object" && !routesToTerminal(name)) {
    fields.on = Object.fromEntries(Object.entries(fields.on).filter(([, target]) => target !== name));
  }
  if (Array.isArray(fields.members)) {
    fields.members = fields.members.filter((member) => member !== name);
  }
  if (Array.isArray(fields.inputsFrom)) {
    fields.inputsFrom = fields.inputsFrom.filter((source) => source !== name);
  }
  return { ...step, fields };
}

/**
 * A step with every reference to `from` retargeted to `to`, in the same four
 * fields a delete drops.
 *
 * The counterpart of `withoutReferencesTo`, and here for the same reason: this
 * rule was written out once in `lib/edit.mjs` and again in
 * `web/editor/editor.mjs`, which is the drift this module exists to end. A
 * rename that misses a field dangles exactly as invisibly as a delete that
 * does.
 *
 * The `on` guard is the mirror of the delete's, and matters more: retargeting a
 * rename of a step called `abort` did not merely drop those routes, it pointed
 * them at the renamed step — turning every outcome that failed closed to the
 * `abort` terminal into one that carries on into live work.
 */
export function withReferencesRetargeted(step, from, to) {
  const fields = { ...step.fields };
  if (fields.over === from) {
    fields.over = to;
  }
  if (fields.on && typeof fields.on === "object" && !routesToTerminal(from)) {
    fields.on = Object.fromEntries(Object.entries(fields.on).map(([outcome, target]) => [outcome, target === from ? to : target]));
  }
  if (Array.isArray(fields.members)) {
    fields.members = fields.members.map((member) => (member === from ? to : member));
  }
  if (Array.isArray(fields.inputsFrom)) {
    fields.inputsFrom = fields.inputsFrom.map((source) => (source === from ? to : source));
  }
  return { ...step, fields };
}
