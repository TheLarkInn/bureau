// Enumeration over the dimension product with exact exclusion accounting.
//
// The full product is 10^8-scale, so it is never materialised. Instead the
// walk assigns dimensions in order and, as soon as every input a rule reads is
// assigned, applies it. A rule that fails prunes the whole subtree below that
// partial assignment — and the size of the pruned subtree is exactly the
// number of tuples that rule removed, so the accounting stays exact without
// ever holding the product in memory.
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
 * assignment. Returns the surviving tuples plus per-rule exclusion counts and
 * one worked example per rule.
 */
export function enumerate(order, valuesFor) {
  const gates = rulesByDepth(order);
  const tails = tailSizes(order, valuesFor);
  const removed = new Map(CONSTRAINTS.map((rule) => [rule.id, { rule: rule.id, count: 0, example: null }]));
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
        entry.count += tails[depth + 1];
        entry.example ??= { ...next };
        continue;
      }
      walk(depth + 1, next);
    }
  };

  walk(0, {});
  total = tails[0];
  return { kept, excluded: [...removed.values()], combinations: total };
}
