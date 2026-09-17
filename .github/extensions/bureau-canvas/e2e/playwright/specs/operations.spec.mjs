import { appendFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, SAMPLE, test } from "../fixtures.mjs";
import { collect } from "../../../web/statelab/checks.mjs";

test.use({ entryView: "operations" });
const PIPELINE = "agent-eligible-pipeline";

async function seed(canvas, id, tail = [], age = 1000) {
  const source = { remote: "https://example.invalid/reviewed-config.git", reference: "reviewed", commit: "execution-commit-not-authoring-head" };
  const events = [{ kind: "run_started", data: { run_id: id, assignment: SAMPLE.assignment,
    snapshot: { pipeline: { name: PIPELINE }, config_source: source } } }, ...tail]
    .map((event, seq) => ({ seq, at_ms: Date.now() - age + seq, ...event }));
  await mkdir(join(canvas.runs, id));
  await writeFile(join(canvas.runs, id, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
  return events;
}

const step = { kind: "step_started", data: { step: "implement" } };
const pause = { kind: "output", data: { stream: "run", data: "run paused at a step boundary: operator pause" } };
const failed = { kind: "run_finished", data: { outcome: "failure", terminal: "abort", message: "Verification failed." } };

test("the shared default starts at Operations and reaches assignment, pipeline, and editor by keyboard", async ({ page, canvas }) => {
  const writes = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/intent")) writes.push(request.postDataJSON()?.kind);
  });
  await page.goto(canvas.url);
  await expect(page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
  await expect(page.getByText("Sample configuration", { exact: true })).toBeVisible();
  await expect(page.getByText("Current adopted source not observed", { exact: true })).toBeVisible();
  const configure = page.getByRole("button", { name: `Configure assignment ${SAMPLE.assignment}`, exact: true });
  await configure.focus();
  await configure.press("Enter");
  await expect(page.locator(".assignment-head")).toBeFocused();
  await expect(page.locator(".assignment-head")).toHaveAttribute("aria-expanded", "true");
  await page.getByRole("button", { name: `Open pipeline ${PIPELINE}`, exact: true }).click();
  await expect(page.getByTestId("design-surface-transitions")).toHaveAttribute("aria-selected", "true");
  await page.getByRole("link", { name: "Edit pipeline", exact: true }).click();
  await expect(page.locator(".editor-shell")).toBeVisible();
  expect(writes.every((kind) => ["navigate", "open-pipeline"].includes(kind))).toBe(true);
});

test("exact run links hand off to real Live controls and Replay without selecting another run", async ({ page, canvas }) => {
  await seed(canvas, "paused-run", [step, pause]);
  await seed(canvas, "failed-run", [step, failed]);
  await page.goto(canvas.url);
  await page.getByRole("button", { name: "Inspect in Live: paused-run", exact: true }).click();
  await expect(page.getByLabel("Live run", { exact: true })).toHaveValue("paused-run");
  await expect(page.getByTestId("run-resume")).toBeVisible();
  await page.getByRole("button", { name: "Operations", exact: true }).click();
  await page.getByRole("button", { name: "Open in Replay: failed-run", exact: true }).click();
  await expect(page.getByLabel("Replay run", { exact: true })).toHaveValue("failed-run");
  await expect(page.getByLabel("Replay position", { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByLabel("Replay run", { exact: true })).toHaveValue("failed-run");
});

async function requestOperations(page) {
  await page.evaluate(() => {
    void fetch("./intent", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind: "navigate", input: { view: "operations" } }) });
  });
}

test("shared navigation reaches Operations from the clean standalone editor", async ({ editor }) => {
  await requestOperations(editor.page);
  await expect(editor.page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
});

test("shared navigation preserves a refused editor draft across refresh and discards only on consent", async ({ editor }) => {
  const { page } = editor;
  await page.getByRole("button", { name: "+ Add step" }).click();
  await expect(page.locator(".editor-status")).toContainText(/unsaved|issue/u);
  let accepted = false;
  let dialogs = 0;
  page.on("dialog", (dialog) => {
    dialogs += 1;
    return accepted ? dialog.accept() : dialog.dismiss();
  });
  await requestOperations(page);
  await expect.poll(() => dialogs).toBe(1);
  await page.evaluate(() => fetch("./intent", { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "operations", refresh: true }) }));
  await expect(page.locator('[data-ref="step-4"]')).toBeVisible();
  accepted = true;
  await requestOperations(page);
  await expect(page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
  expect(dialogs).toBe(2);
});

