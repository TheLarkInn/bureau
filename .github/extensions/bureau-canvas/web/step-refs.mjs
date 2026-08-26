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
 * Dropping rather than blanking: an outcome with no route and an outcome routed
 * to nothing are different states in this editor — the first is a gap the
 * decision panel offers to fill, the second is a value — and the reader deleted
 * a step, not an outcome.
 */
export function withoutReferencesTo(step, name) {
  const fields = { ...step.fields };
  if (fields.over === name) {
    delete fields.over;
  }
  if (fields.on && typeof fields.on === "object") {
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
