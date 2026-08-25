// One repair, for every React Flow surface here.
//
// React Flow measures a node once, when its ResizeObserver delivery arrives,
// and `updateNodeInternals` returns without applying anything if the viewport
// element is not queryable at that moment. A node's box never changes again, so
// the observer does not fire a second time: a delivery that loses that race
// leaves the graph blank for good — nodes in the DOM at `visibility: hidden`,
// an empty minimap, and a surface that reads as "this pipeline has no steps".
//
// Rendered as a child of `ReactFlow`, this sits inside the store context, where
// the viewport is known to exist, so re-driving measurement there always lands.
// It draws nothing.

import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useNodesInitialized, useStore, useUpdateNodeInternals } from "@xyflow/react";

// Enough attempts to cover a lost delivery, and few enough to stop rather than
// spin if a node has genuinely left the DOM. `useNodesInitialized` is false for
// an empty graph, which is a real state here, so an empty `ids` never repairs.
const REPAIRS = 5;
const DELAY_MS = 80;
const SEPARATOR = "\u0000";

/**
 * Whether the pane is actually being rendered, asked of the pane itself.
 *
 * The store's `width`/`height` cannot answer this: React Flow's resize handler
 * writes `o.width || 500` and `o.height || 500`, so a pane measuring zero — a
 * tab hidden with `display: none`, a shut disclosure — is recorded as 500x500
 * and reads as measurable from the moment it mounts. Asking the pane's own
 * `checkVisibility()` and offset box is the only reading that distinguishes a
 * graph on screen from one that merely has a store entry.
 *
 * A `ResizeObserver` is what re-arms the budget: revealing a `display: none`
 * pane takes its box from zero to real, which is a resize, so the reveal itself
 * is what flips this back to `true`.
 */
function usePaneRendered() {
  const domNode = useStore((state) => state.domNode);
  const [rendered, setRendered] = useState(false);

  const read = useCallback((node) => {
    setRendered(Boolean(node) && node.checkVisibility() && node.offsetWidth > 0 && node.offsetHeight > 0);
  }, []);

  useEffect(() => {
    read(domNode);
    if (!domNode) {
      return undefined;
    }
    const observer = new ResizeObserver(() => read(domNode));
    observer.observe(domNode);
    return () => observer.disconnect();
  }, [domNode, read]);

  return rendered;
}

export function MeasurementGuard({ ids }) {
  const initialized = useNodesInitialized();
  const update = useUpdateNodeInternals();
  const [attempt, setAttempt] = useState(0);
  // Whether the graph can be measured at all right now.
  //
  // An attempt made against a graph that is not being rendered cannot land, so
  // it is not counted. Without this the budget was spent before it could be
  // needed: a graph mounted inside a shut disclosure burned all five repairs in
  // the first 400ms on a pane nobody could see and then retired, so a delivery
  // lost at the moment the reader revealed it had nothing left to repair it.
  //
  // The config surface no longer mounts its graph until the disclosure is open,
  // which is the better half of that fix and lives in `app.mjs`. This half
  // still matters for every graph that is revealed rather than mounted — the
  // editor's Relations tab mounts its pane `hidden` behind the Pipeline tab —
  // and it states the invariant where the repair is, rather than relying on
  // every caller to mount at the right moment.
  const measurable = usePaneRendered();
  // Callers build a fresh array every render; the join gives the effect a
  // dependency that changes only when the graph's nodes actually change.
  const key = ids.join(SEPARATOR);
  const list = useMemo(() => (key === "" ? [] : key.split(SEPARATOR)), [key]);

  useEffect(() => setAttempt(0), [key, measurable]);

  useEffect(() => {
    if (initialized || !measurable || list.length === 0 || attempt >= REPAIRS) {
      return undefined;
    }
    const timer = setTimeout(() => {
      update(list);
      setAttempt((count) => count + 1);
    }, DELAY_MS);
    return () => clearTimeout(timer);
  }, [attempt, initialized, list, measurable, update]);

  return null;
}
