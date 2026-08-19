// Run observation and control for the canvas server (DESIGN.md layer 3):
// `GET /runs`, `GET /runs/<id>/events`, the `run-event` SSE forwarding, and
// the pause/resume/cancel intents.
//
// The event log is the only source of truth and liveness is pure filesystem:
// a run is live when its `events.jsonl` holds no `run_finished` event. No
// daemon is consulted, and control goes through the CLI — the canvas never
// writes run markers itself.

import { spawn } from "node:child_process";
import { open, readdir, readFile, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { bureauCandidates } from "./findings.mjs";

const EVENTS_FILE = "events.jsonl";
const RUN_STARTED = "run_started";
const RUN_FINISHED = "run_finished";
const STEP_STARTED = "step_started";
const STEP_FINISHED = "step_finished";

/**
 * The directory holding run directories: `BUREAU_CANVAS_RUNS` when set, else
 * `runs/` under the bureau home (`BUREAU_HOME`, default `~/.bureau`) — the
 * same default the CLI's `home.layout().runs()` resolves to.
 */
export function runsDir(env = process.env) {
  if (env.BUREAU_CANVAS_RUNS) {
    return resolve(env.BUREAU_CANVAS_RUNS);
  }
  return join(env.BUREAU_HOME ?? join(homedir(), ".bureau"), "runs");
}

/**
 * Runs `bureau <args>` and collects its output. `options.exec` replaces the
 * spawn entirely (tests inject argument capture there). Returns `null` when
 * no binary is available, so callers fall back to reading the log directly —
 * the same fixture-fallback shape as config validation.
 */
export async function runBureau(args, options = {}) {
  if (options.exec) {
    return options.exec(args);
  }
  const [candidate] = await bureauCandidates(options);
  if (!candidate) {
    return null;
  }
  const translated = args.map((arg) => candidate.translate?.(arg) ?? arg);
  const child = spawn(candidate.command, [...candidate.args, ...translated], { windowsHide: true });
  return collect(child);
}

function collect(child) {
  return new Promise((resolveRun) => {
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8").on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", () => resolveRun(null));
    child.on("close", (code) => resolveRun({ code, stdout, stderr }));
  });
}

/** One summary per run directory with an event log, sorted by run id. */
export async function listRuns(dir) {
  const entries = await readdir(dir, { withFileTypes: true }).catch(() => []);
  const summaries = [];
  for (const entry of entries) {
    if (entry.isDirectory()) {
      const events = await readRunEvents(dir, entry.name);
      if (events) {
        summaries.push(summarize(entry.name, events));
      }
    }
  }
  return summaries.sort((a, b) => a.run_id.localeCompare(b.run_id));
}

/** The parsed log for one run, or `null` when it has no readable log. */
export async function readRunEvents(dir, runId) {
  const text = await readFile(join(dir, runId, EVENTS_FILE), "utf8").catch(() => null);
  return text === null ? null : parseEvents(text);
}

/**
 * Newline-delimited JSON. A torn final line — a daemon kill mid-append — is
 * dropped, never an error, matching the CLI's tolerant read.
 */
export function parseEvents(text) {
  const events = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) {
      continue;
    }
    try {
      events.push(JSON.parse(line));
    } catch {
      // torn line
    }
  }
  return events;
}

/** Liveness, identity, and the step a run is currently inside. */
export function summarize(runId, events) {
  const started = events.find((event) => event.kind === RUN_STARTED);
  const openSteps = [];
  let finished = false;
  for (const event of events) {
    if (event.kind === STEP_STARTED) {
      openSteps.push(event.data?.step ?? null);
    } else if (event.kind === STEP_FINISHED) {
      const index = openSteps.lastIndexOf(event.data?.step ?? null);
      if (index >= 0) {
        openSteps.splice(index, 1);
      }
    } else if (event.kind === RUN_FINISHED) {
      finished = true;
    }
  }
  return {
    run_id: runId,
    assignment: started?.data?.assignment ?? null,
    started_at: typeof started?.at_ms === "number" ? new Date(started.at_ms).toISOString() : null,
    live: !finished,
    current_step: openSteps.at(-1) ?? null,
  };
}

/**
 * Tails every live run's log and publishes each appended event as
 * `{ run_id, event }`. Polling, not `fs.watch`: the log is appended and
 * fsync'd by another process and, in this repo's dev setup, can sit on a
 * WSL share where watch events are unreliable or never arrive. A one-second
 * poll is cheap at run-log sizes and behaves identically on every platform.
 *
 * Tailing starts at the end of each file on first sight — replay is
 * `GET /runs/<id>/events`, not this channel. A run is dropped when its
 * `run_finished` arrives.
 */
export function createRunTail({ dir, publish, intervalMs = 1000 }) {
  const tails = new Map();
  const finished = new Set();
  let timer = null;
  let polling = false;

  async function poll() {
    if (polling) {
      return;
    }
    polling = true;
    try {
      const entries = await readdir(dir, { withFileTypes: true }).catch(() => []);
      for (const entry of entries) {
        if (entry.isDirectory() && !finished.has(entry.name) && !tails.has(entry.name)) {
          await stat(join(dir, entry.name, EVENTS_FILE))
            .then((info) => info.isFile() && tails.set(entry.name, { offset: info.size, pending: "" }))
            .catch(() => {});
        }
      }
      for (const [runId, tail] of tails) {
        if (!finished.has(runId)) {
          await readAppended(join(dir, runId, EVENTS_FILE), tail, runId);
        }
      }
    } finally {
      polling = false;
    }
  }

  async function readAppended(path, tail, runId) {
    const info = await stat(path).catch(() => null);
    if (!info?.isFile()) {
      return;
    }
    if (info.size < tail.offset) {
      tail.offset = 0;
      tail.pending = "";
    }
    if (info.size === tail.offset) {
      return;
    }
    const file = await open(path, "r").catch(() => null);
    if (!file) {
      return;
    }
    try {
      const buffer = Buffer.alloc(info.size - tail.offset);
      await file.read(buffer, 0, buffer.length, tail.offset);
      tail.offset = info.size;
      consume(tail, buffer.toString("utf8"), runId);
    } finally {
      await file.close();
    }
  }

  function consume(tail, chunk, runId) {
    const lines = `${tail.pending}${chunk}`.split("\n");
    tail.pending = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) {
        continue;
      }
      const event = parseEvents(line)[0];
      if (!event) {
        continue;
      }
      publish({ run_id: runId, event });
      if (event.kind === RUN_FINISHED) {
        finished.add(runId);
        tails.delete(runId);
      }
    }
  }

  return {
    poll,
    start() {
      timer ??= setInterval(() => void poll(), intervalMs);
      timer.unref();
    },
    stop() {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    },
  };
}
