// Enumeration over the dimension product with exact exclusion accounting.
//
// The full product is 10^8-scale, so it is never materialised. Instead the
// walk assigns dimensions in order and, as soon as every input a rule reads is
// assigned, applies it. A rule that fails prunes the whole subtree below that
// partial assignment, and that subtree's size is added to the rule's tally —
// so every excluded tuple is attributed to exactly one rule and the totals
// balance without ever holding the product in memory.
//
// What that tally is, precisely: the number of tuples this rule was the FIRST
// to reject, in `ORDER`. It is not "how many tuples this rule forbids". A
// tuple broken by three rules is charged to whichever of them completes its
// inputs earliest, so `pruned` is a property of the rule *and the walk order*,
// not of the rule alone — which is why it is named `pruned` rather than
// `removed`, and why the lab says "pruned here" when it prints it.
//
// The order-independent claim, which is the one that matters, is that the
// surviving set does not depend on `ORDER` at all. `statelab.test.mjs` pins
// that by re-enumerating under permuted orders and comparing kept sets.
//
// Pure: same dimensions and rules in, same counts out, every time.

import { CONSTRAINTS, rulesReadyFor } from "./constraints.mjs";

/** Rules keyed by the depth at which their last input becomes assigned. */
function rulesByDepth(order) {
  const assigned = new Set();
  const seen = new Set();
  return order.map((dimension) => {
    assigned.add(dimension);
    const ready = rulesReadyFor(assigned).filter((rule) => !seen.has(rule.id));
    for (const rule of ready) {
      seen.add(rule.id);
    }
    return ready;
  });
}

/** How many tuples hang below a node at `depth`, i.e. the fan-out remaining. */
function tailSizes(order, valuesFor) {
  const sizes = new Array(order.length + 1).fill(1);
  for (let index = order.length - 1; index >= 0; index -= 1) {
    sizes[index] = sizes[index + 1] * valuesFor(order[index]).length;
  }
  return sizes;
}

/**
 * Walks the product, pruning on the first rule that rejects a partial
 * assignment. Returns the surviving tuples plus, per rule, how many tuples it
 * was the first to prune and one worked example of such a prune.
 */
export function enumerate(order, valuesFor) {
  const gates = rulesByDepth(order);
  const tails = tailSizes(order, valuesFor);
  const removed = new Map(CONSTRAINTS.map((rule) => [rule.id, { rule: rule.id, pruned: 0, example: null }]));
  const kept = [];
  let total = 0;

  const walk = (depth, combo) => {
    if (depth === order.length) {
      kept.push({ ...combo });
      return;
    }
    for (const value of valuesFor(order[depth])) {
      const next = { ...combo, [order[depth]]: value.id };
      const broken = gates[depth].find((rule) => !rule.holds(next));
      if (broken) {
        const entry = removed.get(broken.id);
        entry.pruned += tails[depth + 1];
        entry.example ??= { assigned: { ...next }, depth: depth + 1, of: order.length };
        continue;
      }
      walk(depth + 1, next);
    }
  };

  walk(0, {});
  total = tails[0];
  return { kept, excluded: [...removed.values()], combinations: total };
}
