import { PassThrough } from "node:stream";
import { setTimeout as sleep } from "node:timers/promises";

import { SCHEMA } from "../deployment/supervision/protocol.mjs";
import { supervise } from "../deployment/supervision/heartbeat.mjs";

export const COMMIT = "1".repeat(40);
export const OWNER = "2".repeat(32);
export const GUARD = { invocation: "3".repeat(32), pid: 123, starttime: "456", cgroup: "/system.slice/test-guard.service" };
export const ENGINE = { invocation: "4".repeat(32), pid: 789, starttime: "1011", cgroup: "/system.slice/test-engine.service" };

export function reply(challenge, patch = {}) {
  return { schema: SCHEMA, type: "heartbeat", nonce: challenge.nonce, sequence: challenge.sequence,
    guard: challenge.guard, owner: OWNER, commit: COMMIT, ageMs: 0, ...patch };
}

export function harness(receive, options = {}) {
  const input = new PassThrough();
  const output = new PassThrough();
  const events = [];
  let current = { state: "running", identity: ENGINE };
  const effects = {
    now: () => performance.now(), wait: sleep, root: async () => "",
    prepare: async () => { events.push("prepare"); return GUARD; },
    observe: async () => current,
    fresh: async () => { events.push("fresh"); },
    notify: async (message) => { events.push(message); },
    ...options.effects,
  };
  output.on("data", (chunk) => receive(JSON.parse(chunk), input, {
    events, setState(value) { current = value; },
  }));
  const running = supervise({ input, output, commit: COMMIT, effects,
    limits: { response: 80, startup: 200, interval: 2, ...options.limits } });
  return { input, output, events, running };
}
