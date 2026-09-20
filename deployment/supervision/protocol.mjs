import { randomBytes } from "node:crypto";
import { StringDecoder } from "node:string_decoder";

import { requireValue } from "../../scripts/maintenance-contract.mjs";

export const SCHEMA = "bureau-windows-v1";
export const FRAME_BYTES = 2048;
export const LIMITS = Object.freeze({ response: 6000, startup: 30_000, interval: 1000, command: 2000, drain: 20_000 });
export const ID = /^[a-f0-9]{32}$/u;
export const COMMIT = /^[a-f0-9]{40}$/u;
export const nonce = () => randomBytes(16).toString("hex");

export function exactKeys(value, names) {
  requireValue(value !== null && typeof value === "object" && !Array.isArray(value)
    && Object.keys(value).sort().join(",") === [...names].sort().join(","), "invalid supervision fields");
}

export function identity(value) {
  exactKeys(value, ["invocation", "pid", "starttime", "cgroup"]);
  requireValue(typeof value.invocation === "string" && ID.test(value.invocation)
    && Number.isSafeInteger(value.pid) && value.pid > 1
    && typeof value.starttime === "string" && /^[1-9]\d{0,19}$/u.test(value.starttime)
    && typeof value.cgroup === "string" && /^\/system\.slice\/[a-zA-Z0-9_.@-]+\.service$/u.test(value.cgroup),
  "unobservable owned identity");
  return value;
}

export function sameIdentity(left, right) {
  return ["invocation", "pid", "starttime", "cgroup"].every((key) => left[key] === right[key]);
}

export function heartbeat(value, challenge, owner, commit) {
  exactKeys(value, ["schema", "type", "nonce", "sequence", "guard", "owner", "commit", "ageMs"]);
  requireValue(value.schema === SCHEMA && value.type === "heartbeat"
    && value.nonce === challenge.nonce && value.sequence === challenge.sequence
    && value.guard === challenge.guard && typeof value.owner === "string" && ID.test(value.owner)
    && (!owner || value.owner === owner) && typeof value.commit === "string" && value.commit === commit
    && typeof value.nonce === "string" && typeof value.guard === "string"
    && Number.isSafeInteger(value.ageMs) && value.ageMs >= 0 && value.ageMs <= 2000,
  "invalid or stale ownership heartbeat");
  return value.owner;
}

// Only one requested frame may be outstanding. EOF and unsolicited/buffered
// heartbeats cannot extend a lease, even if the writer later stops responding.
export function frames(stream) {
  const decoder = new StringDecoder("utf8");
  let pending = null;
  let buffer = "";
  let failure = null;
  const fail = (error) => {
    failure ??= error;
    pending?.reject(failure);
    pending = null;
  };
  const data = (chunk) => {
    if (failure) return;
    if (!pending || Buffer.byteLength(buffer) + chunk.length > FRAME_BYTES) {
      fail(new Error("unsolicited or oversized supervision input"));
      return;
    }
    buffer += decoder.write(chunk);
    const end = buffer.indexOf("\n");
    if (end < 0) return;
    if (end !== buffer.length - 1 || buffer.includes("\uFFFD")) {
      fail(new Error("malformed or buffered supervision input"));
      return;
    }
    const waiter = pending;
    pending = null;
    try { waiter.resolve(JSON.parse(buffer)); } catch { fail(new Error("malformed supervision JSON")); waiter.reject(failure); }
    buffer = "";
  };
  const end = () => fail(new Error("supervision input closed"));
  stream.on("data", data).on("end", end).on("error", fail);
  return {
    read(timeout) {
      if (failure) return Promise.reject(failure);
      requireValue(!pending, "concurrent supervision read");
      return new Promise((resolve, reject) => {
        const timer = timeout === undefined ? null
          : setTimeout(() => fail(new Error("ownership heartbeat deadline exceeded")), timeout);
        pending = {
          resolve(value) { clearTimeout(timer); resolve(value); },
          reject(error) { clearTimeout(timer); reject(error); },
        };
      });
    },
    check() { if (failure) throw failure; },
    close() {
      fail(new Error("supervision reader closed"));
      stream.off("data", data).off("end", end).off("error", fail);
      stream.pause();
    },
  };
}

export async function writeFrame(stream, value) {
  const text = `${JSON.stringify(value)}\n`;
  requireValue(Buffer.byteLength(text) <= FRAME_BYTES, "supervision output exceeds its bound");
  await new Promise((resolve, reject) => stream.write(text, (error) => error ? reject(error) : resolve()));
}
