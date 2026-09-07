import { cp, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, RUN_ID, test } from "../fixtures.mjs";
import { applyOps, enterState, pageAdapter, test as overviewTest } from "../matrix-fixtures.mjs";
import { initialGraphViewport } from "../../../web/graph-presentation.mjs";
import { applyFixture } from "../../../web/statelab/fixtures.mjs";
import { STATES, TRANSITIONS } from "../../../web/statelab/registry.mjs";

async function openViewer(page, canvas) {
  await page.goto(canvas.url);
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
  await expect(page.getByRole("tab", { name: "transitions", exact: true })).toHaveAttribute("aria-selected", "true");
}

async function openGraph(page, canvas) {
  await openViewer(page, canvas);
  await page.getByRole("tab", { name: "graph", exact: true }).click();
  await expect(page.locator(".pipeline-flow .react-flow__node-stepCard")).toHaveCount(3);
  return page.locator(".pipeline-flow");
}

const viewerCard = (page, name) => page.locator(`.pipeline-flow .react-flow__node[data-id="${name}"] .flow-card`);
const transform = (graph) => graph.locator(".react-flow__viewport")
  .evaluate((element) => getComputedStyle(element).transform);
const scale = (graph) => graph.locator(".react-flow__viewport")
  .evaluate((element) => new DOMMatrixReadOnly(getComputedStyle(element).transform).a);
const connections = (graph) => graph.locator(".react-flow__edge")
  .evaluateAll((edges) => edges.map((edge) => edge.getAttribute("data-id")).sort());

async function expectCentered(graph, node) {
  await expect.poll(async () => {
    const surface = await graph.boundingBox();
    const card = await node.boundingBox();
    return Math.abs(card.x + card.width / 2 - surface.x - surface.width / 2);
  }).toBeLessThan(3);
}

async function expectReachable(control) {
  await control.scrollIntoViewIfNeeded();
  await expect(control).toBeInViewport();
  await control.click({ trial: true });
  expect(await control.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
    return box.left >= 0 && box.right <= innerWidth && (hit === element || element.contains(hit));
  })).toBe(true);
}

