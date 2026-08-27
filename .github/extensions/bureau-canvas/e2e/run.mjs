import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

process.env.BUREAU_CANVAS_TEST = "1";

const canvas = await import("../extension.mjs");
// `BUREAU_CANVAS_EDGE` overrides the lookup. The default is where the Windows
// installer puts it; the override is for an install that is somewhere else, or
// for a non-Windows Edge. It is not a way to drive a Windows Edge from another
// OS — see `crossOsReason`.
const EDGE_EXE = process.env.BUREAU_CANVAS_EDGE
  ?? "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const E2E_DIR = fileURLToPath(new URL("./", import.meta.url));
const SCREENSHOT_DIR = join(E2E_DIR, "screenshots");
const PROFILE_DIR = join(E2E_DIR, ".edge-profiles");
const REFERENCE_FIXTURE = new URL("../test/fixtures/reference-payload.json", import.meta.url);
const COMMITTED_FIXTURE = new URL("../test/fixtures/committed-payload.json", import.meta.url);
// Scratch config for the CRUD flow. `target/` is gitignored and is not a
// scanned config directory, and the binary runs inside WSL where a Windows
// temp directory is invisible.
const CRUD_ROOT = fileURLToPath(new URL("../../../../target/canvas-crud-tests/", import.meta.url));
const VIEWPORT = { width: 1920, height: 1200, deviceScaleFactor: 1, mobile: false };
const screenshots = [];
const results = [];

class CdpSession {
  constructor(wsUrl) {
    this.ws = new WebSocket(wsUrl);
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Map();
    this.diagnostics = [];
    this.ws.addEventListener("message", (event) => this.receive(event.data));
  }

  async open(timeout = 10_000) {
    if (this.ws.readyState === WebSocket.OPEN) {
      return;
    }
    await withTimeout(new Promise((resolve, reject) => {
      this.ws.addEventListener("open", resolve, { once: true });
      this.ws.addEventListener("error", reject, { once: true });
    }), timeout, "Timed out connecting to DevTools WebSocket");
  }

