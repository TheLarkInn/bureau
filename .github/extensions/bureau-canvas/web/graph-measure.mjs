import { useCallback, useEffect, useMemo, useState } from "react";
import { useStore, useStoreApi, useUpdateNodeInternals } from "@xyflow/react";
import { hasNodeMeasurement, measuredGraphNodes } from "./graph-measure-state.mjs";

// Controlled-node updates can lose internal measurements without changing the
// DOM box. Bound repairs per loss episode, not per lifetime of the node IDs.
const REPAIRS = 5;
const DELAY_MS = 80;
const SEPARATOR = "\u0000";

function surfaceVisible(surface) {
  return surface?.offsetWidth > 0 && surface?.offsetHeight > 0;
}

function useVisibleSurface() {
  const surface = useStore((state) => state.domNode);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const observe = () => setVisible(surfaceVisible(surface));
    observe();
    if (!surface) return undefined;
    const observer = new ResizeObserver(observe);
    observer.observe(surface);
    return () => observer.disconnect();
  }, [surface]);
  return visible;
}

export function useGraphMeasurement(ids) {
  const key = ids.join(SEPARATOR);
  const list = useMemo(() => (key === "" ? [] : key.split(SEPARATOR)), [key]);
  // The vendor hook checks internals.userNode; these controlled props do not
  // receive dimension changes. Rendering and cameras use the internal nodes.
  const ready = useStore((state) => measuredGraphNodes(list, (id) => state.nodeLookup.get(id)) !== null);
  const visible = useVisibleSurface();
  const store = useStoreApi();
  const update = useUpdateNodeInternals();
  const [repair, setRepair] = useState(() => ({ key, attempt: 0 }));
  const attempt = repair.key === key ? repair.attempt : 0;
  const retry = useCallback(() => setRepair({ key, attempt: 0 }), [key]);

  useEffect(() => setRepair({ key, attempt: 0 }), [key, ready, visible]);

  useEffect(() => {
    if (ready || !visible || list.length === 0 || attempt >= REPAIRS) {
      return undefined;
    }
    let frame;
    const timer = setTimeout(() => {
      const state = store.getState();
      if (!surfaceVisible(state.domNode)) return;
      const missing = list.filter((id) => !hasNodeMeasurement(state.nodeLookup.get(id)));
      if (missing.length) {
        update(missing);
        // The vendor applies the repair in its own rAF. Account afterward so
        // the fifth successful delivery cannot be reported as exhaustion.
        frame = requestAnimationFrame(() => setRepair((current) =>
          current.key === key ? { key, attempt: current.attempt + 1 } : current));
      }
    }, DELAY_MS);
    return () => {
      clearTimeout(timer);
      cancelAnimationFrame(frame);
    };
  }, [attempt, ready, visible, key, list, store, update]);

  return { list, ready, visible, retry, exhausted: !ready && visible && attempt >= REPAIRS };
}