async function nextPaint(graph) {
  await graph.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

async function expectAllNodesInside(graph) {
  const surface = await graph.boundingBox();
  for (const node of await graph.locator(".react-flow__node").all()) {
    const box = await node.boundingBox();
    expect(box.x).toBeGreaterThanOrEqual(surface.x - 1);
    expect(box.y).toBeGreaterThanOrEqual(surface.y - 1);
    expect(box.x + box.width).toBeLessThanOrEqual(surface.x + surface.width + 1);
    expect(box.y + box.height).toBeLessThanOrEqual(surface.y + surface.height + 1);
  }
}

async function measuredFrame(graph) {
  return graph.evaluate((element) => {
    const surface = element.querySelector(".react-flow") ?? element;
    const box = surface.getBoundingClientRect();
    const viewport = surface.querySelector(".react-flow__viewport");
    const matrix = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
    const nodes = [...surface.querySelectorAll(".react-flow__node")].map((node) => node.getBoundingClientRect());
    const left = Math.min(...nodes.map((node) => node.left));
    const top = Math.min(...nodes.map((node) => node.top));
    const width = Math.max(...nodes.map((node) => node.right)) - left;
    const height = Math.max(...nodes.map((node) => node.bottom)) - top;
    return {
      camera: { x: matrix.e, y: matrix.f, zoom: matrix.a },
      bounds: {
        x: (left - box.left - matrix.e) / matrix.a,
        y: (top - box.top - matrix.f) / matrix.a,
        width: width / matrix.a, height: height / matrix.a,
      },
      surface: { width: box.width, height: box.height },
      left: left - box.left, top: top - box.top,
      controlsBottom: surface.querySelector(".graph-navigator").getBoundingClientRect().bottom - box.top,
    };
  });
}

async function expectInitialFrame(graph) {
  let measured;
  await expect.poll(async () => {
    measured = await measuredFrame(graph);
    const expected = initialGraphViewport(measured.bounds, measured.surface.width, measured.surface.height);
    return Math.max(Math.abs(measured.camera.x - expected.x), Math.abs(measured.camera.y - expected.y),
      Math.abs(measured.camera.zoom - expected.zoom) * 1000);
  }, { message: "Initial framing must apply measured coordinates and zoom, not leave the origin at 100%" })
    .toBeLessThan(1);
  expect(measured.camera.zoom).toBeGreaterThanOrEqual(0.8);
  expect(measured.camera.zoom).toBeLessThanOrEqual(1);
  expect(measured.left).toBeGreaterThanOrEqual(31);
  expect(measured.top).toBeGreaterThanOrEqual(71);
  expect(measured.top).toBeGreaterThan(measured.controlsBottom + 8);
  await expect(graph.getByRole("button", { name: "Actual size", exact: true }))
    .toHaveText(`${Math.round(measured.camera.zoom * 100)}%`);
}

async function readableInitialView(graph, width) {
  await expect(graph.locator(".react-flow__edge")).not.toHaveCount(0);
  await expectInitialFrame(graph);
  if (width <= 760) {
    await graph.getByRole("button", { name: "Fit graph" }).click();
    await expect.poll(() => scale(graph)).toBeLessThan(0.8);
    expect(await scale(graph)).toBeGreaterThanOrEqual(0.2);
    await expectAllNodesInside(graph);
  }
  return transform(graph);
}

function watchWrites(page) {
  const writes = [];
  page.on("request", (request) => {
    if (request.method() === "POST" && new URL(request.url()).pathname.endsWith("/intent")) {
      writes.push(request.postDataJSON()?.kind);
    }
  });
  return writes;
}

async function seedExistingRun(canvas, run) {
  const source = fileURLToPath(new URL(`../../../test/fixtures/runs/${run}/`, import.meta.url));
  await cp(source, join(canvas.runs, run), { recursive: true });
}

const LONG_COMMAND = "cargo test --offline --workspace --all-targets -p bureau-core -p bureau-process-contract "
  + "--features extended-validation,concurrent-contracts,integration-fixtures -- --test-threads=1 "
  + "--nocapture --exact completes_every_concurrent_member_without_losing_its_recorded_outcome";

async function longCommandState(page) {
  const state = await page.evaluate(() => fetch("./state").then((response) => response.json()));
  const payload = JSON.parse(await readFile(new URL("../../../test/fixtures/concurrent-payload.json", import.meta.url), "utf8"));
  const steps = payload.config.pipelines["review-queue-pipeline"].steps;
  steps.find((step) => step.name === "run-checks").steps = ["unit", "lint"];
  Object.assign(steps.find((step) => step.name === "read-diff"),
    { name: "unit", type: "deterministic", role: null, run: LONG_COMMAND });
  Object.assign(steps.find((step) => step.name === "read-tests"),
    { name: "lint", run: "cargo clippy --offline --workspace" });
  process.env.BUREAU_CANVAS_TEST = "1";
  const { buildState } = await import("../../../extension.mjs");
  return buildState({ ...state, pipeline: "review-queue-pipeline" }, { payload });
}

async function expectConcurrentPreview(graph) {
  const card = (name) => graph.locator(`.react-flow__node[data-id="${name}"] :is(.flow-card, .editor-card)`);
  const unit = await card("unit").boundingBox();
  const lint = await card("lint").boundingBox();
  expect(unit.x).toBeCloseTo(lint.x, 1);
  expect(unit.y).toBeLessThan(lint.y);
  expect(unit.y + unit.height).toBeLessThan(lint.y);
  const preview = card("unit").locator(".detail");
  await expect(preview).toHaveText(LONG_COMMAND);
  await expect(preview).toHaveAttribute("title", LONG_COMMAND);
  const measurement = await preview.evaluate((element) => ({
    clippedPreview: element.scrollHeight > element.clientHeight,
    atMostThreeLines: element.clientHeight <= Number.parseFloat(getComputedStyle(element).lineHeight) * 3 + 1,
    hiddenHeight: element.scrollHeight - element.clientHeight,
  }));
  expect(measurement).toMatchObject({ clippedPreview: true, atMostThreeLines: true });
  expect(unit.height + measurement.hiddenHeight).toBeGreaterThan(lint.y - unit.y);
}

async function expectFullCommand(page, target) {
  if (target === "editor") {
    const command = page.getByRole("textbox", { name: "run", exact: true });
    await expect(command).toHaveValue(LONG_COMMAND);
    await command.focus();
    await command.press("Control+End");
    expect(await command.evaluate((element) => element.selectionStart)).toBe(LONG_COMMAND.length);
    await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
    return;
  }
  const command = page.getByRole("region", { name: "Selected step", exact: true }).getByText(LONG_COMMAND, { exact: true });
  await command.scrollIntoViewIfNeeded();
  await expect(command).toBeVisible();
  const visibility = await command.evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    const lines = [...range.getClientRects()];
    const box = element.getBoundingClientRect();
    // A pre-wrapped line's box includes hanging spaces beyond its last glyph.
    const text = element.firstChild;
    const glyphs = [...text.textContent].flatMap((character, index) => {
      if (!character.trim()) return [];
      range.setStart(text, index);
      range.setEnd(text, index + 1);
      return [...range.getClientRects()];
    });
    return {
      unclipped: element.scrollHeight <= element.clientHeight + 1,
      // Font metrics change line counts; every glyph must still fit vertically.
      insideHeight: glyphs.length > 0 && glyphs.every((glyph) => glyph.top >= box.top - 1 && glyph.bottom <= box.bottom + 1),
      insideWidth: glyphs.every((glyph) => glyph.left >= box.left - 1 && glyph.right <= box.right + 1),
      insideField: lines.at(-1).bottom <= box.bottom + 1,
      onScreen: lines.at(-1).bottom <= innerHeight,
      bounds: { width: box.width, bottom: box.bottom, lineBottom: lines.at(-1).bottom,
        left: box.left, right: box.right, lineLeft: Math.min(...lines.map((line) => line.left)),
        lineRight: Math.max(...glyphs.map((glyph) => glyph.right)),
        scrollHeight: element.scrollHeight, clientHeight: element.clientHeight },
    };
  });
  expect(visibility, JSON.stringify(visibility)).toMatchObject({
    unclipped: true, insideHeight: true, insideWidth: true, insideField: true, onScreen: true,
  });
}