test("remote and on-screen Operations navigation respect unsaved assignment fields with one consent", async ({ card }) => {
  const { page } = card;
  await page.locator(".limits-value").click();
  await page.getByRole("button", { name: "runs per day limit" }).click();
  await expect(page.locator(".limits-editor")).toHaveAttribute("data-dirty", "true");
  let accepted = false;
  let dialogs = 0;
  page.on("dialog", (dialog) => {
    dialogs += 1;
    return accepted ? dialog.accept() : dialog.dismiss();
  });
  await requestOperations(page);
  await expect.poll(() => dialogs).toBe(1);
  await expect(page.locator(".navigation-error")).toContainText("unsaved field changes were kept");
  await page.evaluate(() => fetch("./intent", { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ kind: "operations", refresh: true }) }));
  await expect(page.locator(".limits-editor")).toHaveAttribute("data-dirty", "true");
  accepted = true;
  await page.getByRole("button", { name: "Operations", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
  expect(dialogs).toBe(2);
});

test("attention, search and live refresh preserve unknown, stale, missing and corrupt evidence", async ({ page, canvas }) => {
  await seed(canvas, "active-run", [step]);
  await seed(canvas, "paused-run", [step, pause]);
  await seed(canvas, "failed-run", [step, failed]);
  await seed(canvas, "stale-run", [step], 600_000);
  await mkdir(join(canvas.runs, "missing-log"));
  await seed(canvas, "corrupt-log");
  await appendFile(join(canvas.runs, "corrupt-log", "events.jsonl"), "corrupt complete line\n");
  await page.goto(canvas.url);
  const stale = page.locator('[data-run-id="stale-run"]');
  await expect(stale).toContainText("Unfinished, stale evidence");
  await expect(page.locator('[data-run-id="missing-log"]')).toContainText("Run log is missing");
  await expect(page.locator('[data-run-id="corrupt-log"]')).toContainText("Invalid JSON");
  await expect(page.locator('[data-run-id="failed-run"]')).toContainText("Unknown");
  await page.getByRole("button", { name: "Show failed runs: 1", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Observed runs", exact: true })).toBeFocused();
  await expect(page.locator(".ops-run")).toHaveCount(1);
  await page.getByRole("button", { name: "All runs", exact: true }).click();
  await page.getByLabel("Find a run", { exact: true }).fill("STALE");
  await expect(page.locator(".ops-run")).toHaveCount(1);
  await page.getByLabel("Find a run", { exact: true }).fill("");
  await page.getByRole("button", { name: "Failed", exact: true }).click();
  await expect(page.locator(".ops-run")).toHaveCount(1);
  await page.getByRole("button", { name: "Active", exact: true }).click();
  await expect(page.locator(".ops-run")).toHaveCount(1);
  await appendFile(join(canvas.runs, "active-run", "events.jsonl"),
    `${JSON.stringify({ ...failed, seq: 2, at_ms: Date.now() })}\n`);
  await expect(page.getByText("No runs match these filters.", { exact: true })).toBeVisible({ timeout: 10_000 });
});

test("read failure keeps the last snapshot visibly stale and never reports healthy zero activity", async ({ page, canvas }) => {
  await seed(canvas, "watched-run", [step]);
  await page.goto(canvas.url);
  await expect(page.locator('[data-run-id="watched-run"]')).toBeVisible();
  await page.route("**/runs", (route) => route.fulfill({ status: 503, body: "unavailable" }));
  await expect(page.getByText(/Showing the last readable snapshot, not current activity/u)).toBeVisible({ timeout: 10_000 });
  await expect(page.locator(".ops-count")).toHaveText(["Unknown", "Unknown", "Unknown", "Unknown"]);
  await expect(page.locator('[data-run-id="watched-run"]')).toBeVisible();
});

test("an incomplete inventory remains unavailable rather than idle when opening Live", async ({ page, canvas }) => {
  await page.route("**/runs", (route) => route.fulfill({ json: { runs: [],
    observation: { state: "limited", message: "Directory preview is limited; other runs may need attention." } } }));
  await page.goto(canvas.url);
  await expect(page.locator(".ops-count")).toHaveText(["Unknown", "Unknown", "Unknown", "Unknown"]);
  await page.getByRole("button", { name: `Run controls for ${SAMPLE.assignment}`, exact: true }).click();
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "unavailable");
  await expect(page.getByTestId("live-count")).not.toHaveAttribute("data-count", "0");
});

async function configFixture(page, canvas, validation) {
  const state = await (await page.request.get(new URL("/state", canvas.url).href)).json();
  state.validation = validation;
  state.authoring = { state: "unavailable", changes: null, commit: null };
  state.config.view = { assignments: [], roles: [], repos: [], pipelines: [], orphans: [] };
  state.pipelines = {};
  await page.route("**/state", (route) => route.fulfill({ json: state }));
  await page.route("**/events", (route) => route.fulfill({
    contentType: "text/event-stream", body: `event: state\ndata: ${JSON.stringify(state)}\n\n`,
  }));
}

test("invalid empty config and missing observations explain next actions rather than a pass", async ({ page, canvas }, info) => {
  await page.setViewportSize({ width: 320, height: 900 });
  await configFixture(page, canvas, { state: "validated", ok: false, errors: ["assignments/work.yaml: invalid YAML"] });
  await page.route("**/runs", (route) => route.fulfill({ json: { runs: [],
    observation: { state: "missing", message: "Run directory is missing; no run evidence has been read." } } }));
  await page.goto(canvas.url);
  await expect(page.getByText("Invalid configuration", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Configuration validation errors")).toContainText("assignments/work.yaml: invalid YAML");
  await expect(page.getByText(/No assignments available/u)).toBeVisible();
  await expect(page.locator(".ops-count")).toHaveText(["Unknown", "Unknown", "Unknown", "Unknown"]);
  await expect(page.getByRole("button", { name: "Refresh config and evidence", exact: true })).toBeEnabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: info.outputPath("operations-invalid-320.png"), fullPage: true });
});

test("SDK factory native completion and measured credits never become a Bureau result or cloud control", async ({ page, canvas }) => {
  const factory = JSON.parse(await readFile(new URL("../../../test/fixtures/copilot-factory.json", import.meta.url), "utf8"));
  const fact = (data) => ({ kind: "copilot_factory", data: { session_id: "sdk-observed", ...data } });
  await seed(canvas, "native-completed", [step,
    fact({ event: "prepared", intent: { session_id: "sdk-observed", step: "implement", factory, workspace: { directory: "/preserved/work" } } }),
    fact({ event: "accepted", run_id: "native-run", attempt: 1 }),
    fact({ event: "observed", run: { runId: "native-run", status: "completed" }, summary: {
      runId: "native-run", factoryName: factory.name, status: "completed", canResume: false,
      consumed: { activeMs: 1000, subagents: 2, nanoAiu: 2e9 },
    } }),
  ]);
  await page.goto(canvas.url);
  const row = page.locator('[data-run-id="native-completed"]');
  await row.getByText("Local SDK factory evidence", { exact: true }).click();
  await expect(row).toContainText("sdk-observed");
  await expect(row).toContainText("native-run");
  await expect(row.locator(".ops-state")).toHaveText("Active (observed)");
  await expect(row).toContainText("Unknown");
  await expect(page.locator(".ops-boundary")).toContainText("cloud pause, resume, cancel, retry, approval, and feedback are unsupported");
  await row.getByText("Run source and evidence", { exact: true }).click();
  await expect(row).toContainText("execution-commit-not-authoring-head");
  await expect(row).toContainText("Not recorded");
});

for (const width of [320, 375, 1280]) {
  test(`Operations remains usable at ${width}px with named controls and visible keyboard focus`, async ({ page, canvas }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await seed(canvas, "failed-run-with-a-long-but-readable-identifier", [step, failed]);
    await page.goto(canvas.url);
    await expect(page.getByRole("heading", { name: "Operations", exact: true })).toBeVisible();
    await expect(page.locator(".ops-run")).toHaveCount(2);
    const search = page.getByLabel("Find a run", { exact: true });
    await search.focus();
    await expect(search).toBeFocused();
    expect(await search.evaluate((element) => getComputedStyle(element).outlineWidth)).toBe("3px");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const buttons = await page.locator(".operations button, .bureau-navigation button").evaluateAll((elements) => elements.map((element) =>
      ({ height: element.getBoundingClientRect().height, name: element.getAttribute("aria-label") || element.textContent.trim() })));
    expect(buttons.every((button) => button.height >= 44 && button.name.length > 0)).toBe(true);
    await page.screenshot({ path: info.outputPath(`operations-${width}.png`), fullPage: true });
  });
}

test("Operations text and controls meet contrast in light and host-token dark themes", async ({ page, canvas }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await seed(canvas, "failed-run", [step, failed]);
  await seed(canvas, "paused-run", [step, pause]);
  await page.goto(canvas.url);
  await expect(page.locator(".ops-run")).toHaveCount(3);
  const request = { selectors: [], measure: [], contrast: [".operations p", ".operations dt",
    ".operations a", ".operations button", ".operations summary", ".ops-state", ".bureau-navigation button"] };
  for (const dark of [false, true]) {
    if (dark) await page.addStyleTag({ content: `:root {
      --background-color-default: #0d1117; --text-color-default: #e6edf3; --text-color-muted: #9da7b3;
      --border-color-default: #30363d; --true-color-blue: #58a6ff; --true-color-red: #f85149;
      --true-color-yellow: #d29922; --true-color-green: #3fb950; --color-focus-outline: #58a6ff;
    }` });
    await page.getByRole("button", { name: "Refresh config and evidence", exact: true }).hover();
    expect(await page.locator(".operations .btn").first().evaluate((element) => getComputedStyle(element).transitionDuration)).toBe("0s");
    const snapshot = await page.evaluate(({ source, request }) => new Function(`return (${source})`)()(document, request),
      { source: collect.toString(), request });
    expect(snapshot.contrast.length).toBeGreaterThan(20);
    expect(snapshot.contrast.filter((item) => !Number.isFinite(item.ratio) || item.ratio < 4.5),
      dark ? "dark contrast" : "light contrast").toEqual([]);
  }
});

async function navigationContrast(page) {
  const snapshot = await page.evaluate((source) => new Function(`return (${source})`)()(document,
    { selectors: [], measure: [], contrast: [".bureau-navigation button"] }), collect.toString());
  expect(snapshot.contrast).toHaveLength(2);
  expect(snapshot.contrast.filter((item) => !Number.isFinite(item.ratio) || item.ratio < 4.5)).toEqual([]);
}

for (const motion of ["no-preference", "reduce"]) {
  test(`selected global navigation stays readable on the graph's own dark surface (${motion})`, async ({ page, canvas }) => {
    await page.emulateMedia({ reducedMotion: motion });
    await page.goto(canvas.url);
    await page.getByRole("button", { name: `Open pipeline ${PIPELINE}`, exact: true }).click();
    const properties = await page.locator(".bureau-navigation button").evaluateAll((buttons) =>
      buttons.flatMap((button) => getComputedStyle(button).transitionProperty.split(",").map((value) => value.trim())));
    expect(properties.filter((property) => ["all", "color", "background-color"].includes(property))).toEqual([]);
    await page.getByTestId("design-surface-graph").click();
    await expect(page.locator(".pipeline-flow")).toBeVisible();
    const navigation = page.getByRole("navigation", { name: "Bureau views" });
    const selected = navigation.getByRole("button", { name: "Configuration", exact: true });
    const inactive = navigation.getByRole("button", { name: "Operations", exact: true });
    await expect(selected).toHaveAttribute("aria-current", "page");
    await navigationContrast(page);
    for (const button of [inactive, selected]) {
      await button.hover();
      await navigationContrast(page);
    }
    await inactive.focus();
    for (const [key, button] of [["Tab", selected], ["Shift+Tab", inactive]]) {
      await page.keyboard.press(key);
      await expect(button).toBeFocused();
      expect(await button.evaluate((element) => element.matches(":focus-visible"))).toBe(true);
      expect(await button.evaluate((element) => getComputedStyle(element).outlineWidth)).toBe("3px");
      await navigationContrast(page);
    }
  });
}
