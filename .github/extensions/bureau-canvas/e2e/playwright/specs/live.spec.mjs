import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, SAMPLE, test } from "../fixtures.mjs";

async function openPipeline(page, canvas) {
  await page.goto(canvas.url);
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
}

/**
 * Writes one live run into this worker's run root, before the page is opened.
 *
 * The `run_started` payload mirrors `RunStartedData` in
 * `crates/bureau/src/runlog/event.rs` — `run_id`, `assignment`, `snapshot` —
 * because a fixture is a claim about what bureau writes. It used to carry a flat
 * `data.pipeline`, which that struct has no field for and bureau has never
 * emitted, so the whole pipeline-scoped Live surface was green against a shape
 * production cannot produce.
 */
async function seedRun(canvas, runId) {
  const dir = join(canvas.runs, runId);
  const snapshot = {
    run_id: runId,
    assignment: { name: SAMPLE.assignment, pipeline: "agent-eligible-pipeline" },
    pipeline: { name: "agent-eligible-pipeline" },
  };
  const events = [
    { seq: 0, at_ms: 1_800_000_000_000, kind: "run_started", data: { run_id: runId, assignment: SAMPLE.assignment, snapshot } },
    { seq: 1, at_ms: 1_800_000_001_000, kind: "step_started", data: { run_id: runId, step: "implement" } },
  ];
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
}

test("Live distinguishes an idle reconciler from a broken listing", async ({ page, canvas }) => {
  await openPipeline(page, canvas);

  await expect(page.getByTestId("live-count")).toHaveAttribute("data-count", "0");
  await page.getByRole("tab", { name: /live, 0 runs in progress/iu }).click();
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "idle");
  await expect(page.getByText("A reconcile loop is not itself a run.")).toBeVisible();
  await expect(page.getByLabel("Live run")).toBeDisabled();
  await expect(page.getByRole("button", { name: "Run reconcile now" })).toBeEnabled();
});

/*
 * The badge advertises; the reader picks. There is deliberately no auto-follow.
 *
 * This test used to be titled "…follows the newest run without a manual pick"
 * and then made a manual pick, so the load-bearing half of its own name was
 * asserted by nothing — `useLiveOverlay` starts `runId` at `null` and only ever
 * changes it through the picker's `onChange`. It would have passed identically
 * against a build that had never had auto-follow, which is what it was doing.
 *
 * Renaming it is the fix rather than implementing the claim, because the claim
 * is one this surface has already decided against: selecting a run for the
 * reader moves the overlay under them, which is the same loss as a reconcile
 * pass switching them into Replay. So the unselected state is now asserted
 * first — the count is offered and no overlay is drawn — and the pick is what
 * produces one. That ordering is the contract, and it can fail from both ends.
 */
test("Live advertises the newest run and overlays the one the reader picks", async ({ page, canvas }) => {
  const runId = "live-fixture-run";
  await seedRun(canvas, runId);
  await openPipeline(page, canvas);

  await expect(page.getByTestId("live-count")).toHaveAttribute("data-count", "1");
  await page.getByRole("tab", { name: /live, 1 run in progress/iu }).click();
  await expect(page.locator(".flow-card.overlay-running")).toHaveCount(0);
  await page.getByLabel("Live run").selectOption(runId);
  await expect(page.locator(".flow-card.overlay-running")).toBeVisible();
});

test("Run reconcile now stays busy until the pass finishes, then says what it did", async ({ page, canvas }) => {
  let finish = () => {};
  const held = new Promise((resolve) => {
    finish = resolve;
  });
  await page.route("**/intent", async (route) => {
    const body = route.request().postDataJSON();
    if (body?.kind !== "reconcile-now") {
      await route.continue();
      return;
    }
    await held;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: true, output: "no eligible work" }),
    });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();

  await page.getByRole("button", { name: "Run reconcile now" }).click();
  await expect(page.getByRole("button", { name: "Reconciling…" })).toBeDisabled();
  finish();
  await expect(page.getByRole("status")).toHaveText("Reconcile pass finished. It claimed no work for this pipeline.");
  await expect(page.getByRole("button", { name: "Run reconcile now" })).toBeEnabled();
});

/*
 * A pass report belongs to the visit that asked for it.
 *
 * `useLiveOverlay` is owned by `PipelineView`, so leaving Live only stops
 * rendering the controls — the hook, and the request it has out, survive the
 * trip. `dismissControls` withdrew the *rendered* result on the way out and
 * nothing withdrew the one still in flight, so a pass answered while the reader
 * was in Design installed its sentence anyway, and Live had a report waiting on
 * the way back for a pass this visit never ran.
 *
 * The run controls were already ticketed against exactly this; the pass had no
 * ticket of its own. It needs a separate one rather than a share of theirs,
 * because theirs is bumped on every change of selection and a pass is about the
 * pipeline, not the run being watched.
 */
test("a reconcile report does not follow the reader out of Live and back", async ({ page, canvas }) => {
  let finish = () => {};
  const held = new Promise((resolve) => {
    finish = resolve;
  });
  await page.route("**/intent", async (route) => {
    if (route.request().postDataJSON()?.kind !== "reconcile-now") {
      await route.continue();
      return;
    }
    await held;
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, output: "no eligible work" }) });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  await page.getByRole("button", { name: "Run reconcile now" }).click();
  await expect(page.getByRole("button", { name: "Reconciling…" })).toBeDisabled();

  await page.getByRole("tab", { name: /design/iu }).click();
  finish();
  await page.getByRole("tab", { name: /live/iu }).click();

  await expect(page.getByRole("status")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Run reconcile now" })).toBeEnabled();
});