test("viewer keeps light transitions as the default and scopes dark styling to the graph", async ({ page, canvas }) => {
  await openViewer(page, canvas);
  await expect(page.locator(".pipeline-routes")).toBeVisible();
  await expect(page.locator(".pipeline-flow")).toHaveCount(0);
  const background = (locator) => locator.evaluate((element) => {
    while (element) {
      const color = getComputedStyle(element).backgroundColor;
      if (color !== "rgba(0, 0, 0, 0)" && color !== "transparent") {
        return color.match(/\d+/gu).slice(0, 3).map(Number);
      }
      element = element.parentElement;
    }
    return [255, 255, 255];
  });
  const light = await background(page.locator(".pipeline-routes"));
  expect(Math.min(...light)).toBeGreaterThan(180);
  await page.getByRole("tab", { name: "graph", exact: true }).click();
  expect(Math.max(...await background(page.locator(".pipeline-flow")))).toBeLessThan(100);
  expect(Math.max(...await background(viewerCard(page, "verify")))).toBeLessThan(100);
  await expect(page.locator(".flow-card .graph-state")).toHaveText(["Design", "Design", "Design"]);
  await expect(page.getByRole("button", { name: "Review next" })).toHaveCount(0);
  await page.getByRole("tab", { name: "transitions", exact: true }).click();
  expect(await background(page.locator(".pipeline-routes"))).toEqual(light);
});

test("viewer camera changes the transform and percentage, resets actual size, and fits every card", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas);
  const controls = graph.getByRole("group", { name: "Graph view controls" });
  const actual = controls.getByRole("button", { name: "Actual size" });
  await actual.click();
  await expect(actual).toHaveText("100%");
  await expect.poll(() => scale(graph)).toBeCloseTo(1, 3);
  const before = await transform(graph);
  await controls.getByRole("button", { name: "Zoom in" }).click();
  await expect.poll(() => scale(graph)).toBeGreaterThan(1);
  await expect(actual).not.toHaveText("100%");
  expect(await transform(graph)).not.toBe(before);
  const enlarged = await scale(graph);
  await controls.getByRole("button", { name: "Zoom out" }).click();
  await expect.poll(() => scale(graph)).toBeLessThan(enlarged);
  await actual.click();
  await expect(actual).toHaveText("100%");
  await controls.getByRole("button", { name: "Fit graph" }).click();
  await expect.poll(() => scale(graph)).toBeLessThan(1);
  await expectAllNodesInside(graph);
});

test("viewer outcome captions stay outside every step and terminal card", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas);
  await expect(graph.locator(".edge-caption")).toHaveCount(10);
  await graph.getByRole("button", { name: "Fit graph" }).click();
  await nextPaint(graph);
  const overlaps = await graph.evaluate((surface) => {
    const boxes = (selector) => [...surface.querySelectorAll(selector)].map((element) => ({
      name: element.getAttribute("data-edge-id") ?? element.closest(".react-flow__node")?.getAttribute("data-id"),
      box: element.getBoundingClientRect(),
    }));
    const captions = boxes(".edge-caption");
    const cards = boxes(".react-flow__node .flow-card");
    const hits = [];
    for (const caption of captions) {
      for (const card of cards) {
        const a = caption.box;
        const b = card.box;
        if (a.left < b.right - 0.5 && a.right > b.left + 0.5 && a.top < b.bottom - 0.5 && a.bottom > b.top + 0.5) {
          hits.push({ caption: caption.name, card: card.name });
        }
      }
    }
    return hits;
  });
  expect(overlaps).toEqual([]);
});

