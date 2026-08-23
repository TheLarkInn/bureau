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

import React, { useEffect, useMemo, useState } from "react";
import { useNodesInitialized, useUpdateNodeInternals } from "@xyflow/react";

// Enough attempts to cover a lost delivery, and few enough to stop rather than
// spin if a node has genuinely left the DOM. `useNodesInitialized` is false for
// an empty graph, which is a real state here, so an empty `ids` never repairs.
const REPAIRS = 5;
const DELAY_MS = 80;
const SEPARATOR = "\u0000";

export function MeasurementGuard({ ids }) {
  const initialized = useNodesInitialized();
  const update = useUpdateNodeInternals();
  const [attempt, setAttempt] = useState(0);
  // Callers build a fresh array every render; the join gives the effect a
  // dependency that changes only when the graph's nodes actually change.
  const key = ids.join(SEPARATOR);
  const list = useMemo(() => (key === "" ? [] : key.split(SEPARATOR)), [key]);

  useEffect(() => setAttempt(0), [key]);

  useEffect(() => {
    if (initialized || list.length === 0 || attempt >= REPAIRS) {
      return undefined;
    }
    const timer = setTimeout(() => {
      update(list);
      setAttempt((count) => count + 1);
    }, DELAY_MS);
    return () => clearTimeout(timer);
  }, [attempt, initialized, list, update]);

  return null;
}
