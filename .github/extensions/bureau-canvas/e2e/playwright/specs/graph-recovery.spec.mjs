import { expect, RUN_ID, test } from "../fixtures.mjs";

test.use({ entryView: "operations" });

async function measurementProbe(page, drop = false) {
  await page.clock.install();
  await page.addInitScript((drop) => {
    const probe = { drop, allowed: [], blocked: [], queries: [], deliveries: [] };
    window.__graphMeasurements = probe;
    const Native = window.ResizeObserver;
    window.ResizeObserver = class extends Native {
      constructor(callback) {
        super((entries, observer) => {
          if (probe.drop && entries.some((entry) => entry.target.hasAttribute("data-id"))) {
            probe.deliveries.push(() => callback(entries, observer));
            const allowed = entries.filter((entry) => probe.allowed.includes(entry.target.dataset.id));
            if (allowed.length) callback(allowed, observer);
          } else {
            callback(entries, observer);
          }
        }, drop);
      }
    };
    const query = Element.prototype.querySelector;
    Element.prototype.querySelector = function (selector) {
      const node = query.call(this, selector);
      // Only the vendor's explicit repair queries use this selector.
      if (/^\.react-flow__node\[data-id="/u.test(selector)) {
        probe.queries.push(node?.dataset.id);
        if (probe.blocked.includes(node?.dataset.id)) return null;
      }
      return node;
    };
  });
}

async function visibleNodes(graph) {
  const nodes = graph.locator(".react-flow__node");
  expect(await nodes.count()).toBeGreaterThan(0);
  for (const node of await nodes.all()) await expect(node).toBeVisible();
}

async function openPipeline(page, canvas, drop = false) {
  await measurementProbe(page, drop);
  await page.goto(canvas.url);
  await page.getByRole("button", { name: "Configuration", exact: true }).click();
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
}

async function openGraph(page, canvas, surface) {
  await openPipeline(page, canvas);
  if (surface === "editor" || surface === "relations") {
    await page.getByRole("link", { name: "Edit" }).click();
    await page.getByRole("tab", { name: "graph", exact: true }).click();
    if (surface === "relations") await page.getByRole("button", { name: "Relations", exact: true }).click();
  } else {
    if (surface === "viewer") await page.getByRole("tab", { name: "graph", exact: true }).click();
    else await page.getByTestId(`mode-${surface}`).click();
    if (surface === "replay") await page.getByLabel("Replay run").selectOption(RUN_ID);
  }
  const selector = surface === "editor" ? ".editor-flow" : surface === "relations" ? ".relation-flow" : ".pipeline-flow";
  const graph = page.locator(selector);
  await visibleNodes(graph);
  await page.clock.pauseAt(new Date(await page.evaluate(() => Date.now() + 1000)));
  await page.evaluate(() => { window.__graphMeasurements.queries = []; });
  return graph;
}

const ids = (graph) => graph.locator(".react-flow__node").evaluateAll((nodes) => nodes.map((node) => node.dataset.id));
const camera = (graph) => graph.locator(".react-flow__viewport")
  .evaluate((node) => getComputedStyle(node).transform);
const queries = (page) => page.evaluate(() => window.__graphMeasurements.queries);

async function loseMeasurements(page, graph, id) {
  await page.evaluate(() => { window.__graphMeasurements.drop = true; });
  await graph.locator(`.react-flow__node[data-id="${id}"]`).click();
  await expect.poll(() => graph.locator(".react-flow__node").evaluateAll((nodes) =>
    nodes.some((node) => getComputedStyle(node).visibility === "hidden"))).toBe(true);
}

async function expectWholeGraphInside(graph) {
  await visibleNodes(graph);
  const surface = await graph.boundingBox();
  for (const node of await graph.locator(".react-flow__node").all()) {
    const box = await node.boundingBox();
    expect(box.x).toBeGreaterThanOrEqual(surface.x - 1);
    expect(box.y).toBeGreaterThanOrEqual(surface.y - 1);
    expect(box.x + box.width).toBeLessThanOrEqual(surface.x + surface.width + 1);
    expect(box.y + box.height).toBeLessThanOrEqual(surface.y + surface.height + 1);
  }
}

for (const surface of ["viewer", "editor", "relations", "live", "replay"]) {
  test(`${surface} healthy internal measurements do not spend repair attempts`, async ({ page, canvas }) => {
    const graph = await openGraph(page, canvas, surface);
    await page.clock.runFor(800);
    expect(await queries(page)).toEqual([]);
    await visibleNodes(graph);
  });
}

for (const surface of ["viewer", "editor", "relations", "live", "replay"]) {
  test(`${surface} recovers repeated same-ID measurement loss without moving the camera`, async ({ page, canvas }) => {
    const graph = await openGraph(page, canvas, surface);
    const originalIds = await ids(graph);
    const targets = surface === "relations" ? ["role:reviewer", "role:implementer"] : ["verify", "implement"];
    const originalCamera = await camera(graph);
    for (let episode = 0; episode < 6; episode++) {
      await loseMeasurements(page, graph, targets[episode % targets.length]);
      await page.clock.runFor(160);
      await visibleNodes(graph);
      expect(await ids(graph)).toEqual(originalIds);
      expect(await camera(graph)).toBe(originalCamera);
    }
  });
}

test("one loss is bounded at five repairs and a later recovered episode gets its own budget", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas, "viewer");
  const originalIds = await ids(graph);
  await page.evaluate((blocked) => { window.__graphMeasurements.blocked = blocked; }, originalIds);
  await loseMeasurements(page, graph, "verify");
  await page.clock.runFor(800);
  expect(await queries(page)).toHaveLength(originalIds.length * 5);
  await page.clock.runFor(800);
  expect(await queries(page)).toHaveLength(originalIds.length * 5);
  await page.evaluate(() => {
    const probe = window.__graphMeasurements;
    probe.blocked = [];
    for (const deliver of probe.deliveries.splice(0)) deliver();
    probe.queries = [];
  });
  await visibleNodes(graph);
  await loseMeasurements(page, graph, "implement");
  await page.clock.runFor(160);
  await visibleNodes(graph);
  expect(await queries(page)).toHaveLength(originalIds.length);
});

