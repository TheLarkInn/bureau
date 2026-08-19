import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, isAbsolute, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { actions } from "./lib/actions.mjs";
import { applyPlan, create, crudActions, emptyPlan, remove as removeEntity, rename } from "./lib/crud.mjs";
import { findings } from "./lib/findings.mjs";
import { configLayout, pipelineContainers, pipelineHandles, pipelineLayout } from "./lib/layout.mjs";
import { arrangementFor, readLayout, savePipeline } from "./lib/pipeline.mjs";
import { createRunTail, listRuns, parseEvents, readRunEvents, runBureau, runsDir } from "./lib/runs.mjs";
import { configView, pipelineView, relationView } from "./lib/view.mjs";

const CANVAS_ID = "bureau";
const DISPLAY_NAME = "Bureau";
const DESCRIPTION = "Renders Bureau config assignments, roles, repos, and pipelines.";
const ENTRY_FILE = fileURLToPath(import.meta.url);
const EXTENSION_DIR = dirname(ENTRY_FILE);
const WEB_DIR = resolve(EXTENSION_DIR, "web");
const REPO_ROOT = resolve(EXTENSION_DIR, "../../..");
const FALLBACK_FIXTURE = resolve(EXTENSION_DIR, "test", "fixtures", "committed-payload.json");
const TEST_MISSING_BUREAU = resolve(EXTENSION_DIR, "test", "fixtures", "missing-bureau");

export const inputSchema = {
    type: "object",
    additionalProperties: false,
    properties: {
        dir: {
            type: "string",
            description: "Config directory, defaulting to .bureau in the repository root.",
        },
        pipeline: {
            type: "string",
            description: "Pipeline drill-down selected by the caller.",
        },
    },
};

export const canvasDeclaration = {
    id: CANVAS_ID,
    displayName: DISPLAY_NAME,
    description: DESCRIPTION,
    inputSchema,
};

export const servers = new Map();
const subjects = new Map();
const plans = new Map();

/**
 * Actions receive a relative `dir` default, so resolve it the same way
 * `resolveInput` does — against the repository root, never the process cwd.
 */
function actionDependencies() {
    return {
        getSubject: (instanceId) => subjects.get(instanceId),
        setSubject: (instanceId, subject) => subjects.set(instanceId, subject),
        getPlan: (instanceId) => plans.get(instanceId),
        setPlan: (instanceId, plan) => plans.set(instanceId, plan),
        clearPlan: (instanceId) => plans.delete(instanceId),
        loadFindings: (dir) => loadConfigPayload(configDir(dir), {}),
        publish: (instanceId, event, payload) => publishEvent(instanceId, event, payload),
    };
}

function configDir(dir) {
    return isAbsolute(dir) ? dir : resolve(REPO_ROOT, dir);
}

async function publishEvent(instanceId, event, payload) {
    const entry = servers.get(instanceId);
    if (!entry) {
        return;
    }
    if (event === "state" && payload?.subject) {
        entry.state = await buildState({ ...entry.state, ...resolvedSubject(payload.subject), instanceId }, {});
        publishState(entry);
        return;
    }
    for (const client of entry.clients) {
        client.write(`event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`);
    }
}

function resolvedSubject(subject) {
    const dir = subject.dir ?? ".bureau";
    return {
        dir: isAbsolute(dir) ? dir : resolve(REPO_ROOT, dir),
        pipeline: subject.pipeline ?? null,
    };
}

export function canvasActions(deps = actionDependencies()) {
    return [...actions, ...crudActions].map((action) => ({
        name: action.name,
        description: action.description,
        inputSchema: action.inputSchema,
        handler: (ctx) => action.handler(ctx, deps),
    }));
}

export function resolveInput(input = {}) {
    const dir = input.dir ?? ".bureau";

    return {
        canvasId: CANVAS_ID,
        instanceId: "",
        repoRoot: REPO_ROOT,
        dir: isAbsolute(dir) ? dir : resolve(REPO_ROOT, dir),
        pipeline: input.pipeline ?? null,
        message: "Bureau config view.",
    };
}

