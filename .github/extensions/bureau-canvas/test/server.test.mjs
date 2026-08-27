import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";

const canvas = await import("../extension.mjs");

function readChunk(reader) {
    return new Promise((resolveRead, rejectRead) => {
        const timer = setTimeout(() => rejectRead(new Error("timed out waiting for dashboard reload")), 5000);
        reader.read().then(
            (result) => {
                clearTimeout(timer);
                resolveRead(result);
            },
            (error) => {
                clearTimeout(timer);
                rejectRead(error);
            },
        );
    });
}

async function readEvent(reader, name) {
    let text = "";
    while (!text.includes(`event: ${name}`)) {
        const { value, done } = await readChunk(reader);
        if (done) {
            break;
        }
        text += new TextDecoder().decode(value);
    }
    return text;
}

test("declares the Bureau canvas", () => {
    assert.deepStrictEqual(canvas.canvasDeclaration, {
        id: "bureau",
        displayName: "Bureau",
        description: "Renders Bureau config assignments, roles, repos, and pipelines.",
        inputSchema: canvas.inputSchema,
    });
});

test("resolves default input against the workspace", () => {
    assert.equal(canvas.resolveInput({}, process.cwd()).dir, resolve(process.cwd(), ".bureau"));
});

test("serves config page and state", async () => {
    const instanceId = "bureau-server-test";
    const opened = await canvas.openBureauCanvas({ instanceId, input: { pipeline: "smoke" } });

    try {
        const page = await fetch(opened.url).then((response) => response.text());
        const state = await fetch(new URL("/state", opened.url)).then((response) => response.json());
        assert.deepStrictEqual(
            [
                page.includes("Bureau"),
                state.instanceId,
                state.pipeline,
                (await fetch(opened.url)).headers.get("x-frame-options"),
            ],
            [true, instanceId, "smoke", "SAMEORIGIN"],
        );
    } finally {
        await canvas.closeBureauCanvas({ instanceId });
    }
});

/**
 * `/sample` is what the State Lab starts from, and the claim it makes is that
 * the payload is the bundled one rather than whatever config the host was
 * opened on. Asserted against the *committed fixture's* own assignment names,
 * not a literal list here, so the two cannot drift: the lab's fixtures reach
 * into this payload by name, and a sample that stopped carrying them would take
 * a fifth of the matrix down with it.
 *
 * The comparison is with `/state` from the same host, because "it served
 * something" is not the claim — "it served the sample even though the host has
 * its own config" is. Under `BUREAU_CANVAS_TEST=1` the binary is missing, so
 * `/state` already falls back to the same payload; what this pins is that
 * `/sample` reaches it by its own route rather than by that coincidence.
 */
test("serves the bundled sample the state lab starts from", async () => {
    const instanceId = "bureau-sample-test";
    const opened = await canvas.openBureauCanvas({ instanceId, input: {} });
    try {
        const sample = await fetch(new URL("/sample", opened.url)).then((response) => response.json());
        const fixture = JSON.parse(await readFile(new URL("./fixtures/committed-payload.json", import.meta.url), "utf8"));
        const named = (config) => Object.keys(config?.assignments ?? {}).sort();
        assert.deepStrictEqual(
            {
                assignments: sample.config.view.assignments.map((item) => item.name).sort(),
                state: sample.validation.state,
                instanceId: sample.instanceId,
            },
            { assignments: named(fixture.config), state: "fixture", instanceId },
        );
    } finally {
        await canvas.closeBureauCanvas({ instanceId });
    }
});

test("Copilot canvas host remains frameable", async () => {    const instanceId = "bureau-embedded-frame-test";
    const options = canvas.createBureauCanvasOptions(() => process.cwd());
    const opened = await options.open({ instanceId, input: {} });
    try {
        const response = await fetch(opened.url);
        assert.deepStrictEqual(
            [response.headers.get("x-frame-options"), response.headers.get("content-security-policy")],
            [null, null],
        );
    } finally {
        await options.onClose({ instanceId });
    }
});

test("reuses the server for the same instance", async () => {
    const instanceId = "bureau-idempotent-test";
    const first = await canvas.openBureauCanvas({ instanceId, input: {} });
    const second = await canvas.openBureauCanvas({ instanceId, input: { pipeline: "next" } });

    try {
        const state = await fetch(new URL("/state", first.url)).then((response) => response.json());
        assert.deepStrictEqual([second.url, state.pipeline], [first.url, "next"]);
    } finally {
        await canvas.closeBureauCanvas({ instanceId });
    }
});

test("streams a state event", async () => {
    const instanceId = "bureau-events-test";
    const opened = await canvas.openBureauCanvas({ instanceId, input: {} });
    const response = await fetch(new URL("/events", opened.url));

    try {
        const { value } = await response.body.getReader().read();
        const event = new TextDecoder().decode(value);
        assert.match(event, /event: state\ndata: .*"instanceId":"bureau-events-test"/);
    } finally {
        await canvas.closeBureauCanvas({ instanceId });
    }
});

test("mutation endpoint requires the page capability and same origin", async () => {
    const instanceId = "bureau-server-security-test";
    const opened = await canvas.openBureauCanvas({ instanceId, input: {} });
    const capability = canvas.servers.get(instanceId).capability;
    const intent = {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: '{"kind":"back-to-config"}',
    };
    try {
        const missing = await fetch(new URL("/intent", opened.url), intent);
        const hostile = await fetch(new URL("/intent", opened.url), {
            ...intent,
            headers: {
                ...intent.headers,
                "X-Bureau-Capability": capability,
                Origin: "https://example.invalid",
            },
        });
        const wrongType = await fetch(new URL("/intent", opened.url), {
            ...intent,
            headers: {
                "Content-Type": "text/plain",
                "X-Bureau-Capability": capability,
            },
        });
        const allowed = await fetch(new URL("/intent", opened.url), {
            ...intent,
            headers: {
                ...intent.headers,
                "X-Bureau-Capability": capability,
                Origin: new URL(opened.url).origin,
            },
        });
        assert.deepStrictEqual(
            [missing.status, hostile.status, wrongType.status, allowed.status],
            [403, 403, 403, 200],
        );
    } finally {
        await canvas.closeBureauCanvas({ instanceId });
    }
});

test("development host reloads open pages after web changes", async (t) => {
    const watchDir = await mkdtemp(join(tmpdir(), "bureau-dashboard-reload-"));
    const instanceId = "bureau-development-reload-test";
    const opened = await canvas.openBureauCanvas(
        { instanceId, input: {} },
        { dev: true, devIntervalMs: 20, watchDir },
    );
    t.after(() => rm(watchDir, { recursive: true, force: true }));
    const pageResponse = await fetch(opened.url);
    const page = await pageResponse.text();
    const eventsResponse = await fetch(new URL("/events", opened.url));
    const reader = eventsResponse.body.getReader();
    await readEvent(reader, "state");
    await writeFile(join(watchDir, "change.css"), "body {}\n");
    const reload = await readEvent(reader, "reload");
    await reader.cancel();
    await canvas.closeBureauCanvas({ instanceId });
    assert.deepStrictEqual(
        [page.includes("/dev-reload.mjs"), pageResponse.headers.get("cache-control"), reload.includes("event: reload")],
        [true, "no-store", true],
    );
});

test("uses app theme tokens in the config page", async () => {
    const html = await readFile(new URL("../web/index.html", import.meta.url), "utf8");
    assert.deepStrictEqual(
        ["--background-color-default", "--border-color-default", "--text-color-default", "--font-sans"].map((token) =>
            html.includes(token),
        ),
        [true, true, true, true],
    );
});
