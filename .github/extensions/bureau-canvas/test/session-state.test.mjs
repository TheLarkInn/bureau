import assert from "node:assert/strict";
import { test } from "node:test";

import { sessionValue, storeSessionValue } from "../web/session-state.js";

test("dashboard selections survive reload and clear explicitly", () => {
  const values = new Map();
  globalThis.sessionStorage = {
    getItem: (key) => values.get(key) ?? null,
    removeItem: (key) => values.delete(key),
    setItem: (key, value) => values.set(key, value),
  };
  storeSessionValue("pipeline-mode", "live");
  const stored = sessionValue("pipeline-mode");
  storeSessionValue("pipeline-mode", null);
  storeSessionValue("live-run:pipeline-a", "run-a");
  storeSessionValue("live-run:pipeline-b", "run-b");
  storeSessionValue("selected-step:pipeline-a", "verify");
  storeSessionValue("selected-step:pipeline-b", "review");
  assert.deepStrictEqual(
    [
      stored,
      sessionValue("pipeline-mode", "design"),
      sessionValue("live-run:pipeline-a"),
      sessionValue("live-run:pipeline-b"),
      sessionValue("selected-step:pipeline-a"),
      sessionValue("selected-step:pipeline-b"),
    ],
    ["live", "design", "run-a", "run-b", "verify", "review"],
  );
});