  call(method, params = {}, timeout = 8_000) {
    const id = this.nextId;
    this.nextId += 1;
    const message = JSON.stringify({ id, method, params });
    const done = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timed out waiting for CDP ${method}`));
      }, timeout);
      this.pending.set(id, { method, resolve, reject, timer });
    });
    this.ws.send(message);
    return done;
  }

  waitForEvent(method, timeout = 15_000) {
    return withTimeout(new Promise((resolve) => {
      const list = this.listeners.get(method) ?? [];
      list.push(resolve);
      this.listeners.set(method, list);
    }), timeout, `Timed out waiting for CDP event ${method}`);
  }

  resetDiagnostics() {
    this.diagnostics = [];
  }

  async receive(data) {
    const message = JSON.parse(await messageText(data));
    if (message.id) {
      this.complete(message);
      return;
    }
    this.captureDiagnostic(message);
    for (const listener of this.listeners.get(message.method) ?? []) {
      listener(message.params);
    }
    this.listeners.delete(message.method);
  }

  complete(message) {
    const entry = this.pending.get(message.id);
    if (!entry) {
      return;
    }
    clearTimeout(entry.timer);
    this.pending.delete(message.id);
    if (message.error) {
      entry.reject(new Error(`${entry.method} failed: ${message.error.message}`));
    } else {
      entry.resolve(message.result ?? {});
    }
  }

  captureDiagnostic(message) {
    if (message.method === "Runtime.exceptionThrown") {
      this.diagnostics.push({ kind: "exception", text: exceptionText(message.params) });
    }
    if (message.method === "Runtime.consoleAPICalled" && message.params?.type === "error") {
      this.diagnostics.push({ kind: "console.error", text: consoleText(message.params.args) });
    }
    if (message.method === "Log.entryAdded" && message.params?.entry?.level === "error") {
      this.diagnostics.push({ kind: "log.error", text: message.params.entry.text });
    }
  }

  close() {
    this.ws.close();
  }
}

async function main() {
  const skip = skipReason();
  if (skip) {
    console.log(`bureau-canvas e2e skipped: ${skip}`);
    return;
  }
  await mkdir(SCREENSHOT_DIR, { recursive: true });
  console.log("bureau-canvas e2e: launching Microsoft Edge over CDP");
  const browser = await launchEdge();
  let fatal = null;
  try {
    console.log("bureau-canvas e2e: running browser assertions");
    await runSuite(browser.page);
  } catch (error) {
    fatal = error;
    results.push({ name: "browser harness completed all required renders", ok: false, error: error.message });
  } finally {
    try {
      await browser.close();
    } catch (error) {
      fatal ??= error;
      results.push({ name: "Edge browser exited and profile cleaned", ok: false, error: error.message });
    }
  }
  printReport();
  if (fatal || results.some((result) => !result.ok)) {
    process.exitCode = 1;
  }
}

function skipReason() {
  // The cross-OS pairing is read from the *shape* of the two paths, so it does
  // not need the executable to exist — and it has to be asked first. On WSL a
  // Windows Edge is installed and perfectly real, but `C:\...` is not a path
  // this host can stat, so the existence check below would blame a missing
  // browser for a pairing this harness already knows how to name. Naming the
  // pairing is the whole point; getting there only when Edge happens to be
  // absent for a second reason defeats it.
  const crossOs = crossOsReason();
  if (crossOs) {
    return crossOs;
  }
  if (!existsSync(EDGE_EXE)) {
    return `Microsoft Edge not found at ${EDGE_EXE}`;
  }
  if (typeof WebSocket !== "function") {
    return "Node global WebSocket is unavailable; run with Node 24 or newer";
  }
  // There is deliberately no network preflight. One used to stand here, from
  // when the pages loaded React from esm.sh; `d9556b1` vendored the renderer an
  // hour later and `render.test.mjs` now asserts the page carries no remote
  // reference at all. Left in place it gated a fully offline harness on the
  // CDN it had stopped using — so an offline machine skipped every assertion
  // below and still exited 0. A harness may name a pairing it cannot run,
  // which is what `crossOsReason` does; it may not pass by running nothing.
  return null;
}

/**
 * A Windows Edge cannot be driven from a POSIX host. It reads
 * `--user-data-dir=/home/...` as a Windows path, so `DevToolsActivePort` is
 * written somewhere this process cannot see and the browser exits with an
 * empty stderr — twenty seconds of waiting and nothing to go on. Under WSL's
 * default NAT networking the CDP port would not be reachable either, since
 * Edge binds Windows' loopback and not this one.
 *
 * Naming the pairing is the whole point: an unrunnable harness should say
 * which two things cannot be paired, not fail blank.
 */
function crossOsReason() {
  if (!EDGE_EXE.toLowerCase().endsWith(".exe") || !PROFILE_DIR.startsWith("/")) {
    return null;
  }
  return `${EDGE_EXE} is a Windows browser but this is a POSIX host, so its profile would be ${PROFILE_DIR}, a path Windows cannot open; run this harness with the Windows node`;
}

async function launchEdge() {
  await mkdir(PROFILE_DIR, { recursive: true });
  const profile = join(PROFILE_DIR, `profile-${process.pid}-${Date.now()}`);
  await mkdir(profile, { recursive: true });
  const args = [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "about:blank",
  ];
  const child = spawn(EDGE_EXE, args, { stdio: ["ignore", "ignore", "pipe"] });
  const stderr = [];
  child.stderr.on("data", (chunk) => stderr.push(chunk.toString()));
  try {
    const port = await readDevToolsPort(profile, child, stderr);
    const target = await browserTarget(port);
    const page = new CdpSession(target.webSocketDebuggerUrl);
    await page.open();
    await page.call("Page.enable");
    await page.call("Runtime.enable");
    await page.call("Log.enable");
    await page.call("Emulation.setDeviceMetricsOverride", VIEWPORT);
    return { page, close: () => closeBrowser(child, profile, page) };
  } catch (error) {
    await closeBrowser(child, profile);
    throw error;
  }
}

async function readDevToolsPort(profile, child, stderr) {
  const activePort = join(profile, "DevToolsActivePort");
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (child.exitCode != null) {
      throw new Error(`Edge exited before DevTools opened: ${stderr.join("").trim()}`);
    }
    if (existsSync(activePort)) {
      const [port] = (await readFile(activePort, "utf8")).trim().split(/\r?\n/);
      return port;
    }
    await delay(100);
  }
  throw new Error(`Timed out waiting for ${activePort}`);
}

async function browserTarget(port) {
  const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(5_000) });
  const targets = await response.json();
  const target = targets.find((item) => item.type === "page" && item.webSocketDebuggerUrl);
  if (!target) {
    throw new Error("No page target exposed by DevTools");
  }
  return target;
}

async function closeBrowser(child, profile, page) {
  await page?.call("Browser.close", {}, 2_000).catch(() => undefined);
  page?.close();
  if (child.exitCode == null) {
    child.kill("SIGTERM");
    await waitForExit(child, 3_000);
  }
  if (child.exitCode == null) {
    child.kill("SIGKILL");
    await waitForExit(child, 3_000);
  }
  await removeProfile(profile);
}

async function runSuite(page) {
  console.log("bureau-canvas e2e: config view");
  await withInstance("config", {}, {}, async (instance) => {
    await navigate(page, instance.opened.url);
    await renderAndScreenshot(page, ".assignment-card", "assignment cards", "config.png");
    await checkConfigView(page, instance.opened.url);
  });
  console.log("bureau-canvas e2e: committed pipeline by input");
  await withInstance("committed-open", { pipeline: "agent-eligible-pipeline" }, {}, async (instance) => {
    await navigate(page, instance.opened.url);
    await renderAndScreenshot(page, ".react-flow__node .flow-card", "committed pipeline", "committed-pipeline-open.png");
    await checkPipelineView(page, instance.opened.url, "agent-eligible-pipeline", "opened input");
  });
  console.log("bureau-canvas e2e: committed pipeline by intent");
  await withInstance("committed-intent", {}, {}, async (instance) => {
    await navigate(page, instance.opened.url);
    await postIntent(instance.opened.url, "agent-eligible-pipeline");
    await renderAndScreenshot(page, ".react-flow__node .flow-card", "intent pipeline", "committed-pipeline-intent.png");
    await checkPipelineView(page, instance.opened.url, "agent-eligible-pipeline", "open-pipeline intent");
  });
  console.log("bureau-canvas e2e: reference pipeline");
  const payload = JSON.parse(await readFile(REFERENCE_FIXTURE, "utf8"));
  await withInstance("reference", { pipeline: "fix-failing-test" }, { payload }, async (instance) => {
    await navigate(page, instance.opened.url);
    await renderAndScreenshot(page, ".react-flow__node .flow-card", "reference pipeline", "reference-pipeline.png");
    await checkReferenceState(instance.opened.url);
    await checkPipelineView(page, instance.opened.url, "fix-failing-test", "reference fixture");
  });
  console.log("bureau-canvas e2e: detail expansion");
  await checkDetailExpansion(page);
  console.log("bureau-canvas e2e: CRUD from empty");
  await runCrudSuite(page);
}

/**
 * The epic's acceptance criterion (#43), through the real UI: build a whole
 * config from an empty directory, then take it away again.
 */
async function runCrudSuite(page) {
  // `mkdtemp` needs the parent to exist, and `target/` is gitignored — so in a
  // fresh clone this whole suite failed on its first line, and only passed on a
  // tree that had already built something.
  await mkdir(CRUD_ROOT, { recursive: true });
  const dir = await mkdtemp(join(CRUD_ROOT, "e2e-"));
  await Promise.all(["roles", "assignments", "pipelines"].map((sub) => mkdir(join(dir, sub), { recursive: true })));
  try {
    // This flow needs the real CLI: the point is that what the UI builds is
    // accepted by `bureau validate`. The rest of the suite stays hermetic.
    await withInstance("crud", { dir }, { findingsOptions: {} }, async (instance) => {
      await navigate(page, instance.opened.url);
      await record("crud create action is present without an always-open form", async () => {
        await waitForRender(page, ".create-toolbar", "create action");
        const form = await evaluate(page, "document.querySelectorAll(\"[data-testid='create-bar']\").length");
        assert(form === 0, `expected collapsed create form, saw ${form}`);
      });
      await buildThroughUi(instance.opened.url);
      await navigate(page, instance.opened.url);
      await record("crud a pending plan is visible as a draft", async () => {
        const state = await fetchJson(new URL("/state", instance.opened.url));
        // Wait for the mount rather than sampling the DOM immediately, or this
        // measures render timing instead of the feature.
        await waitForRender(page, "[data-testid='draft-bar']", "draft bar");
        const draft = await evaluate(page, "document.querySelectorAll(\"[data-testid='draft-bar']\").length");
        assert(draft === 1, `expected a draft bar, saw ${draft}; plan=${JSON.stringify(state.plan)}`);
      });
      await postJson(instance.opened.url, { kind: "save-plan" });
      await record("crud the built config validates", async () => {
        const state = await fetchJson(new URL("/state", instance.opened.url));
        assert(state.validation.state === "validated", `expected validated, saw ${state.validation.state}`);
        assert(state.validation.errors.length === 0, `expected no errors, saw ${state.validation.errors.length}`);
      });
      await navigate(page, instance.opened.url);
      await renderAndScreenshot(page, ".assignment-card", "built config", "crud-built.png");
      await record("crud delete asks before acting and names referrers", async () => {
        const asked = await postJson(instance.opened.url, { kind: "delete", input: { dir, kind: "role", name: "implementer" } });
        assert(asked.result.confirmed === false, "delete acted without confirmation");
        assert(asked.result.referrers.length > 0, "delete reported no referrers for a referenced role");
      });
      await tearDownThroughUi(instance.opened.url, dir);
      await record("crud teardown leaves the directory empty", async () => {
        const left = await Promise.all(["roles", "assignments", "pipelines"].map((sub) => readdir(join(dir, sub))));
        assert(left.every((entries) => entries.length === 0), `expected empty config, saw ${JSON.stringify(left)}`);
      });
    });
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

async function buildThroughUi(url) {
  const steps = [
    { kind: "repo", name: "bureau", fields: { url: "https://x/y.git", forge: "github", access: "push", credential: "github-main" } },
    { kind: "role", name: "implementer", fields: { permissions: ["repo:read", "repo:write", "model:invoke"] } },
    { kind: "pipeline", name: "build", fields: {} },
    {
      kind: "assignment",
      name: "work",
      fields: { work: { forge: "github", source: "a/b", filter: "is:open" }, repos: ["bureau"], pipeline: "build", role: "implementer", verify: "cargo test --offline" },
    },
  ];
  for (const input of steps) {
    const response = await postJson(url, { kind: "create", input });
    assert(response.ok, `create ${input.kind} failed: ${response.error ?? "unknown"}`);
  }
}

async function tearDownThroughUi(url, dir) {
  for (const [kind, name] of [["assignment", "work"], ["pipeline", "build"], ["role", "implementer"], ["repo", "bureau"]]) {
    const response = await postJson(url, { kind: "delete", input: { dir, kind, name, confirm: true } });
    assert(response.ok, `delete ${kind} failed: ${response.error ?? "unknown"}`);
  }
  await postJson(url, { kind: "save-plan" });
}

async function postJson(url, body) {
  const capability = [...canvas.servers.values()].find((entry) => entry.url === url)?.capability;
  const response = await fetch(new URL("/intent", url), {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Bureau-Capability": capability },
    body: JSON.stringify(body),
  });
  return response.json();
}

async function fetchJson(url) {
  const response = await fetch(url);
  return response.json();
}

async function withInstance(name, input, options, fn) {
  const instanceId = `bureau-e2e-${name}-${Date.now()}`;
  const opened = await canvas.openBureauCanvas({ instanceId, input }, options);
  const instance = { opened, close: () => canvas.closeBureauCanvas({ instanceId }) };
  try {
    await fn(instance);
  } finally {
    await instance.close();
  }
}

async function navigate(page, url) {
  page.resetDiagnostics();
  const loaded = page.waitForEvent("Page.loadEventFired", 20_000);
  await page.call("Page.navigate", { url }, 10_000);
  await loaded;
}

async function waitForRender(page, selector, label) {
  const deadline = Date.now() + 45_000;
  while (Date.now() < deadline) {
    const ready = await evaluate(page, `Boolean(document.querySelector(${JSON.stringify(selector)}))`);
    if (ready) {
      await delay(500);
      return;
    }
    await delay(250);
  }
  throw new Error(`Timed out waiting for ${label}; diagnostics: ${formatDiagnostics(page.diagnostics)}`);
}

async function renderAndScreenshot(page, selector, label, fileName) {
  try {
    await waitForRender(page, selector, label);
  } finally {
    await screenshot(page, fileName);
  }
}

async function checkConfigView(page, url) {
  const state = await fetchState(url);
  await record("config renders without uncaught errors or console errors", () => assertNoDiagnostics(page));
  await record("the landing has one card per assignment", async () => {
    const counts = await evaluate(page, assignmentCountsExpression());
    assert.deepEqual(counts, {
      assignments: state.config.view.assignments.length,
    });
  });
  await record("assignment cards do not overlap", async () => assertNoOverlap(await boxes(page, ".assignment-card")));
  await record("the relation graph starts collapsed", async () => {
    assert.equal(await evaluate(page, `document.querySelector(".relation-section").open`), false);
  });
  await record("opening the relation graph shows a card per config item", async () => {
    await evaluate(page, `document.querySelector(".relation-section summary").click()`);
    await waitForRender(page, ".relation-section .relation-card", "relation graph cards");
    const counts = await evaluate(page, relationCountsExpression());
    assert.deepEqual(counts, {
      assignments: state.config.view.assignments.length,
      roles: state.config.view.roles.length,
      repos: state.config.view.repos.length,
      pipelines: state.config.view.pipelines.length,
    });
  });
  await record("the relation graph fits every card inside the surface", async () => {
    // The graph still pans and zooms; `fitView` must leave nothing clipped.
    assert.deepEqual(await evaluate(page, clippedCardsExpression()), []);
  });
  await record("the relation graph draws one edge per reference the config holds", async () => {
    // Against `relationView`'s own edges, not against what the renderer handed
    // React Flow. `data-graph-edges` is derived from the same projection the
    // surface draws, which makes it exactly the right barrier for "has the edge
    // pass happened" and no evidence at all for "are the right edges there": a
    // projection that dropped every edge would declare zero, draw zero, and
    // satisfy it. The pipeline graph has been checked against its layout since
    // the first version of this harness (see the edge-path count below); the
    // relation graph, which draws the assignment-first mental model this canvas
    // is built around, was only ever asked whether it had drawn *some* edge.
    await waitForRender(page, ".relation-section .react-flow__edge-path", "relation graph edges");
    const drawn = await evaluate(page, `document.querySelectorAll(".relation-section .react-flow__edge-path").length`);
    assert.equal(drawn, state.config.relation.edges.length);
  });
}

/** Assignment expansion exposes operational state, not scaffold-only fields. */
async function checkDetailExpansion(page) {
  const payload = JSON.parse(await readFile(COMMITTED_FIXTURE, "utf8"));
  await withInstance("detail", {}, { payload }, async (instance) => {
    await navigate(page, instance.opened.url);
    await waitForRender(page, ".assignment-card", "assignment cards");
    const before = await evaluate(page, assignmentDetailExpression());
    await record("the assignment card starts collapsed", () => {
      assert.deepEqual({ expanded: before.expanded, hasDetail: before.hasDetail }, { expanded: false, hasDetail: false });
    });
    await evaluate(page, `document.querySelector("[data-ref='assignment:agent-eligible'] .assignment-head").click()`);
    const after = await evaluate(page, assignmentDetailExpression());
    await record("clicking expands the assignment without scaffold-only role or verify rows", () => {
      assert.deepEqual(
        {
          expanded: after.expanded,
          hasDetail: after.hasDetail,
          hasPipeline: after.labels.includes("pipeline"),
          hasRole: after.labels.includes("role"),
          hasVerify: after.labels.includes("verify"),
        },
        { expanded: true, hasDetail: true, hasPipeline: true, hasRole: false, hasVerify: false },
      );
    });
  });
}

/** The collapsed/expanded state and visible field labels. */
function assignmentDetailExpression() {
  return `(() => {
    const card = document.querySelector("[data-ref='assignment:agent-eligible']");
    if (!card) { return { expanded: null }; }
    const head = card.querySelector(".assignment-head");
    const detail = card.querySelector(".assignment-detail");
    return {
      expanded: head?.getAttribute("aria-expanded") === "true",
      hasDetail: Boolean(detail),
      labels: [...(detail?.querySelectorAll(":scope > .detail-row > .detail-label") ?? [])]
        .map((label) => label.textContent.trim()),
    };
  })()`;
}

function assignmentCountsExpression() {
  return `({
    assignments: document.querySelectorAll('.assignment-card').length,
  })`;
}

function relationCountsExpression() {
  return `({
    assignments: document.querySelectorAll('.relation-section .relation-card--assignment').length,
    roles: document.querySelectorAll('.relation-section .relation-card--role').length,
    repos: document.querySelectorAll('.relation-section .relation-card--repo').length,
    pipelines: document.querySelectorAll('.relation-section .relation-card--pipeline').length,
  })`;
}

function clippedCardsExpression() {
  return `(() => {
    const surface = document.querySelector(".relation-section .config-flow");
    if (!surface) { return ["missing .config-flow"]; }
    const frame = surface.getBoundingClientRect();
    const slack = 1;
    return [...document.querySelectorAll(".relation-section .relation-card")]
      .filter((card) => {
        const box = card.getBoundingClientRect();
        return box.left < frame.left - slack || box.right > frame.right + slack
          || box.top < frame.top - slack || box.bottom > frame.bottom + slack;
      })
      .map((card) => card.getAttribute("data-ref"));
  })()`;
}

async function checkPipelineView(page, url, name, label) {
  const state = await fetchState(url);
  const pipeline = state.pipelines[name];
  await record(`pipeline ${label} renders without uncaught errors or console errors`, () => assertNoDiagnostics(page));
  await record(`pipeline ${label} has one node per step and terminal`, async () => {
    assert.equal(await evaluate(page, `document.querySelectorAll(".react-flow__node .flow-card").length`), pipeline.layout.steps.length + pipeline.layout.terminals.length);
  });
  await record(`pipeline ${label} has one SVG edge path per state edge`, async () => {
    assert.equal(await evaluate(page, `document.querySelectorAll(".react-flow__edge-path").length`), pipeline.layout.edges.length);
  });
  if (name === "agent-eligible-pipeline") {
    await record(`pipeline ${label} verify has exactly success and failure control edges`, () => {
      const outcomes = pipeline.layout.edges.filter((edge) => edge.source === "verify" && edge.relation === "control").map((edge) => edge.outcome).sort();
      assert.deepEqual(outcomes, ["failure", "success"]);
    });
  }
  await record(`pipeline ${label} step cards do not overlap`, async () => assertNoOverlap(await boxes(page, ".react-flow__node .flow-card:not(.terminal-pill)")));
  await record(`pipeline ${label} edge labels do not overlap`, async () => assertNoOverlap(await boxes(page, edgeLabelSelector())));
  await record(`pipeline ${label} legend colours match rendered edge colours`, async () => {
    assert.deepEqual(await evaluate(page, legendExpression()), []);
  });
  await record(`pipeline ${label} zoom controls and minimap exist`, async () => {
    assert.deepEqual(await evaluate(page, `({ controls: Boolean(document.querySelector(".react-flow__controls")), minimap: Boolean(document.querySelector(".react-flow__minimap")) })`), { controls: true, minimap: true });
  });
}

async function checkReferenceState(url) {
  const state = await fetchState(url);
  const pipeline = state.pipelines["fix-failing-test"];
  await record("reference fixture exercises nine steps, retry routes, concurrent group, and four-outcome decision", () => {
    const retries = pipeline.layout.edges.filter((edge) => edge.target === "propose" && ["passed", "verdict", "verify"].includes(edge.source)).map((edge) => `${edge.source}:${edge.route}`).sort();
    const passed = pipeline.layout.edges.filter((edge) => edge.source === "passed" && edge.relation === "control").map((edge) => edge.outcome).sort();
    const members = pipeline.layout.steps.filter((step) => step.parentId === "run-checks").map((step) => step.id).sort();
    assert.deepEqual({ steps: pipeline.layout.steps.length, retries, passed, members }, {
      steps: 9,
      retries: ["passed:back", "verdict:back", "verify:back"],
      passed: ["blocked", "failure", "no-work", "success"],
      members: ["apply", "review"],
    });
  });
}

async function postIntent(url, pipeline) {
  const capability = [...canvas.servers.values()].find((entry) => entry.url === url)?.capability;
  const response = await fetch(new URL("/intent", url), {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Bureau-Capability": capability },
    body: JSON.stringify({ kind: "open-pipeline", pipeline }),
  });
  if (!response.ok) {
    throw new Error(`open-pipeline intent failed with HTTP ${response.status}`);
  }
}

async function fetchState(url) {
  return fetch(new URL("/state", url), { signal: AbortSignal.timeout(5_000) }).then((response) => response.json());
}

async function screenshot(page, fileName) {
  const result = await page.call("Page.captureScreenshot", { format: "png", captureBeyondViewport: true }, 15_000);
  const path = join(SCREENSHOT_DIR, fileName);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, Buffer.from(result.data, "base64"));
  screenshots.push(path);
}

async function record(name, fn) {
  try {
    await fn();
    results.push({ name, ok: true });
  } catch (error) {
    results.push({ name, ok: false, error: error.message });
  }
}

async function evaluate(page, expression) {
  const result = await page.call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, 10_000);
  if (result.exceptionDetails) {
    throw new Error(exceptionText(result.exceptionDetails));
  }
  return result.result?.value;
}

function assertNoDiagnostics(page) {
  assert.deepEqual(page.diagnostics, [], formatDiagnostics(page.diagnostics));
}

function assertNoOverlap(items) {
  const overlaps = [];
  for (let left = 0; left < items.length; left += 1) {
    for (let right = left + 1; right < items.length; right += 1) {
      if (intersects(items[left], items[right])) {
        overlaps.push(`${items[left].label} overlaps ${items[right].label}`);
      }
    }
  }
  assert.deepEqual(overlaps, []);
}

function intersects(left, right) {
  return left.right > right.left && right.right > left.left && left.bottom > right.top && right.bottom > left.top;
}

async function boxes(page, selector) {
  return evaluate(page, `Array.from(document.querySelectorAll(${JSON.stringify(selector)})).map((element, index) => {
    const rect = element.getBoundingClientRect();
    return {
      label: element.dataset.ref || element.textContent.trim().replace(/\\s+/g, " ").slice(0, 80) || \`${selector}#\${index}\`,
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
    };
  }).filter((box) => box.right > box.left && box.bottom > box.top)`);
}

function overflowExpression() {
  return `Array.from(document.querySelectorAll("body *")).flatMap((element) => {
    const rect = element.getBoundingClientRect();
    const clientWidth = document.documentElement.clientWidth;
    if (rect.width <= 0 || rect.height <= 0 || rect.right <= clientWidth + 1) {
      return [];
    }
    return [{ tag: element.tagName.toLowerCase(), className: element.className?.toString?.() ?? "", right: Math.round(rect.right), clientWidth }];
  })`;
}

function edgeLabelSelector() {
  return ".react-flow__edge-text, .react-flow__edge-textwrapper, .react-flow__edge-label, .react-flow__edge-labels [class*='label']";
}

function legendExpression() {
  return `(() => {
    const edgeNames = new Map([["success", "success"], ["failure", "failure"], ["blocked", "blocked"], ["no-work", "no-work"], ["data", "inputs_from"], ["observes", "over"]]);
    const used = Array.from(document.querySelectorAll(".react-flow__edge")).map((edge) => {
      const key = Array.from(edge.classList).find((name) => name.startsWith("flow-edge--"))?.replace("flow-edge--", "");
      const path = edge.querySelector(".react-flow__edge-path");
      return key && path ? { key, label: edgeNames.get(key), color: getComputedStyle(path).stroke } : null;
    }).filter(Boolean);
    const legends = Object.fromEntries(Array.from(document.querySelectorAll(".legend-item")).map((item) => {
      const swatch = item.querySelector(".legend-swatch");
      const style = getComputedStyle(swatch);
      const text = item.textContent.trim();
      return [text, style.backgroundColor === "rgba(0, 0, 0, 0)" ? style.borderTopColor : style.backgroundColor];
    }));
    return Array.from(new Map(used.map((item) => [item.label, item.color]))).flatMap(([label, color]) => legends[label] === color ? [] : [{ label, edge: color, legend: legends[label] ?? null }]);
  })()`;
}

function printReport() {
  console.log("Bureau canvas e2e assertions:");
  for (const result of results) {
    console.log(`${result.ok ? "PASS" : "FAIL"} ${result.name}${result.error ? ` — ${result.error}` : ""}`);
  }
  console.log("Screenshots:");
  for (const path of screenshots) {
    console.log(`- ${path}`);
  }
}

function formatDiagnostics(diagnostics) {
  return diagnostics.map((item) => `${item.kind}: ${item.text}`).join("\n");
}

function consoleText(args = []) {
  return args.map((arg) => arg.value ?? arg.description ?? arg.unserializableValue ?? "").join(" ");
}

function exceptionText(details) {
  const exception = details?.exceptionDetails?.exception ?? details?.exception;
  return exception?.description ?? exception?.value ?? details?.text ?? "unknown exception";
}

async function messageText(data) {
  if (typeof data === "string") {
    return data;
  }
  if (data instanceof ArrayBuffer) {
    return Buffer.from(data).toString("utf8");
  }
  if (ArrayBuffer.isView(data)) {
    return Buffer.from(data.buffer, data.byteOffset, data.byteLength).toString("utf8");
  }
  return String(data);
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function onceExit(child) {
  return new Promise((resolve) => child.once("exit", resolve));
}

async function waitForExit(child, timeout) {
  if (child.exitCode != null) {
    return true;
  }
  return Promise.race([onceExit(child).then(() => true), delay(timeout).then(() => false)]);
}

async function removeProfile(profile) {
  let lastError = null;
  for (let attempt = 0; attempt < 20; attempt += 1) {
    try {
      await rm(profile, { recursive: true, force: true });
      return;
    } catch (error) {
      lastError = error;
      await delay(250);
    }
  }
  throw new Error(`could not remove Edge profile ${profile}: ${lastError?.message}`);
}

async function withTimeout(promise, ms, message) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

main().catch((error) => {
  console.error(`bureau-canvas e2e failed: ${error.stack ?? error.message}`);
  process.exitCode = 1;
});