test("viewer search selects and centers the real step, exposes handoffs, and restores focus on Escape", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas);
  const before = await connections(graph);
  expect(before.length).toBeGreaterThan(0);
  await page.keyboard.press("Control+k");
  const search = graph.getByRole("searchbox", { name: "Search steps" });
  await expect(search).toBeFocused();
  await search.fill("cargo test --offline");
  await expect(graph.locator(".graph-search-result")).toHaveCount(1);
  await search.press("Enter");
  await expect(viewerCard(page, "verify")).toHaveClass(/is-highlighted/u);
  const inspector = page.getByRole("region", { name: "Selected step", exact: true });
  await expect(inspector.getByRole("heading", { name: "verify", exact: true })).toBeVisible();
  await expect(inspector).toContainText("cargo test --offline");
  await expect(inspector.getByRole("heading", { name: "Handoffs" })).toBeVisible();
  await expectCentered(graph, viewerCard(page, "verify"));
  await search.press("Escape");
  await expect(search).toHaveCount(0);
  await expect(graph.getByRole("button", { name: /^Find steps/u })).toBeFocused();
  await inspector.getByRole("button", { name: "review", exact: true }).click();
  await expect(viewerCard(page, "review")).toHaveClass(/is-highlighted/u);
  await expect(inspector.getByRole("heading", { name: "review", exact: true })).toBeVisible();
  await expectCentered(graph, viewerCard(page, "review"));
  expect(await connections(graph)).toEqual(before);
});

test("empty and literal queries preserve all graph connections and never select a missing match", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas);
  const before = await connections(graph);
  await graph.getByRole("button", { name: /^Find steps/u }).click();
  const search = graph.getByRole("searchbox", { name: "Search steps" });
  await expect(graph.locator(".graph-search-result")).toHaveCount(3);
  const literal = '<img src=x onerror="window.graphQueryExecuted=true">[.*]+?';
  await search.fill(literal);
  await search.press("Enter");
  await expect(search).toHaveValue(literal);
  await expect(graph.locator(".graph-search-empty")).toHaveText("No matching steps or relations.");
  await expect(page.locator(".graph-step-inspector")).toHaveCount(0);
  expect(await page.evaluate(() => Boolean(window.graphQueryExecuted))).toBe(false);
  await expect(graph.locator(".graph-navigator img")).toHaveCount(0);
  expect(await connections(graph)).toEqual(before);
  await search.fill(" ");
  await expect(graph.locator(".graph-search-result")).toHaveCount(3);
  await expect(graph.locator(".react-flow__node-stepCard")).toHaveCount(3);
});

test("design attention reviews actual validation findings without claiming a failed run", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas);
  const state = await page.evaluate(() => fetch("./state").then((response) => response.json()));
  await page.evaluate((detail) => window.dispatchEvent(new CustomEvent("bureau-state", { detail })), applyFixture("invalid", state));
  await expect(graph.locator(".graph-attention")).toContainText("1 need attention");
  await graph.getByRole("button", { name: "Review next" }).click();
  await expect(viewerCard(page, "verify")).toHaveClass(/is-highlighted/u);
  await expect(viewerCard(page, "verify").locator(".graph-state")).toHaveText("Design");
  await expect(page.locator(".graph-step-inspector")).toContainText("must set `run`");
  await expectCentered(graph, viewerCard(page, "verify"));
  await graph.getByRole("button", { name: "Review next" }).click();
  await expect(viewerCard(page, "verify")).toHaveClass(/is-highlighted/u);
  await expect(graph.locator(".graph-state--failure")).toHaveCount(0);
});

