import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { parse } from "../.github/extensions/bureau-canvas/lib/vendor/yaml.mjs";
import { targets } from "./release-assets.mjs";

async function workflow(name) {
  return parse(await readFile(new URL(`../.github/workflows/${name}.yml`, import.meta.url), "utf8"));
}

test("one CI gate includes lint, all browser suites, and every native architecture", async () => {
  const ci = await workflow("ci");
  assert.deepEqual(ci.jobs.checks.needs, ["source", "lint", "matrix", "visual", "binaries"]);
  assert.equal(ci.jobs.checks.if, "always()");
  for (const name of ["lint", "matrix", "visual", "binaries"]) {
    assert.deepEqual(ci.jobs[name].needs, "source");
    assert.equal(ci.jobs[name].with.ref, "${{ needs.source.outputs.sha }}");
  }
  assert.ok(ci.on.pull_request && ci.on.push && ci.on.workflow_dispatch && ci.on.schedule);
});

test("reusable workflow callers supply only declared inputs and all required inputs", async () => {
  const ci = await workflow("ci");
  for (const job of Object.values(ci.jobs).filter((job) => job.uses)) {
    const name = job.uses.split("/").at(-1).replace(".yml", "");
    const inputs = (await workflow(name)).on.workflow_call.inputs;
    assert.ok(Object.keys(job.with).every((name) => name in inputs));
    assert.ok(Object.entries(inputs).filter(([, input]) => input.required).every(([name]) => name in job.with));
  }
});

test("publishing and automatic merging cannot run before the complete gate", async () => {
  const { jobs } = await workflow("ci");
  for (const name of ["release", "merge-release"]) assert.ok(jobs[name].needs.includes("checks"));
  assert.match(jobs.release.if, /github\.ref == 'refs\/heads\/main'/);
  assert.match(jobs["merge-release"].if, /workflow_dispatch/);
  assert.equal(jobs.checks.permissions.checks, "write");
  assert.ok(jobs.checks.steps.some((step) => step.with?.script?.includes("head_sha: process.env.TESTED_SHA")));
});

test("platform matrix and publisher agree, and each platform runs offline tests", async () => {
  const build = (await workflow("release-build")).jobs.build;
  assert.deepEqual(build.strategy.matrix.include.map(({ target }) => target).sort(), targets);
  assert.ok(build.steps.some(({ run }) => run?.includes('cargo test --locked --offline --target "$TARGET"')));
  assert.ok(build.steps.some(({ run }) => run?.includes("package-release.sh")));
  assert.ok(build.steps.some(({ with: inputs }) => inputs?.["if-no-files-found"] === "error"));
});

test("untrusted test jobs have no repository write token", async () => {
  for (const name of ["rust-lints", "canvas-state-matrix", "canvas-visual-regression", "release-build"]) {
    const file = await workflow(name);
    assert.deepEqual(file.permissions, { contents: "read" });
    const jobs = Object.values(file.jobs);
    for (const { steps } of jobs) {
      const checkout = steps.find(({ uses }) => uses?.startsWith("actions/checkout@"));
      assert.equal(checkout.with["persist-credentials"], false);
    }
  }
});

test("release uploads same-run artifacts before publishing, without a release-event trigger", async () => {
  const release = await workflow("release");
  assert.deepEqual(Object.keys(release.on), ["workflow_call"]);
  const steps = release.jobs.release.steps;
  const download = steps.findIndex(({ uses }) => uses?.startsWith("actions/download-artifact@"));
  const draft = steps.findIndex(({ with: inputs }) => inputs?.command === "release");
  const publish = steps.findIndex(({ with: inputs }) => inputs?.script?.includes("await publish("));
  assert.ok(download < draft && draft < publish);
  assert.equal(steps[download].with["run-id"], undefined);
  assert.equal(release.concurrency["cancel-in-progress"], false);
});

test("release-plz always gets a named branch at the immutable tested commit", async () => {
  const { steps } = (await workflow("release")).jobs.release;
  let attached = false;
  for (const step of steps) {
    if (step.uses?.startsWith("actions/checkout@")) attached = false;
    if (step.run?.includes('git switch --force-create main "$TESTED_SHA"')) {
      assert.equal(step.env.TESTED_SHA, "${{ inputs.ref }}");
      assert.match(step.run, /--set-upstream-to=origin\/main main/);
      attached = true;
    }
    if (step.uses?.startsWith("release-plz/action@")) assert.equal(attached, true);
  }
});
