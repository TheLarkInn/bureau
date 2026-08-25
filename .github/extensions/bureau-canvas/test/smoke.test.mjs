// Boot smoke per host (design Q12): each host must come up and answer the
// shared endpoints — GET /state, GET /events, GET / — on an ephemeral
// loopback port, offline. The standalone host is spawned as a child process
// the way a user launches it; the canvas host is booted in-process, the way
// every existing test stubs the Copilot SDK out (BUREAU_CANVAS_TEST=1, so
// the dynamic `@github/copilot-sdk` import never resolves).

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

// Scratch under target/: gitignored, and reachable from a WSL-spawned node
// where an os.tmpdir() on a Windows share would be invisible.
const SCRATCH_ROOT = fileURLToPath(new URL("../../../../target/canvas-smoke-tests/", import.meta.url));
const SERVE = fileURLToPath(new URL("../serve.mjs", import.meta.url));

/** A minimal on-disk config the standalone host can load without a binary. */
async function fixtureConfig() {
  await mkdir(SCRATCH_ROOT, { recursive: true });
  const dir = await mkdtemp(join(SCRATCH_ROOT, "smoke-"));
  await writeFile(join(dir, "repos.yaml"), "repos:\n  bureau:\n    url: https://example.invalid/bureau.git\n    access: push\n");
  return dir;
}

/** First `data:` payload of a named SSE event, or "" if none arrives in time. */
async function readEvent(reader, name, timeoutMs = 5000) {
  const decoder = new TextDecoder();
  let seen = "";
  const until = Date.now() + timeoutMs;
  while (!seen.includes(`event: ${name}`) && Date.now() < until) {
    const { value, done } = await reader.read();
    if (done) {
      break;
    }
    seen += decoder.decode(value, { stream: true });
  }
  return seen;
}

/** The endpoint probes both hosts must answer, against a base URL. */
async function probeSharedEndpoints(base) {
  const stateResponse = await fetch(new URL("/state", base));
  const state = await stateResponse.json();
  const eventsResponse = await fetch(new URL("/events", base));
  const reader = eventsResponse.body.getReader();
  const firstEvent = await readEvent(reader, "state");
  reader.releaseLock();
  await eventsResponse.body.cancel().catch(() => {});
  const page = await fetch(base).then((response) => response.text());
  return { stateResponse, state, eventsResponse, firstEvent, page };
}

function assertSharedEndpoints(probe) {
  assert.deepStrictEqual(
    {
      stateStatus: probe.stateResponse.status,
      stateIsJson: probe.stateResponse.headers.get("content-type")?.includes("application/json"),
      stateHasShape: Boolean(probe.state.canvasId && probe.state.status && probe.state.config),
      eventsStatus: probe.eventsResponse.status,
      eventsIsSse: probe.eventsResponse.headers.get("content-type")?.includes("text/event-stream"),
      eventsOpened: probe.firstEvent.includes("event: state\ndata: "),
      pageIsCanvas: probe.page.includes("Bureau"),
    },
    {
      stateStatus: 200,
      stateIsJson: true,
      stateHasShape: true,
      eventsStatus: 200,
      eventsIsSse: true,
      eventsOpened: true,
      pageIsCanvas: true,
    },
  );
}

test("standalone host boots and serves the shared endpoints", async (t) => {
  const dir = await fixtureConfig();
  t.after(() => rm(dir, { recursive: true, force: true }));
  const child = spawn(process.execPath, [SERVE, "--dir", dir], {
    env: { ...process.env, BUREAU_CANVAS_TEST: "1" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  t.after(() => {
    child.kill("SIGTERM");
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });

  let output = "";
  const url = await new Promise((resolveUrl, rejectUrl) => {
    const deadline = setTimeout(() => rejectUrl(new Error(`standalone host did not print a URL; stderr: ${stderr}`)), 15000);
    child.stdout.on("data", (chunk) => {
      output += chunk;
      const match = output.match(/Bureau dashboard: (http:\/\/127\.0\.0\.1:\d+\/)/u);
      if (match) {
        clearTimeout(deadline);
        resolveUrl(match[1]);
      }
    });
    child.once("exit", (code) => rejectUrl(new Error(`standalone host exited ${code} before boot; stderr: ${stderr}`)));
  });

  try {
    assertSharedEndpoints(await probeSharedEndpoints(url));
  } finally {
    child.kill("SIGTERM");
    await Promise.race([once(child, "exit"), new Promise((resolveWait) => setTimeout(resolveWait, 5000))]);
  }
});

test("canvas host boots with the SDK stubbed and serves the same endpoints", async () => {
  process.env.BUREAU_CANVAS_TEST = "1";
  const canvas = await import("../extension.mjs");
  const instanceId = "bureau-smoke-canvas-host";
  const opened = await canvas.openBureauCanvas({ instanceId, input: {} });

  try {
    assert.match(opened.url, /^http:\/\/127\.0\.0\.1:\d+\/$/u);
    assertSharedEndpoints(await probeSharedEndpoints(opened.url));
  } finally {
    await canvas.closeBureauCanvas({ instanceId });
  }
});
