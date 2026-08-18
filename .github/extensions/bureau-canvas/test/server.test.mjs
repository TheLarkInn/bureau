import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { test } from "node:test";

process.env.BUREAU_CANVAS_TEST = "1";

const canvas = await import("../extension.mjs");

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
        assert.deepStrictEqual([page.includes("Bureau"), state.instanceId, state.pipeline], [true, instanceId, "smoke"]);
    } finally {
        await canvas.closeBureauCanvas({ instanceId });
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

test("uses app theme tokens in the config page", async () => {
    const html = await readFile(new URL("../web/index.html", import.meta.url), "utf8");
    assert.deepStrictEqual(
        ["--background-color-default", "--border-color-default", "--text-color-default", "--font-sans"].map((token) =>
            html.includes(token),
        ),
        [true, true, true, true],
    );
});
