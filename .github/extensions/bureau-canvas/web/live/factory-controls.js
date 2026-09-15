import { useEffect, useState } from "react";
import { parseFactoryControls } from "./factory-controls.mjs";

export function useFactoryControls(runId, record, busy) {
  const [state, setState] = useState({ runId: null, control: null, error: null });
  const sessionId = record?.sessionId;
  const eventSeq = record?.eventSeq;
  const clean = record?.executionClean;
  const active = record?.runtimeActive;
  useEffect(() => {
    setState({ runId, control: null, error: null });
    if (!sessionId || !clean || active || busy) return undefined;
    const controller = new AbortController();
    let alive = true;
    fetch(`./runs/${encodeURIComponent(runId)}/controls`, { cache: "no-store", signal: controller.signal })
      .then(async (response) => {
        const payload = await response.json();
        if (!response.ok) throw new Error(payload.error ?? "Bureau could not verify local factory resume.");
        return parseFactoryControls(payload, runId);
      })
      .then((control) => { if (alive) setState({ runId, control,
        error: control ? null : "Bureau reports no active local factory to continue." }); })
      .catch((error) => { if (alive) setState({ runId, control: null, error: String(error.message ?? error) }); });
    return () => { alive = false; controller.abort(); };
  }, [runId, sessionId, eventSeq, clean, active, busy]);
  return state.runId === runId ? state : { control: null, error: null };
}