export async function openBureauCanvas(ctx, options = {}) {
    const input = resolveInput(ctx.input ?? {});
    const state = await buildState({ ...input, instanceId: ctx.instanceId }, options);
    subjects.set(ctx.instanceId, subjectFromState(state));
    let entry = servers.get(ctx.instanceId);

    if (!entry) {
        entry = await startServer(state, options);
        servers.set(ctx.instanceId, entry);
    } else {
        entry.state = state;
        publishState(entry);
    }
    // Kept so a later refresh rebuilds state the same way this open did.
    entry.options = options;

    return { title: DISPLAY_NAME, status: state.status, url: entry.url };
}

export async function closeBureauCanvas(ctx) {
    const entry = servers.get(ctx.instanceId);
    if (!entry) {
        return;
    }

    servers.delete(ctx.instanceId);
    subjects.delete(ctx.instanceId);
    await closeServer(entry);
}

export function createBureauCanvasOptions(getWorkspacePath = () => process.cwd()) {
    return {
        ...canvasDeclaration,
        actions: canvasActions(),
        open: (ctx) => openBureauCanvas(ctx, { workspacePath: getWorkspacePath() }),
        onClose: closeBureauCanvas,
    };
}

export async function buildState(input, options = {}) {
    const result = await loadConfigPayload(input.dir, options);
    const payload = payloadFromResult(result);
    const view = configView(payload);
    const layouts = await loadLayoutSidecar(input.dir, options);
    const pipelines = pipelineStates(payload, result.config, layouts);
    const state = {
        ...input,
        status: statusFor(result),
        validation: validationState(result),
        findings: result.findings ?? [],
        findingsByItem: findingsByItem(result.findings ?? []),
        findingsByStep: findingsByStep(result.findings ?? []),
        generalFindings: generalFindings(result.findings ?? []),
        config: { view, layout: configLayout(view), relation: relationView(payload) },
        pipelines,
        plan: planSummary(input.instanceId),
    };
    return { ...state, selectedPipeline: selectedPipeline(state, input.pipeline) };
}

/** The editor's node positions; a missing or unreadable sidecar means none. */
async function loadLayoutSidecar(dir, options) {
    if (options.layouts) {
        return options.layouts;
    }
    return readLayout(dir);
}

async function loadConfigPayload(dir, options) {
    if (options.payload) {
        return resultFromPayload(options.payload, dir);
    }
    const result = await findings(dir, findingsOptions(options));
    if (["binary-missing", "dir-missing"].includes(result.state)) {
        return fallbackResult(dir, result);
    }
    return result;
}

function resultFromPayload(payload, fallbackDir) {
    return {
        ok: Boolean(payload.ok),
        state: "validated",
        dir: payload.dir ?? fallbackDir,
        errors: payload.errors ?? [],
        config: payload.config ?? null,
        findings: payload.findings ?? [],
    };
}

function findingsOptions(options) {
    if (options.findingsOptions) {
        return options.findingsOptions;
    }
    return process.env.BUREAU_CANVAS_TEST === "1" ? { binary: TEST_MISSING_BUREAU } : {};
}

async function fallbackResult(dir, reason) {
    const text = await readFile(FALLBACK_FIXTURE, "utf8");
    const payload = JSON.parse(text);
    return {
        ok: true,
        state: "fixture",
        dir,
        errors: [],
        config: payload.config,
        findings: payload.findings ?? [],
        fixtureReason: reason.state,
        message: fallbackMessage(reason),
    };
}

function fallbackMessage(reason) {
    const detail = reason.state === "binary-missing" ? "bureau binary not available" : "config directory not found";
    return `Showing bundled sample; ${detail}.`;
}