test("Fit waits for every intended node instead of fitting a measured subset", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas, "viewer");
  await graph.getByRole("button", { name: "Zoom in", exact: true }).click();
  await page.clock.runFor(32);
  const originalCamera = await camera(graph);
  const originalIds = await ids(graph);
  await page.evaluate((blocked) => {
    window.__graphMeasurements.blocked = blocked;
    window.__graphMeasurements.allowed = ["implement"];
  },
    originalIds.filter((id) => id !== "implement"));
  await loseMeasurements(page, graph, "verify");
  await page.clock.runFor(100);
  await expect(graph.locator('.react-flow__node[data-id="implement"]')).toBeVisible();
  await expect(graph.locator('.react-flow__node[data-id="verify"]')).toBeHidden();
  await graph.getByRole("button", { name: "Fit graph", exact: true }).click();
  await page.clock.runFor(32);
  expect(await camera(graph)).toBe(originalCamera);
  await page.evaluate(() => { window.__graphMeasurements.blocked = []; });
  await page.clock.runFor(160);
  await expectWholeGraphInside(graph);
  expect(await ids(graph)).toEqual(originalIds);
  expect(await camera(graph)).not.toBe(originalCamera);
});

test("adding an editor step fits only after the added node is measured", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas, "editor");
  const originalIds = await ids(graph);
  const originalCamera = await camera(graph);
  await page.evaluate(() => { window.__graphMeasurements.drop = true; });
  await page.getByTestId("editor-add-step").click();
  await expect(graph.locator(".react-flow__node")).toHaveCount(originalIds.length + 1);
  const added = (await ids(graph)).filter((id) => !originalIds.includes(id));
  await page.evaluate((blocked) => { window.__graphMeasurements.blocked = blocked; }, added);
  await page.clock.runFor(100);
  await expect(graph.locator(`.react-flow__node[data-id="${added[0]}"]`)).toBeHidden();
  expect(await camera(graph)).toBe(originalCamera);
  await page.evaluate(() => { window.__graphMeasurements.blocked = []; });
  await page.clock.runFor(400);
  await expectWholeGraphInside(graph);
});

test("a hidden relation surface keeps its repair budget until it becomes visible", async ({ page, canvas }) => {
  await openPipeline(page, canvas, true);
  await page.getByRole("link", { name: "Edit" }).click();
  await expect(page.locator(".editor-tabs")).toBeVisible();
  await page.clock.pauseAt(new Date(await page.evaluate(() => Date.now() + 1000)));
  await page.clock.runFor(800);
  expect(await queries(page)).toEqual([]);
  await page.getByRole("button", { name: "Relations", exact: true }).click();
  const graph = page.locator(".relation-flow");
  await graph.getByRole("button", { name: "Fit graph", exact: true }).click();
  await page.clock.runFor(160);
  await expectWholeGraphInside(graph);
});

test("an exhausted Fit ends busy state and an explicit Fit starts bounded recovery", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas, "viewer");
  const originalIds = await ids(graph);
  await page.evaluate((blocked) => { window.__graphMeasurements.blocked = blocked; }, originalIds);
  await loseMeasurements(page, graph, "verify");
  await graph.getByRole("button", { name: "Fit graph", exact: true }).click();
  await page.clock.runFor(800);
  expect(await queries(page)).toHaveLength(originalIds.length * 5);
  await expect(graph.getByRole("group", { name: "Graph view controls" })).not.toHaveAttribute("aria-busy", "true");
  await expect(graph.locator(".graph-help")).toHaveText("Some nodes could not be measured. Fit to retry.");
  await page.clock.runFor(800);
  expect(await queries(page)).toHaveLength(originalIds.length * 5);
  await page.evaluate(() => { window.__graphMeasurements.blocked = []; });
  await graph.getByRole("button", { name: "Fit graph", exact: true }).click();
  await page.clock.runFor(160);
  await expectWholeGraphInside(graph);
  expect(await queries(page)).toHaveLength(originalIds.length * 6);
  await expect(graph.locator(".graph-help")).toHaveText("Drag to pan. Scroll to zoom.");
});

test("Fit remains queued through a successful fifth measurement repair", async ({ page, canvas }) => {
  const graph = await openGraph(page, canvas, "viewer");
  const originalIds = await ids(graph);
  await page.evaluate((blocked) => { window.__graphMeasurements.blocked = blocked; }, originalIds);
  await loseMeasurements(page, graph, "verify");
  await graph.getByRole("button", { name: "Fit graph", exact: true }).click();
  for (let attempt = 1; attempt <= 4; attempt++) {
    await page.clock.runFor(100);
    expect(await queries(page)).toHaveLength(originalIds.length * attempt);
  }
  await page.evaluate(() => { window.__graphMeasurements.blocked = []; });
  await page.clock.runFor(160);
  await expectWholeGraphInside(graph);
  expect(await queries(page)).toHaveLength(originalIds.length * 5);
  await expect(graph.locator(".graph-help")).toHaveText("Drag to pan. Scroll to zoom.");
});
