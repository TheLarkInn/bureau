import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, SAMPLE, test } from "../fixtures.mjs";

const FACTORY = JSON.parse(await readFile(new URL("../../../test/fixtures/copilot-factory.json", import.meta.url), "utf8"));

async function configure(page) {
  await page.locator('[data-ref="implement"]').click();
  await page.getByLabel("Use a local Copilot factory").check();
  for (const [label, key] of [
    ["Factory name", "name"], ["Provider source", "extension"], ["Approved provider digest", "extension_digest"],
    ["Model credential reference", "model_credential"],
  ]) {
    await page.getByLabel(label, { exact: true }).fill(FACTORY[key]);
  }
  await page.getByText("Qualified runtime and SDK", { exact: true }).click();
  const profile = page.getByLabel("Factory SDK capability profile", { exact: true });
  await expect(profile).toHaveValue("copilot-sdk-factory-v1");
  await expect(profile).toHaveAttribute("readonly", "");
  for (const [label, key] of [
    ["Qualified runtime directory", "directory"], ["Approved runtime digest", "digest"],
    ["Exact runtime version", "version"], ["Host executable", "executable"],
    ["CLI entrypoint (optional)", "cli"], ["CLI distribution directory", "dist"],
  ]) {
    await page.getByLabel(label, { exact: true }).fill(FACTORY.runtime[key]);
  }
}