function payloadFromResult(result) {
    return {
        ok: result.ok,
        dir: result.dir,
        errors: result.errors ?? [],
        config: result.config ?? null,
        findings: result.findings ?? [],
    };
}

function pipelineStates(payload, config, layouts = {}) {
    const names = Object.keys(config?.pipelines ?? {}).sort();
    return Object.fromEntries(names.map((name) => [name, pipelineState(payload, config, name, layouts)]));
}

function pipelineState(payload, config, name, layouts = {}) {
    const view = pipelineView(payload, name);
    const layout = pipelineLayout(view);
    return {
        view,
        layout,
        handles: pipelineHandles(layout),
        containers: pipelineContainers(layout),
        summary: pipelineSummary(view, config),
        arrangement: arrangementFor(layouts, name),
    };
}

function pipelineSummary(view, config) {
    return {
        kindCounts: counts(view.steps.map((step) => step.kind)),
        agentSteps: view.steps.filter((step) => step.kind === "agent").map((step) => agentStepSummary(view.name, step, config)),
    };
}

function agentStepSummary(pipeline, step, config) {
    const role = config?.roles?.[step.fields.role] ?? {};
    return {
        name: step.name,
        role: step.fields.role,
        trust: step.fields.trust ?? role.min_trust ?? null,
        ref: `pipeline:${pipeline}/${step.name}`,
    };
}

function counts(values) {
    const result = {};
    for (const value of values) {
        result[value] = (result[value] ?? 0) + 1;
    }
    return result;
}

function statusFor(result) {
    if (result.state === "fixture") {
        return result.message;
    }
    if (result.state === "validated") {
        return result.ok ? "Validated" : "Validation findings";
    }
    return "Config unavailable";
}

function validationState(result) {
    return {
        ok: result.ok,
        state: result.state,
        dir: result.dir,
        errors: result.errors ?? [],
        message: result.message ?? null,
    };
}

function findingsByItem(findingsList) {
    return groupFindings(findingsList, itemKeyForFinding);
}

function findingsByStep(findingsList) {
    return groupFindings(findingsList, stepKeyForFinding);
}

function groupFindings(findingsList, keyFor) {
    const grouped = {};
    for (const finding of findingsList) {
        const key = keyFor(finding);
        if (key) {
            grouped[key] = [...(grouped[key] ?? []), finding];
        }
    }
    return grouped;
}

function itemKeyForFinding(finding) {
    const target = finding.target ?? {};
    if (target.kind === "step") {
        return `pipeline:${target.pipeline}`;
    }
    if (["assignment", "role", "pipeline", "repo"].includes(target.kind)) {
        return `${target.kind}:${target[target.kind]}`;
    }
    return null;
}

function stepKeyForFinding(finding) {
    const target = finding.target ?? {};
    return target.kind === "step" ? `pipeline:${target.pipeline}/${target.step}` : null;
}

function generalFindings(findingsList) {
    return findingsList.filter((finding) => !itemKeyForFinding(finding));
}

function subjectFromState(state) {
    return {
        dir: state.dir,
        ...(state.pipeline ? { pipeline: state.pipeline } : {}),
    };
}

function selectedPipeline(state, name) {
    if (!name) {
        return null;
    }
    const missing = !state.pipelines[name];
    return missing ? { name, missing, notice: `No pipeline named \`${name}\` in this config.` } : { name, missing };
}

async function startServer(state, options = {}) {
    const entry = { clients: new Set(), runTail: undefined, server: undefined, state, url: "" };
    const server = createServer((request, response) => {
        void handleRequest(entry, request, response).catch(() => sendStatus(response, 500));
    });

    entry.server = server;
    entry.runTail = createRunTail({
        dir: options.runsDir ?? runsDir(),
        publish: (payload) => publishRunEvent(entry, payload),
        ...(typeof options.runTailIntervalMs === "number" ? { intervalMs: options.runTailIntervalMs } : {}),
    });
    entry.runTail.start();
    await listen(server);
    const address = server.address();
    if (!address || typeof address === "string") {
        throw new Error("loopback server address unavailable");
    }

    entry.url = `http://127.0.0.1:${address.port}/`;
    return entry;
}