test("editor search opens the real inspector without changing or saving its draft", async ({ editor }) => {
  const { page } = editor;
  const graph = page.locator(".editor-flow");
  const before = await connections(graph);
  const writes = watchWrites(page);
  await graph.getByRole("button", { name: /^Find steps/u }).click();
  const search = graph.getByRole("searchbox", { name: "Search steps" });
  await search.fill("verify");
  await search.press("Enter");
  await expect(page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify");
  await expect(page.getByRole("textbox", { name: "run", exact: true })).toHaveValue("cargo test --offline");
  await expect(page.locator('[data-ref="verify"]')).toHaveClass(/is-highlighted/u);
  await expectCentered(graph, page.locator('[data-ref="verify"]'));
  await search.press("Escape");
  await expect(graph.getByRole("button", { name: /^Find steps/u })).toBeFocused();
  await expect(page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify");
  await expect(page.locator(".editor-status")).toHaveText("saved");
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  expect(await connections(graph)).toEqual(before);
  expect(writes).toEqual([]);
  expect(editor.errors).toEqual([]);
});

for (const key of ["Enter", "Space"]) {
  test(`native node ${key} selects the editor inspector and relation without dirtying the draft`, async ({ editor }) => {
    const { page } = editor;
    const writes = watchWrites(page);
    const step = page.locator('.editor-flow .react-flow__node[data-id="verify"]');
    await step.focus();
    await step.press(key);
    await expect(step).toHaveClass(/selected/u);
    await expect(page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify");
    await expect(page.getByRole("textbox", { name: "run", exact: true })).toHaveValue("cargo test --offline");
    await expect(page.locator(".editor-flow .graph-search")).toHaveCount(0);
    await expect(page.locator(".editor-status")).toHaveText("saved");
    await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
    await page.getByRole("button", { name: "Relations", exact: true }).click();
    const relation = page.locator('.relation-flow .react-flow__node[data-id="role:reviewer"]');
    await relation.focus();
    await relation.press(key);
    await expect(relation).toHaveClass(/selected/u);
    await expect(page.locator(".relation-flow .graph-search")).toHaveCount(0);
    await page.getByRole("button", { name: "Pipeline", exact: true }).click();
    await expect(page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify");
    await expect(page.locator(".editor-status")).toHaveText("saved");
    expect(writes).toEqual([]);
    expect(editor.errors).toEqual([]);
  });
}

for (const modifier of ["Control", "Meta"]) {
  test(`${modifier}+K targets only the visible mounted graph and results use native Tab/Enter`, async ({ editor }) => {
    const { page } = editor;
    const pipeline = page.locator(".editor-flow");
    const relations = page.locator(".relation-flow");
    await expect(relations).toBeAttached();
    await expect(relations).not.toBeVisible();
    const trigger = pipeline.locator(".graph-navigator-toggle");
    await expect(trigger).toHaveAttribute("aria-keyshortcuts", "Control+k Meta+k");
    await expect(trigger).toHaveAttribute("title", /Ctrl\/Cmd\+K/u);
    await page.keyboard.press(`${modifier}+k`);
    const search = pipeline.getByRole("searchbox", { name: "Search steps" });
    await expect(search).toBeFocused();
    await expect(relations.locator(".graph-search")).toHaveCount(0);
    await search.fill("verify");
    await search.press("Tab");
    await expect(pipeline.locator(".graph-search-result")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify");
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
    await page.getByRole("button", { name: "Relations", exact: true }).click();
    await page.keyboard.press(`${modifier}+k`);
    const relationSearch = relations.getByRole("searchbox", { name: "Search nodes" });
    await expect(relationSearch).toBeFocused();
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await expect(pipeline.locator(".graph-search")).toHaveCount(0);
    await relationSearch.fill("reviewer");
    await relationSearch.press("Tab");
    await expect(relations.locator(".graph-search-result")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(relations.locator('.react-flow__node[data-id="role:reviewer"]')).toHaveClass(/selected/u);
    await page.keyboard.press("Escape");
    await expect(relations.locator(".graph-navigator-toggle")).toBeFocused();
    await page.getByRole("button", { name: "Pipeline", exact: true }).click();
    await expect(page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("verify");
    await expect(page.locator(".editor-status")).toHaveText("saved");
    expect(editor.errors).toEqual([]);
  });
}

test("relations search selects and centers a configured node without executing or mutating anything", async ({ editor }) => {
  const { page } = editor;
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.getByRole("button", { name: "Relations", exact: true }).click();
  const graph = page.locator(".relation-flow");
  await expect.poll(() => scale(graph)).toBeGreaterThan(0.6);
  const before = await connections(graph);
  expect(before.length).toBeGreaterThan(0);
  const writes = watchWrites(page);
  const initial = await transform(graph);
  await graph.getByRole("button", { name: /^Find nodes/u }).click();
  const search = graph.getByRole("searchbox", { name: "Search nodes" });
  await search.fill("reviewer");
  await search.press("Enter");
  const card = graph.locator('[data-ref="role:reviewer"]');
  await expect(graph.locator('.react-flow__node[data-id="role:reviewer"]')).toHaveClass(/selected/u);
  await expectCentered(graph, card);
  await expect.poll(() => scale(graph)).toBeCloseTo(1, 3);
  expect(await transform(graph)).not.toBe(initial);
  await search.fill("no-such-node");
  await expect(graph.locator(".graph-search-empty")).toBeVisible();
  await expect(graph.getByRole("button", { name: "Actual size" })).toHaveText("100%");
  expect(await scale(graph)).toBeCloseTo(1, 3);
  expect(await connections(graph)).toEqual(before);
  await search.press("Escape");
  await expect(graph.getByRole("button", { name: /^Find nodes/u })).toBeFocused();
  await page.getByRole("button", { name: "Pipeline", exact: true }).click();
  await expect(page.locator(".editor-status")).toHaveText("saved");
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  expect(writes).toEqual([]);
});

for (const [run, label, attention] of [["run-live", "Running", false], ["run-paused", "Paused", true]]) {
  test(`live graph labels and attention follow the ${run} fixture, never fabricated progress`, async ({ page, canvas }) => {
    await seedExistingRun(canvas, run);
    await openViewer(page, canvas);
    await page.getByRole("tab", { name: /live/iu }).click();
    await expect(page.locator(".flow-card .graph-state")).toHaveText(["No run", "No run", "No run"]);
    await expect(page.getByRole("button", { name: "Review next" })).toHaveCount(0);
    await page.getByLabel("Live run").selectOption(run);
    await expect(viewerCard(page, "implement").locator(".graph-state")).toHaveText(label);
    await expect(viewerCard(page, "verify").locator(".graph-state")).toHaveText("Pending");
    await expect(viewerCard(page, "review").locator(".graph-state")).toHaveText("Pending");
    await expect(page.locator(".run-status")).toHaveText(label.toLowerCase());
    await expect(page.getByRole("progressbar")).toHaveCount(0);
    expect((await page.locator(".flow-card").allTextContents()).join(" ")).not.toMatch(/\d+\s*%/u);
    if (attention) {
      await expect(page.locator(".graph-attention")).toContainText("1 need attention");
      await page.getByRole("button", { name: "Review next" }).click();
      await expect(viewerCard(page, "implement")).toHaveClass(/is-highlighted/u);
      await expect(page.locator(".graph-step-inspector")).toContainText("implement");
      await expectCentered(page.locator(".pipeline-flow"), viewerCard(page, "implement"));
    } else {
      await expect(page.getByRole("button", { name: "Review next" })).toHaveCount(0);
    }
  });
}

test("live backfill and event updates preserve the reader's zoom and pan", async ({ page, canvas }) => {
  await seedExistingRun(canvas, "run-live");
  await page.addInitScript(() => {
    const NativeEventSource = window.EventSource;
    window.__graphSources = [];
    window.EventSource = class extends NativeEventSource {
      constructor(...args) {
        super(...args);
        window.__graphSources.push(this);
      }
    };
  });
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  await page.route("**/runs/run-live/events", async (route) => {
    await held;
    await route.continue();
  });
  await openViewer(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  const graph = page.locator(".pipeline-flow");
  await expect.poll(() => scale(graph)).toBeLessThan(1);
  await graph.getByRole("button", { name: "Actual size" }).click();
  await graph.getByRole("button", { name: "Zoom in" }).click();
  await expect.poll(() => scale(graph)).toBeGreaterThan(1);
  const beforePan = await transform(graph);
  const box = await graph.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height * 0.75);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 70, box.y + box.height * 0.75 + 30, { steps: 5 });
  await page.mouse.up();
  await expect.poll(() => transform(graph)).not.toBe(beforePan);
  const camera = await transform(graph);
  const requested = page.waitForRequest("**/runs/run-live/events");
  await page.getByLabel("Live run").selectOption("run-live");
  await requested;
  expect(await transform(graph)).toBe(camera);
  release();
  await expect(viewerCard(page, "implement").locator(".graph-state")).toHaveText("Running");
  expect(await transform(graph)).toBe(camera);
  const events = [
    { seq: 3, at_ms: 1740000003000, kind: "step_finished", data: { run_id: "run-live", step: "implement", outcome: "success" } },
    { seq: 4, at_ms: 1740000004000, kind: "step_started", data: { run_id: "run-live", step: "verify" } },
  ];
  // The tail starts at EOF on discovery. Deliver fixture frames to the native
  // streams so this camera test cannot race discovery of a newly seeded run.
  await page.evaluate((frames) => {
    for (const source of window.__graphSources) {
      for (const event of frames) {
        source.dispatchEvent(new MessageEvent("run-event", { data: JSON.stringify({ run_id: "run-live", event }) }));
      }
    }
  }, events);
  await expect(viewerCard(page, "verify").locator(".graph-state")).toHaveText("Running");
  await expect(viewerCard(page, "implement").locator(".graph-state")).toHaveText("Success");
  expect(await transform(graph)).toBe(camera);
});

test("replay search sees recorded pending and successful states, not invented completion", async ({ page, canvas }) => {
  await openViewer(page, canvas);
  await page.getByRole("tab", { name: "replay", exact: true }).click();
  await page.getByLabel("Replay run").selectOption(RUN_ID);
  const scrubber = page.locator(".replay-scrubber");
  await expect(scrubber).not.toHaveAttribute("max", "0");
  await expect(page.locator(".flow-card .graph-state")).toHaveText(["Pending", "Pending", "Pending"]);
  await page.getByRole("button", { name: /^Find steps/u }).click();
  const search = page.getByRole("searchbox", { name: "Search steps" });
  await search.fill("success");
  await expect(page.locator(".graph-search-result")).toHaveCount(0);
  const graph = page.locator(".pipeline-flow");
  await graph.getByRole("button", { name: "Actual size" }).click();
  await graph.getByRole("button", { name: "Zoom in" }).click();
  await expect.poll(() => scale(graph)).toBeGreaterThan(1);
  const camera = await transform(graph);
  await scrubber.fill(await scrubber.getAttribute("max"));
  await expect(page.locator(".flow-card .graph-state")).toHaveText(["Success", "Success", "Success"]);
  await expect(page.locator(".graph-search-result")).toHaveCount(3);
  await expect(page.getByRole("button", { name: "Review next" })).toHaveCount(0);
  expect(await transform(graph)).toBe(camera);
});

for (const width of [1280, 760, 390]) {
  test.describe(`${width}px graph surfaces`, () => {
    test.use({ viewport: { width, height: 900 } });

    for (const target of ["viewer", "editor"]) {
      test(`${target} concurrent long-command preview does not overlap its sibling and the full command stays inspectable`, async ({ page, canvas }) => {
        const adapter = pageAdapter(page, canvas);
        await adapter.goto(target === "editor" ? "editor" : "pipeline");
        await adapter.publish(await longCommandState(page));
        await page.getByRole("tab", { name: /^graph$/iu }).click();
        const graph = page.locator(target === "editor" ? ".editor-flow" : ".pipeline-flow");
        await expect(graph.locator('.react-flow__node[data-id="unit"]')).toBeVisible();
        await graph.getByRole("button", { name: "Actual size", exact: true }).click();
        await expect.poll(() => scale(graph)).toBeCloseTo(1, 3);
        await expectConcurrentPreview(graph);
        const before = await connections(graph);
        const status = await page.locator(".editor-status").allTextContents();
        const writes = watchWrites(page);
        await graph.getByRole("button", { name: /^Find steps/u }).click();
        const search = graph.getByRole("searchbox", { name: "Search steps" });
        await search.fill("completes_every_concurrent_member_without_losing_its_recorded_outcome");
        await expect(graph.locator(".graph-search-result")).toHaveCount(1);
        await search.press("Enter");
        await search.press("Escape");
        await expect(graph.locator('.react-flow__node[data-id="unit"] .is-highlighted')).toBeVisible();
        await expectFullCommand(page, target);
        expect(await page.locator(".editor-status").allTextContents()).toEqual(status);
        expect(await connections(graph)).toEqual(before);
        expect(writes).toEqual([]);
      });
    }

    test("viewer starts legibly and keeps controls and selected details reachable", async ({ page, canvas }) => {
      const graph = await openGraph(page, canvas);
      const camera = await readableInitialView(graph, width);
      const find = graph.getByRole("button", { name: /^Find steps/u });
      await expectReachable(find);
      await find.click();
      await nextPaint(graph);
      expect(await transform(graph)).toBe(camera);
      const search = graph.getByRole("searchbox", { name: "Search steps" });
      await search.fill("verify");
      await expectReachable(graph.locator(".graph-search-result"));
      await search.press("Enter");
      await search.press("Escape");
      for (const name of ["implement", "review"]) {
        await find.click();
        await search.fill(name);
        await search.press("Enter");
        await expect(viewerCard(page, name)).toHaveClass(/is-highlighted/u);
        await expectCentered(graph, viewerCard(page, name));
        await search.press("Escape");
      }
      for (const name of ["Zoom out", "Actual size", "Zoom in", "Fit graph"]) {
        await expectReachable(graph.getByRole("button", { name, exact: true }));
      }
      await expectReachable(page.getByRole("button", { name: "Close step details" }));
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });

    test("editor and relations start legibly without page overflow or automatic camera resets", async ({ editor }) => {
      const { page } = editor;
      for (const [surface, label] of [["editor-flow", "steps"], ["relation-flow", "nodes"]]) {
        if (label === "nodes") {
          await page.getByRole("button", { name: "Relations", exact: true }).click();
        }
        const graph = page.locator(`.${surface}`);
        const camera = await readableInitialView(graph, width);
        const find = graph.getByRole("button", { name: new RegExp(`^Find ${label}`, "u") });
        await expectReachable(find);
        await find.click();
        await expectReachable(graph.getByRole("searchbox", { name: `Search ${label}` }));
        await graph.getByRole("searchbox").press("Escape");
        await nextPaint(graph);
        expect(await transform(graph)).toBe(camera);
        for (const name of ["Zoom out", "Actual size", "Zoom in", "Fit graph"]) {
          await expectReachable(graph.getByRole("button", { name, exact: true }));
        }
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      }
    });
  });
}

// The registry reviews an explicitly fitted overview; the cases above review
// readable defaults and navigation. These use the unchanged matrix judge so
// clipping, overlaps, contrast, edge counts, and offline guards still apply.
for (const width of [1280, 760]) {
  overviewTest.describe(`${width}px registry overviews`, () => {
    overviewTest.use({ viewport: { width, height: 900 } });

    for (const id of [
      "probe--selection-behind-relations-tab",
      "probe--dirty-editor-behind-relations-tab",
      "probe--editor-save-transport-lost",
      "probe--relation-open-under-expanded-card",
      "surface:config+data:validated+section:stack+disclosure:relation-open+card:collapsed",
      "surface:config+data:validated+section:empty+orphans:present+disclosure:relation-open",
    ]) {
      overviewTest(`fits the active graph for ${id}`, async ({ watched, host }) => {
        const state = STATES.find((item) => item.id === id);
        const result = await enterState(state, watched.page, host);
        expect(result.failures).toEqual([]);
        expect(result.settled).toBe(true);
        await expectAllNodesInside(watched.page.locator(".react-flow:visible"));
        if (id === "probe--editor-save-transport-lost") {
          expect(watched.errors).toEqual(expect.arrayContaining([expect.stringContaining("/intent")]));
        } else {
          expect(watched.errors).toEqual([]);
        }
      });
    }

    overviewTest("mode transitions and return edges fit each newly mounted graph", async ({ watched, host }) => {
      const design = STATES.find((item) => item.id === "surface:pipeline+data:validated+mode:design");
      expect((await enterState(design, watched.page, host)).failures).toEqual([]);
      for (const mode of ["live", "replay"]) {
        const target = STATES.find((item) => item.id === `surface:pipeline+data:validated+mode:${mode}`);
        const forward = TRANSITIONS.find((edge) => edge.from === design.id && edge.to === target.id);
        const back = TRANSITIONS.find((edge) => edge.from === target.id && edge.to === design.id);
        expect((await applyOps(forward.delta, target, watched.page, host)).failures).toEqual([]);
        await expectAllNodesInside(watched.page.locator(".pipeline-flow"));
        expect((await applyOps(back.delta, design, watched.page, host)).failures).toEqual([]);
        await expectAllNodesInside(watched.page.locator(".pipeline-flow"));
      }
      expect(watched.errors).toEqual([]);
    });

    overviewTest("replay is fitted only after its selected log is loaded", async ({ watched, host }) => {
      const state = STATES.find((item) => item.dimensions?.surface === "pipeline"
        && item.dimensions.data === "validated" && item.dimensions.mode === "replay"
        && item.dimensions.run === "finished" && item.dimensions.transport === "rest");
      const result = await enterState(state, watched.page, host);
      expect(result.failures).toEqual([]);
      await expectAllNodesInside(watched.page.locator(".pipeline-flow"));
      expect(watched.errors).toEqual([]);
    });

    overviewTest("group overview refits after collapse and its real return edge", async ({ watched, host }) => {
      const expanded = STATES.find((item) => item.id === "probe--group-expanded");
      const collapsed = STATES.find((item) => item.id === "probe--group-collapsed");
      const edge = TRANSITIONS.find((item) => item.from === expanded.id && item.to === collapsed.id);
      const back = TRANSITIONS.find((item) => item.from === collapsed.id && item.to === expanded.id);
      expect((await enterState(expanded, watched.page, host)).failures).toEqual([]);
      expect((await applyOps(edge.delta, collapsed, watched.page, host)).failures).toEqual([]);
      expect((await applyOps(back.delta, expanded, watched.page, host)).failures).toEqual([]);
      await expectAllNodesInside(watched.page.locator(".pipeline-flow"));
      expect(watched.errors).toEqual([]);
    });

    overviewTest("editor draft round trip is reviewed as the same complete graph", async ({ watched, host }) => {
      const state = STATES.find((item) => item.id === "probe--draft-survives-a-tab-round-trip");
      expect((await enterState(state, watched.page, host)).failures).toEqual([]);
      await expectAllNodesInside(watched.page.locator(".editor-flow"));
      expect(watched.errors).toEqual([]);
    });

    overviewTest("renamed editor selection receives a complete fitted overview", async ({ watched, host }) => {
      const state = STATES.find((item) => item.id === "surface:editor+tab:pipeline+pick:agent+edit:renamed");
      expect((await enterState(state, watched.page, host)).failures).toEqual([]);
      await expectAllNodesInside(watched.page.locator(".editor-flow"));
      await expect(watched.page.getByRole("textbox", { name: "name", exact: true })).toHaveValue("agent-renamed");
      await expect(watched.page.locator(".editor-status")).toHaveText("unsaved edits");
      expect(watched.errors).toEqual([]);
    });
  });
}
