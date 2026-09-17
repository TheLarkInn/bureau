import { open, opendir } from "node:fs/promises";
import { join } from "node:path";
import { summarize } from "./runs.mjs";
import { applyEvents } from "../web/live/overlay.js";
import { inspectEventLog, runIdentityProblem } from "../web/run-evidence.mjs";

export const MAX_RUN_LOG_BYTES = 2 * 1024 * 1024;
export const MAX_LISTING_BYTES = 16 * 1024 * 1024;
export const MAX_RUN_DIRECTORIES = 200;

async function limitedLog(path) {
  const file = await open(path, "r");
  try {
    const info = await file.stat();
    if (!info.isFile()) throw new Error("events.jsonl is not a regular file");
    if (info.size > MAX_RUN_LOG_BYTES) return null;
    const chunks = [];
    let size = 0;
    while (size <= MAX_RUN_LOG_BYTES) {
      const buffer = Buffer.alloc(Math.min(64 * 1024, MAX_RUN_LOG_BYTES + 1 - size));
      const { bytesRead } = await file.read(buffer, 0, buffer.length, size);
      if (!bytesRead) return Buffer.concat(chunks, size).toString("utf8");
      chunks.push(buffer.subarray(0, bytesRead));
      size += bytesRead;
    }
    return null;
  } finally {
    await file.close();
  }
}

export async function readRunLog(dir, runId) {
  let text;
  try {
    text = await limitedLog(join(dir, runId, "events.jsonl"));
  } catch (error) {
    return { events: [], warning: null, error: error.code === "ENOENT"
      ? "Run log is missing; run state is unknown."
      : `Run log unreadable (${error.code ?? error.message}); run state is unknown.` };
  }
  if (text === null) return { events: [], warning: null, limited: true,
    error: "Run log exceeds the 2 MiB preview limit; use Bureau CLI inspection. No state was inferred." };
  const log = inspectEventLog(text);
  return { ...log, byte_length: Buffer.byteLength(text), error: log.error ?? runIdentityProblem(runId, log.events) };
}

function recordedCost(finished, factories) {
  const incomplete = factories.error || Object.values(factories.records).some((record) =>
    !record.accounting || record.accountingIncomplete || record.problem);
  const cost = finished?.data?.cost_usd;
  return !incomplete && typeof cost === "number" && Number.isFinite(cost) && cost >= 0 ? cost : null;
}

export function observeRun(runId, log) {
  const { events, error, warning } = log;
  const safe = error ? [] : events;
  const summary = summarize(runId, safe);
  const overlay = applyEvents(safe);
  const started = safe.find((event) => event.kind === "run_started");
  const finished = safe.find((event) => event.kind === "run_finished");
  return {
    ...summary,
    live: !error && summary.live,
    evidence: {
      state: log.limited ? "limited" : error ? "invalid" : warning ? "partial" : "readable",
      message: error ?? warning,
      last_at_ms: safe.at(-1)?.at_ms ?? null,
      status: error ? "unknown" : overlay.status,
      outcome: finished?.data?.outcome ?? null,
      terminal: finished?.data?.terminal ?? null,
      detail: finished?.data?.message ?? null,
      cost_usd: error ? null : recordedCost(finished, overlay.factories),
      config_source: started?.data?.snapshot?.config_source ?? null,
    },
  };
}

export async function readRunListing(dir, now = Date.now()) {
  const runs = [];
  let limitation = null;
  let scanned = 0;
  let bytes = 0;
  try {
    const entries = await opendir(dir);
    for await (const entry of entries) {
      if (scanned >= MAX_RUN_DIRECTORIES) {
        limitation = `Directory preview is limited to ${MAX_RUN_DIRECTORIES} entries in filesystem order`;
        break;
      }
      scanned += 1;
      if (!entry.isDirectory()) continue;
      const log = await readRunLog(dir, entry.name);
      bytes += log.byte_length ?? 0;
      if (bytes > MAX_LISTING_BYTES) {
        limitation = "Directory preview is limited to 16 MiB of log evidence";
        break;
      }
      runs.push(observeRun(entry.name, log));
    }
  } catch (error) {
    return { runs, observation: {
      state: error.code === "ENOENT" ? "missing" : "error", at_ms: now, dir,
      message: error.code === "ENOENT" ? "Run directory is missing; no run evidence has been read."
        : `Run directory unreadable (${error.code ?? error.message}); activity is unknown.`,
    } };
  }
  return { runs: runs.sort((a, b) => a.run_id.localeCompare(b.run_id)),
    observation: { state: limitation ? "limited" : "ready", at_ms: now, dir,
      message: limitation ? `${limitation}; other runs may need attention. Use Bureau CLI for the complete inventory.` : null } };
}