function listen(server) {
    return new Promise((resolveListen, rejectListen) => {
        server.once("error", rejectListen);
        server.listen(0, "127.0.0.1", () => {
            server.off("error", rejectListen);
            resolveListen();
        });
    });
}

async function closeServer(entry) {
    entry.runTail?.stop();
    for (const client of entry.clients) {
        client.end();
    }

    await new Promise((resolveClose) => entry.server.close(resolveClose));
}

async function handleRequest(entry, request, response) {
    const pathname = requestPath(request);
    if (request.method === "POST" && pathname === "/intent") {
        await handleIntent(entry, request, response);
        return;
    }
    if (request.method !== "GET" && request.method !== "HEAD") {
        sendStatus(response, 405);
        return;
    }

    if (pathname === "/state") {
        sendJson(response, entry.state, request.method === "HEAD");
    } else if (pathname === "/events") {
        sendEvents(entry, request, response);
    } else if (pathname === "/runs") {
        sendJson(response, { runs: await listRuns(runsRoot(entry)) }, request.method === "HEAD");
    } else if (pathname.startsWith("/runs/") && pathname.endsWith("/events")) {
        const runId = pathname.slice("/runs/".length, -"/events".length);
        await sendRunEvents(runId, entry, response, request.method === "HEAD");
    } else {
        await sendStatic(pathname, response, request.method === "HEAD");
    }
}

/** The runs root this server observes; overridable for tests. */
function runsRoot(entry) {
    return entry.options?.runsDir ?? runsDir();
}

/** One run's full event log: the CLI's replay when a binary is on hand, the raw log otherwise. */
async function sendRunEvents(runId, entry, response, headOnly) {
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(runId)) {
        sendStatus(response, 400);
        return;
    }
    const dir = runsRoot(entry);
    const run = await runBureau(["show", runId, "--events", "--json", "--runs", dir], entry.options ?? {});
    if (run === null) {
        await sendRunEventsFromLog(runId, dir, response, headOnly);
        return;
    }
    if (run.code !== 0) {
        if (lacksEventsFlag(run)) {
            await sendRunEventsFromLog(runId, dir, response, headOnly);
            return;
        }
        sendStatus(response, 404);
        return;
    }
    sendJson(response, { run_id: runId, events: parseJson(run.stdout) ?? [] }, headOnly);
}

/** A `bureau` on PATH can predate `show --events --json`; read the log directly instead of failing. */
function lacksEventsFlag(run) {
    return /unexpected argument/u.test(`${run.stderr ?? ""}${run.stdout ?? ""}`);
}

async function sendRunEventsFromLog(runId, dir, response, headOnly) {
    const events = await readRunEvents(dir, runId);
    if (!events) {
        sendStatus(response, 404);
        return;
    }
    sendJson(response, { run_id: runId, events, source: "log" }, headOnly);
}

const RUN_CONTROL_VERBS = { "pause-run": "pause", "resume-run": "resume", "cancel-run": "cancel" };

/** Pause, resume, and cancel are the CLI's; the canvas never writes run markers itself. */
async function runControlIntent(entry, intent, response) {
    if (typeof intent.run_id !== "string" || intent.run_id.length === 0) {
        sendStatus(response, 400);
        return;
    }
    const verb = RUN_CONTROL_VERBS[intent.kind];
    const dir = runsRoot(entry);
    const run = await runBureau([verb, intent.run_id, "--runs", dir], entry.options ?? {});
    if (run === null) {
        sendJson(response, { ok: false, error: "bureau binary not available" }, false);
        return;
    }
    sendJson(response, { ok: run.code === 0, exit_code: run.code, output: `${run.stdout}${run.stderr}`.trim() }, false);
}