test("factory configuration remains structured, blocks invalid JSON, and can return to ACP", async ({ editor }) => {
  const page = editor.page;
  await configure(page);
  const args = page.getByLabel("Static factory arguments", { exact: true });
  await args.fill('{"literal":"verify","nested":{"count":3}}');
  await expect(page.getByRole("button", { name: "Save changes" })).toBeEnabled();
  await args.fill("{");
  await expect(args).toHaveValue("{");
  await expect(page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  await args.fill('{"literal":"verify","nested":{"count":3}}');
  await expect(page.getByRole("button", { name: "Save changes" })).toBeEnabled();
  await page.getByLabel("Use a local Copilot factory").uncheck();
  await expect(args).toHaveCount(0);
  expect(editor.errors).toEqual([]);
});

test("factory editor never inserts native ceilings merely by opening their controls", async ({ editor }) => {
  await configure(editor.page);
  await editor.page.getByText("Native ceilings (optional)", { exact: true }).click();
  for (const name of ["Concurrent subagents", "Total subagents", "Cumulative active seconds", "Cumulative AI credits (soft)"]) {
    await expect(editor.page.getByLabel(name, { exact: true })).toHaveValue("");
  }
  await editor.page.getByLabel("Total subagents", { exact: true }).fill("0");
  await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  await editor.page.getByLabel("Total subagents", { exact: true }).fill("3");
  await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeEnabled();
  const concurrent = editor.page.getByLabel("Concurrent subagents", { exact: true });
  await expect(concurrent).toHaveAttribute("max", "500");
  await concurrent.fill("501");
  await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeDisabled();
  await expect(editor.page.getByLabel("Factory configuration issues")).toContainText("no greater than 500");
  await concurrent.fill("500");
  await expect(editor.page.getByRole("button", { name: "Save changes" })).toBeEnabled();
});

async function seedFactory(canvas, status, interrupted) {
  const runId = `factory-${status}${interrupted ? "-interrupted" : ""}`;
  const sessionId = `sdk-${runId}`;
  const nativeId = `native-${runId}`;
  const fact = (data) => ({ kind: "copilot_factory", data: { session_id: sessionId, ...data } });
  const events = [
    { kind: "run_started", data: { run_id: runId, assignment: SAMPLE.assignment,
      snapshot: { pipeline: { name: "agent-eligible-pipeline" } } } },
    { kind: "step_started", data: { step: "implement" } },
    fact({ event: "prepared", intent: { session_id: sessionId, step: "implement", factory: FACTORY,
      workspace: { directory: "/private/preserved-worktree" } } }),
    fact({ event: "runtime_opened", purpose: "execution" }),
    fact({ event: "session_accepted" }),
    fact({ event: "dispatch", operation: "start" }),
    fact({ event: "accepted", run_id: nativeId, attempt: 1 }),
    fact({ event: "observed", run: { runId: nativeId, status, ...(interrupted ? { reason: "interrupted" } : {}) },
      summary: { runId: nativeId, factoryName: FACTORY.name, status, canResume: status === "paused",
        consumed: { nanoAiu: 2e9, activeMs: 1500, subagents: 2 }, terminal: null } }),
    fact({ event: "runtime_closed", purpose: "execution", clean: true, message: "closed" }),
  ].map((event, seq) => ({ ...event, seq, at_ms: 1_800_000_000_000 + seq * 1000 }));
  await mkdir(join(canvas.runs, runId));
  await writeFile(join(canvas.runs, runId, "events.jsonl"), `${events.map((event) => JSON.stringify(event)).join("\n")}\n`);
  return { runId, sessionId, nativeId };
}

for (const [status, interrupted] of [["paused", false], ["error", true]]) {
  test(`Live distinguishes native ${interrupted ? "interrupted" : status} from Bureau completion`, async ({ page, canvas }) => {
    const ids = await seedFactory(canvas, status, interrupted);
    await page.route(`**/runs/${ids.runId}/controls`, (route) => route.fulfill({ json: {
      run_id: ids.runId, local_factory_resume: { session_id: ids.sessionId, event_seq: 8,
        allowed: !interrupted, reason: interrupted ? "Runtime was interrupted." : null },
    } }));
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    await page.getByRole("tab", { name: /live, 1 run in progress/iu }).click();
    await page.getByLabel("Live run").selectOption(ids.runId);
    const details = page.getByTestId("local-factory-details");
    await expect(details).toContainText(ids.sessionId);
    await expect(details).toContainText(ids.nativeId);
    await expect(details).toContainText("/private/preserved-worktree");
    await expect(page.getByTestId("run-resume")).toHaveCount(interrupted ? 0 : 1);
    if (interrupted) await expect(details).toContainText("hard-crashed factories cannot resume");
  });
}

test("Live offers an unadmitted bootstrap continuation only with matching Bureau authority", async ({ page, canvas }) => {
  const ids = await seedFactory(canvas, "paused", false);
  const path = join(canvas.runs, ids.runId, "events.jsonl");
  const events = (await readFile(path, "utf8")).trim().split("\n").map((line) => JSON.parse(line));
  const bootstrap = [...events.slice(0, 5), events.at(-1)];
  bootstrap.push({ kind: "output", data: { stream: "run", data: "paused at a step boundary" } });
  const trace = bootstrap.map((event, seq) => ({ ...event, seq, at_ms: 1_800_000_000_000 + seq * 1000 }));
  await writeFile(path, `${trace.map((event) => JSON.stringify(event)).join("\n")}\n`);
  let eligible = false;
  await page.route(`**/runs/${ids.runId}/controls`, (route) => route.fulfill({ json: {
    run_id: ids.runId, local_factory_resume: { session_id: ids.sessionId, event_seq: 5,
      allowed: eligible, reason: eligible ? null : "Bootstrap evidence needs inspection." },
  } }));
  await page.goto(canvas.url);
  await page.locator(".assignment-head").first().click();
  await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
  await page.getByRole("tab", { name: /live, 1 run in progress/iu }).click();
  await page.getByLabel("Live run").selectOption(ids.runId);
  await expect(page.getByText("Bootstrap evidence needs inspection.", { exact: true })).toBeVisible();
  await expect(page.getByTestId("run-resume")).toHaveCount(0);
  eligible = true;
  await page.getByLabel("Live run").selectOption("");
  await page.getByLabel("Live run").selectOption(ids.runId);
  await expect(page.getByTestId("run-resume")).toHaveCount(1);
  await expect(page.getByTestId("local-factory-details")).toContainText("not admitted");
});
