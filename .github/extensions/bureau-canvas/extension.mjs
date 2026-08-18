import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, isAbsolute, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { actions } from "./lib/actions.mjs";
import { findings } from "./lib/findings.mjs";
import { configLayout, pipelineLayout } from "./lib/layout.mjs";
import { configView, pipelineView } from "./lib/view.mjs";

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

/**
 * Actions receive a relative `dir` default, so resolve it the same way
 * `resolveInput` does — against the repository root, never the process cwd.
 */
function actionDependencies() {
    return {
        getSubject: (instanceId) => subjects.get(instanceId),
        setSubject: (instanceId, subject) => subjects.set(instanceId, subject),
        loadFindings: (dir) => loadConfigPayload(isAbsolute(dir) ? dir : resolve(REPO_ROOT, dir), {}),
        publish: (instanceId, event, payload) => publishEvent(instanceId, event, payload),
    };
}

function publishEvent(instanceId, event, payload) {
    const entry = servers.get(instanceId);
    if (!entry) {
        return;
    }
    for (const client of entry.clients) {
        client.write(`event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`);
    }
}

export function canvasActions(deps = actionDependencies()) {
    return actions.map((action) => ({
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
        entry = await startServer(state);
        servers.set(ctx.instanceId, entry);
    } else {
        entry.state = state;
        publishState(entry);
    }

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
    const pipelines = pipelineStates(payload, result.config);
    const state = {
        ...input,
        status: statusFor(result),
        validation: validationState(result),
        findings: result.findings ?? [],
        findingsByItem: findingsByItem(result.findings ?? []),
        findingsByStep: findingsByStep(result.findings ?? []),
        generalFindings: generalFindings(result.findings ?? []),
        config: { view, layout: configLayout(view) },
        pipelines,
    };
    return { ...state, selectedPipeline: selectedPipeline(state, input.pipeline) };
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

function pipelineStates(payload, config) {
    const names = Object.keys(config?.pipelines ?? {}).sort();
    return Object.fromEntries(names.map((name) => [name, pipelineState(payload, config, name)]));
}

function pipelineState(payload, config, name) {
    const view = pipelineView(payload, name);
    return { view, layout: pipelineLayout(view), summary: pipelineSummary(view, config) };
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

async function startServer(state) {
    const entry = { clients: new Set(), server: undefined, state, url: "" };
    const server = createServer((request, response) => {
        void handleRequest(entry, request, response).catch(() => sendStatus(response, 500));
    });

    entry.server = server;
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
    } else {
        await sendStatic(pathname, response, request.method === "HEAD");
    }
}

async function handleIntent(entry, request, response) {
    const intent = await readIntent(request);
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

if (process.env.BUREAU_CANVAS_TEST !== "1") {
    await register();
}