const CRUD_INTENTS = { create, delete: removeEntity, rename };

async function handleIntent(entry, request, response) {
    const intent = await readIntent(request);
    if (CRUD_INTENTS[intent?.kind]) {
        await runCrudIntent(entry, intent, response);
        return;
    }
    if (intent?.kind === "save-plan" || intent?.kind === "discard-plan") {
        await runPlanIntent(entry, intent, response);
        return;
    }
    if (intent?.kind === "save-pipeline") {
        await runSavePipelineIntent(entry, intent, response);
        return;
    }
    if (RUN_CONTROL_VERBS[intent?.kind]) {
        await runControlIntent(entry, intent, response);
        return;
    }
    if (intent?.kind === "back-to-config") {
        entry.state = { ...entry.state, pipeline: null, selectedPipeline: null };
        subjects.set(entry.state.instanceId, subjectFromState(entry.state));
        publishState(entry);
        sendJson(response, { ok: true, state: entry.state }, false);
        return;
    }
    if (intent?.kind !== "open-pipeline" || typeof intent.pipeline !== "string") {
        sendStatus(response, 400);
        return;
    }
    entry.state = { ...entry.state, pipeline: intent.pipeline };
    entry.state.selectedPipeline = selectedPipeline(entry.state, intent.pipeline);
    subjects.set(entry.state.instanceId, subjectFromState(entry.state));
    publishState(entry);
    sendJson(response, { ok: true, state: entry.state }, false);
}

function readIntent(request) {
    return new Promise((resolveRead) => {
        let body = "";
        request.setEncoding("utf8");
        request.on("data", (chunk) => {
            body += chunk;
        });
        request.on("end", () => resolveRead(parseJson(body)));
    });
}

function parseJson(text) {
    try {
        return JSON.parse(text);
    } catch {
        return null;
    }
}

function requestPath(request) {
    return new URL(request.url ?? "/", "http://127.0.0.1").pathname;
}

/** Runs one CRUD verb and republishes state so a pending plan is visible. */
async function runCrudIntent(entry, intent, response) {
    const deps = actionDependencies();
    const ctx = { instanceId: entry.state.instanceId, input: { dir: entry.state.dir, ...intent.input } };
    try {
        const result = await CRUD_INTENTS[intent.kind](ctx, deps);
        await refreshState(entry);
        sendJson(response, { ok: true, result, state: entry.state }, false);
    } catch (error) {
        sendJson(response, { ok: false, error: String(error?.message ?? error), state: entry.state }, false);
    }
}

/**
 * The editor's save. `savePipeline` owns the round-trip guarantee: findings
 * that name the edited pipeline revert the file, so state only refreshes on
 * a clean save and the findings come back for the UI to mark.
 */
async function runSavePipelineIntent(entry, intent, response) {
    try {
        const dir = entry.state.dir;
        const result = await savePipeline(
            { dir, pipeline: intent.pipeline, view: intent.view, layout: intent.layout ?? null },
            savePipelineOptions(entry),
        );
        if (result.saved) {
            await refreshState(entry);
        }
        sendJson(response, { ok: result.saved, findings: result.findings, path: result.path, state: entry.state }, false);
    } catch (error) {
        sendJson(response, { ok: false, error: String(error?.message ?? error), state: entry.state }, false);
    }
}

function savePipelineOptions(entry) {
    return entry.options?.savePipelineDeps ?? {};
}

async function runPlanIntent(entry, intent, response) {
    const instance = entry.state.instanceId;
    try {
        if (intent.kind === "save-plan") {
            const plan = plans.get(instance) ?? emptyPlan();
            await applyPlan(configDir(entry.state.dir), plan, {});
        }
        plans.delete(instance);
        await refreshState(entry);
        sendJson(response, { ok: true, state: entry.state }, false);
    } catch (error) {
        sendJson(response, { ok: false, error: String(error?.message ?? error), state: entry.state }, false);
    }
}

