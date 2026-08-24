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
import { bureauCandidates, wslShare, wslSharePath } from "./findings.mjs";

const EVENTS_FILE = "events.jsonl";
const RUN_STARTED = "run_started";
const RUN_FINISHED = "run_finished";
const STEP_STARTED = "step_started";
const STEP_FINISHED = "step_finished";

/**
 * The bureau home's `runs/` as *this process's host* resolves it:
 * `BUREAU_CANVAS_RUNS` when set, else `runs/` under the bureau home
 * (`BUREAU_HOME`, default `~/.bureau`) — the same default the CLI's
 * `home.layout().runs()` resolves to. Callers that serve a canvas want
 * `resolveRunsDir()` instead, which also handles bureau living in a WSL
 * distro while the canvas host does not.
 */
export function runsDir(env = process.env) {
  if (env.BUREAU_CANVAS_RUNS) {
    return resolve(env.BUREAU_CANVAS_RUNS);
  }
  return join(env.BUREAU_HOME ?? join(homedir(), ".bureau"), "runs");
}

/** `sh` inside the distro, so a `BUREAU_HOME` set there is honored too. */
const HOME_PROBE = 'printf %s "${BUREAU_HOME:-$HOME/.bureau}"';
const PROBE_TIMEOUT_MS = 5000;

/**
 * The runs root for one canvas server, resolved once at startup.
 *
 * `runsDir()` answers for the host process, which is only correct when bureau
 * runs on that host. In the usual Windows setup it does not: the workspace and
 * the binary live inside a WSL distro, so the bureau home — and every run
 * directory under it — sits on the distro's filesystem while this process asks
 * about `C:\Users\...\.bureau\runs` and finds nothing. An empty answer here is
 * what makes the live and replay run pickers permanently empty.
 *
 * So when the canvas is looking at a distro, ask that distro where its home is
 * and address it through the share. `wslBridged`'s `translate` turns the share
 * path back into a Linux path when it is passed as `--runs`, so run replay and
 * pause/resume/cancel keep working against the same root.
 */
export async function resolveRunsDir(options = {}) {
  const env = options.env ?? process.env;
  if (env.BUREAU_CANVAS_RUNS || env.BUREAU_HOME) {
    return runsDir(env);
  }
  const distro = await bureauDistro(options);
  const home = distro ? await distroHome(distro, options) : null;
  return home ? wslSharePath(distro, `${home}/runs`) : runsDir(env);
}

/**
 * The distro bureau lives in. The canvas's own anchor answers without spawning
 * anything and works even when no binary is built yet; the resolved binary is
 * the fallback for a workspace that sits outside the distro.
 */
async function bureauDistro(options) {
  const anchored = wslShare(options.anchor ?? "")?.distro;
  if (anchored) {
    return anchored;
  }
  const [candidate] = await bureauCandidates(options);
  return candidate?.distro ?? null;
}

async function distroHome(distro, options) {
  const probe = options.probe ?? wslProbe;
  const home = await probe(distro, HOME_PROBE);
  return home?.trim() ? home.trim() : null;
}

/** Bounded: a wedged `wsl.exe` must not keep the canvas from opening. */
function wslProbe(distro, script) {
  const child = spawn("wsl.exe", ["-d", distro, "--", "sh", "-c", script], { windowsHide: true });
  const timer = setTimeout(() => child.kill(), PROBE_TIMEOUT_MS);
  return collect(child).then((run) => {
    clearTimeout(timer);
    return run?.code === 0 ? run.stdout : null;
  });
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

/**
 * Which pipeline a run belongs to, read where bureau actually writes it.
 *
 * `run_started` carries no `pipeline` field: `RunStartedData` is `run_id`,
 * `assignment`, `item` and `snapshot` (`crates/bureau/src/runlog/event.rs`).
 * The pinned pipeline lives inside the snapshot — as the pipeline's own name,
 * and again as the name the assignment selected. Reading a flat `data.pipeline`
 * compiled and tested clean while being `null` for every run bureau has ever
 * written, so the ordering here is deliberate: snapshot first, and a flat field
 * only as the forward-compatible last resort if one is ever added.
 *
 * A run whose log predates snapshots resolves to `null`, which is a run this
 * canvas cannot attribute — not a run belonging to no pipeline.
 */
function pipelineOf(started) {
  const snapshot = started?.data?.snapshot;
  return snapshot?.pipeline?.name ?? snapshot?.assignment?.pipeline ?? started?.data?.pipeline ?? null;
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
    pipeline: pipelineOf(started),
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