/*
 * One press, one intent, and the transport comes back.
 *
 * A run control is a `bureau` invocation, not a form submission: two pauses are
 * two processes against one run, and the second is answered about a run the
 * first has already moved. A duplicate resume is the one that lies — the first
 * clears `PAUSE`, and the second is then refused about a run that is already
 * running, so the reader is told their request failed when it succeeded.
 *
 * The held render itself is asserted by the matrix, at both viewports, on all
 * three verbs. What only a round trip can show is the far end: the controls
 * have to come back when the answer arrives, or a slow host leaves the reader
 * holding a run they can no longer act on at all.
 */
test("a run control posts one intent per press and returns to rest", async ({ page, canvas }) => {
  const runId = "control-guard-run";
  let finish = () => {};
  const held = new Promise((resolve) => {
    finish = resolve;
  });
  let pauses = 0;
  await seedRun(canvas, runId);
  await page.route("**/intent", async (route) => {
    if (route.request().postDataJSON()?.kind !== "pause-run") {
      await route.continue();
      return;
    }
    pauses += 1;
    await held;
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true }) });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  await page.getByLabel("Live run").selectOption(runId);

  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Pausing…" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Cancel" })).toBeDisabled();
  finish();

  await expect(page.getByRole("button", { name: "Pause" })).toBeEnabled();
  expect(pauses).toBe(1);
});

/*
 * The hold is about the run, not about the visit.
 *
 * `useLiveOverlay` outlives Live, and `dismissControls` deliberately keeps the
 * selected run — so the invocation a held control stands for is still running
 * against that same run after a trip to Design. Clearing `controlBusy` there
 * re-armed the button over an intent still in flight, and the mode tabs are
 * never disabled, so two clicks bought a second `bureau` process against one
 * run: exactly the duplicate the guard at the top of `send` exists to refuse.
 *
 * The sibling hold already behaves this way and is asserted above — `reconciling`
 * survives the same trip because "the pass really is still running". This is the
 * run control's half of that invariant, from both ends: held on the way back,
 * and released when the answer finally lands.
 */
test("a held run control survives a trip to Design and takes no second intent", async ({ page, canvas }) => {
  const runId = "hold-across-modes-run";
  let finish = () => {};
  const held = new Promise((resolve) => {
    finish = resolve;
  });
  let pauses = 0;
  await seedRun(canvas, runId);
  await page.route("**/intent", async (route) => {
    if (route.request().postDataJSON()?.kind !== "pause-run") {
      await route.continue();
      return;
    }
    pauses += 1;
    await held;
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true }) });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  await page.getByLabel("Live run").selectOption(runId);
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Pausing…" })).toBeDisabled();

  await page.getByRole("tab", { name: /design/iu }).click();
  await page.getByRole("tab", { name: /live/iu }).click();

  // Still held on the way back, so the second press cannot be made at all.
  await expect(page.getByRole("button", { name: "Pausing…" })).toBeDisabled();
  await page.getByRole("button", { name: "Pausing…" }).click({ force: true });
  finish();
  await expect(page.getByRole("button", { name: "Pause" })).toBeEnabled();
  expect(pauses).toBe(1);
});

/*
 * A reply releases the hold it took, and no other.
 *
 * The hold's ticket is bumped on a change of selection and only there. Without
 * that bump a pause left in flight against run A would, when answered, release
 * the hold the reader has since taken out on run B — re-arming B's control over
 * B's own in-flight invocation and reintroducing the duplicate one run further
 * along. Selecting a run is the one event that genuinely changes the subject.
 */
test("an answer about the run that was left does not release the hold on the run selected since", async ({ page, canvas }) => {
  const [first, second] = ["hold-subject-a", "hold-subject-b"];
  const gates = new Map();
  const opened = new Map();
  for (const id of [first, second]) {
    await seedRun(canvas, id);
    gates.set(id, new Promise((resolve) => opened.set(id, resolve)));
  }
  await page.route("**/intent", async (route) => {
    const body = route.request().postDataJSON();
    if (body?.kind !== "pause-run") {
      await route.continue();
      return;
    }
    await gates.get(body.run_id);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true }) });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  await page.getByLabel("Live run").selectOption(first);
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Pausing…" })).toBeDisabled();

  // Changing the subject frees the new run's transport immediately.
  await page.getByLabel("Live run").selectOption(second);
  await expect(page.getByRole("button", { name: "Pause" })).toBeEnabled();
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Pausing…" })).toBeDisabled();

  // The first run's answer is not about this hold, so it must not lift it.
  opened.get(first)();
  await expect(page.getByRole("button", { name: "Pausing…" })).toBeDisabled();
  opened.get(second)();
  await expect(page.getByRole("button", { name: "Pause" })).toBeEnabled();
});

/*
 * The pass reports on itself, and moves nothing it did not start.
 *
 * A refused pass that still selected a run was drawing a paused run the reader
 * never asked for beside the sentence saying the pass could not be started —
 * two claims about the same click that cannot both be true.
 */
test("a refused reconcile pass moves nothing", async ({ page, canvas }) => {
  await seedRun(canvas, "already-running");
  await page.route("**/intent", async (route) => {
    if (route.request().postDataJSON()?.kind !== "reconcile-now") {
      await route.continue();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ok: false, error: "bureau binary not available" }),
    });
  });
  await openPipeline(page, canvas);
  await page.getByRole("tab", { name: /live/iu }).click();
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "available");

  await page.getByRole("button", { name: "Run reconcile now" }).click();

  await expect(page.locator(".run-control-error")).toHaveText("bureau binary not available");
  await expect(page.getByLabel("Live run")).toHaveValue("");
  await expect(page.getByTestId("run-activity")).toHaveAttribute("data-state", "available");
  await expect(page.locator(".flow-card.overlay-running")).toHaveCount(0);
});