async function refreshState(entry) {
    const input = { dir: entry.state.dir, pipeline: entry.state.pipeline ?? undefined };
    entry.state = await buildState({ ...input, instanceId: entry.state.instanceId }, entry.options ?? {});
    publishState(entry);
}

/** Pending, unsaved work, so the panel can show a draft rather than look saved. */
function planSummary(instanceId) {
    const plan = plans.get(instanceId);
    if (!plan || (plan.writes.length === 0 && plan.removals.length === 0)) {
        return null;
    }
    return {
        writes: plan.writes.map((write) => write.path),
        removals: plan.removals.map((entry) => entry.path),
    };
}

function sendJson(response, value, headOnly) {
    response.writeHead(200, {
        "Cache-Control": "no-store",
        "Content-Type": "application/json; charset=utf-8",
    });
    response.end(headOnly ? undefined : `${JSON.stringify(value)}\n`);
}

function sendEvents(entry, request, response) {
    response.writeHead(200, {
        "Cache-Control": "no-store",
        "Connection": "keep-alive",
        "Content-Type": "text/event-stream; charset=utf-8",
    });
    entry.clients.add(response);
    writeStateEvent(response, entry.state);
    request.on("close", () => entry.clients.delete(response));
}

function publishState(entry) {
    for (const client of entry.clients) {
        writeStateEvent(client, entry.state);
    }
}

/** Forwards one appended run-log event to every SSE client. */
function publishRunEvent(entry, payload) {
    for (const client of entry.clients) {
        client.write(`event: run-event\ndata: ${JSON.stringify(payload)}\n\n`);
    }
}

function writeStateEvent(response, state) {
    response.write(`event: state\ndata: ${JSON.stringify(state)}\n\n`);
}

async function sendStatic(pathname, response, headOnly) {
    const filePath = staticPath(pathname);
    if (!filePath) {
        sendStatus(response, 404);
        return;
    }

    const info = await stat(filePath).catch(() => undefined);
    if (!info?.isFile()) {
        sendStatus(response, 404);
        return;
    }

    response.writeHead(200, { "Content-Type": contentType(filePath) });
    if (headOnly) {
        response.end();
    } else {
        createReadStream(filePath).pipe(response);
    }
}

function staticPath(pathname) {
    let decoded;
    try {
        decoded = decodeURIComponent(pathname === "/" ? "/index.html" : pathname);
    } catch {
        return undefined;
    }

    const filePath = resolve(WEB_DIR, decoded.replace(/^\/+/, ""));
    return filePath === WEB_DIR || filePath.startsWith(`${WEB_DIR}${sep}`) ? filePath : undefined;
}

function contentType(filePath) {
    const types = new Map([
        [".html", "text/html; charset=utf-8"],
        [".mjs", "text/javascript; charset=utf-8"],
        [".js", "text/javascript; charset=utf-8"],
        [".css", "text/css; charset=utf-8"],
    ]);
    return types.get(extname(filePath)) ?? "application/octet-stream";
}

function sendStatus(response, status) {
    if (!response.headersSent) {
        response.writeHead(status);
    }
    response.end();
}

async function register() {
    const { createCanvas, joinSession } = await import("@github/copilot-sdk/extension");
    let workspacePath = process.cwd();
    const options = createBureauCanvasOptions(() => workspacePath);
    const session = await joinSession({ canvases: [createCanvas(options)] });
    workspacePath = session.workspacePath ?? workspacePath;
}

// `BUREAU_CANVAS_TEST` makes the run hermetic (no real `bureau` binary), which
// tests want but a standalone server does not. `BUREAU_CANVAS_NO_SDK` only
// skips registration, so `serve.mjs` reads real config.
if (process.env.BUREAU_CANVAS_TEST !== "1" && process.env.BUREAU_CANVAS_NO_SDK !== "1") {
    await register();
}