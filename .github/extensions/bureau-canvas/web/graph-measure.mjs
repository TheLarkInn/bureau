import { useEffect, useMemo, useState } from "react";
import { useStore, useStoreApi, useUpdateNodeInternals } from "@xyflow/react";
import { hasNodeMeasurement, measuredGraphNodes } from "./graph-measure-state.mjs";

// Controlled-node updates can lose internal measurements without changing the
// DOM box. Bound repairs per loss episode, not per lifetime of the node IDs.
const REPAIRS = 5;
const DELAY_MS = 80;
const SEPARATOR = "\u0000";

export function useGraphMeasurement(ids) {
  const key = ids.join(SEPARATOR);
  const list = useMemo(() => (key === "" ? [] : key.split(SEPARATOR)), [key]);
  // The vendor hook checks internals.userNode; these controlled props do not
  // receive dimension changes. Rendering and cameras use the internal nodes.
  const ready = useStore((state) => measuredGraphNodes(list, (id) => state.nodeLookup.get(id)) !== null);
  return { key, list, ready };
}

export function MeasurementGuard({ ids }) {
  const { key, list, ready } = useGraphMeasurement(ids);
  const store = useStoreApi();
  const update = useUpdateNodeInternals();
  const [attempt, setAttempt] = useState(0);

  useEffect(() => setAttempt(0), [key, ready]);

  useEffect(() => {
    if (ready || list.length === 0 || attempt >= REPAIRS) {
      return undefined;
    }
    const timer = setTimeout(() => {
      const missing = list.filter((id) => !hasNodeMeasurement(store.getState().nodeLookup.get(id)));
      if (missing.length) {
        update(missing);
        setAttempt((count) => count + 1);
      }
    }, DELAY_MS);
    return () => clearTimeout(timer);
  }, [attempt, ready, list, store, update]);

  return null;
}
