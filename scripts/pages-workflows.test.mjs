import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { parse } from "../.github/extensions/bureau-canvas/lib/vendor/yaml.mjs";

async function workflow(name) {
  return parse(await readFile(new URL(`../.github/workflows/${name}.yml`, import.meta.url), "utf8"));
}

test("site validation participates in the existing release-blocking CI aggregate", async () => {
  const { jobs } = await workflow("ci");
  assert.equal(jobs.site.uses, "./.github/workflows/site-checks.yml");
  assert.equal(jobs.site.needs, "source");
  assert.equal(jobs.site.with.ref, "${{ needs.source.outputs.sha }}");
  assert.ok(jobs.checks.needs.includes("site"));
  assert.ok(jobs.release.needs.includes("checks"));
});

test("Pages PR validation is read-only and never receives secrets or persisted credentials", async () => {
  for (const name of ["pages", "site-checks"]) {
    const file = await workflow(name);
    assert.deepEqual(file.permissions, { contents: "read" });
    assert.equal(file.on.pull_request_target, undefined);
    for (const [jobName, job] of Object.entries(file.jobs)) {
      assert.equal(job.secrets, undefined);
      if (jobName !== "deploy") assert.ok(!job.permissions || Object.values(job.permissions).every((value) => value === "read"));
      for (const step of job.steps ?? []) {
        if (step.uses?.startsWith("actions/checkout@")) assert.equal(step.with["persist-credentials"], false);
      }
    }
  }
});

test("deploy only follows checked main and rechecks exact source without checking out code", async () => {
  const { jobs, on, concurrency } = await workflow("pages");
  assert.deepEqual(on.push.branches, ["main"]);
  assert.ok(Object.hasOwn(on, "workflow_dispatch"));
  assert.equal(concurrency["cancel-in-progress"], true);
  assert.deepEqual(jobs.deploy.needs, ["source", "validate"]);
  for (const condition of ["github.repository == 'TheLarkInn/bureau'", "github.ref == 'refs/heads/main'", "github.event_name == 'push'", "github.event_name == 'workflow_dispatch'"]) {
    assert.ok(jobs.deploy.if.includes(condition));
  }
  assert.deepEqual(jobs.deploy.permissions, { contents: "read", pages: "write", "id-token": "write" });
  const steps = jobs.deploy.steps;
  assert.equal(steps.some(({ uses }) => uses?.startsWith("actions/checkout@")), false);
  assert.match(steps[0].with.script, /data\.object\.sha !== process\.env\.TESTED_SHA/u);
  assert.match(steps[0].with.script, /context\.sha !== process\.env\.TESTED_SHA/u);
});

test("Pages configuration never enables a site or changes its domain", async () => {
  const { jobs } = await workflow("pages");
  const configure = jobs.deploy.steps.find(({ uses }) => uses?.startsWith("actions/configure-pages@"));
  assert.equal(configure.with.enablement, false);
  assert.equal(jobs.deploy.environment.name, "github-pages");
  assert.equal(jobs.deploy.steps.at(-1).uses, "actions/deploy-pages@v4.0.5");
  assert.doesNotMatch(JSON.stringify(jobs), /CNAME|cname|custom.domain|secrets\./u);
});

test("reusable site checks pin source and enforce real bounded offline browser evidence", async () => {
  const file = await workflow("site-checks");
  const job = file.jobs.site;
  assert.equal(job["runs-on"], "ubuntu-24.04");
  assert.equal(job.defaults.run.shell, "bash", "explicit bash preserves pipefail for JSON evidence through tee");
  assert.ok(job["timeout-minutes"] <= 15);
  const checkout = job.steps.find(({ uses }) => uses?.startsWith("actions/checkout@"));
  assert.equal(checkout.with.ref, "${{ inputs.ref }}");
  for (const command of ["node site/test.mjs", "node site/verify-links.mjs", "node site/browser.mjs", "node site/check.mjs --kind accessibility --json", "node site/check.mjs --kind responsive --json"]) {
    assert.ok(job.steps.some(({ run }) => run?.includes(command)));
  }
  assert.ok(job.steps.some(({ run }) => run?.includes("install --with-deps chromium")));
  assert.equal(file.on.workflow_call.inputs.upload_pages.default, false);
});

test("artifacts are same-run, narrowly scoped, finite retention, and never uploaded for a PR deployment", async () => {
  const { steps } = (await workflow("site-checks")).jobs.site;
  const upload = steps.find(({ uses }) => uses?.startsWith("actions/upload-pages-artifact@"));
  assert.equal(upload.with.path, "site/dist");
  assert.equal(upload.with["retention-days"], 1);
  assert.match(upload.if, /inputs\.upload_pages/u);
  assert.match(upload.if, /github\.ref == 'refs\/heads\/main'/u);
  assert.match(upload.if, /github\.event_name == 'push'/u);
  const failure = steps.find(({ uses }) => uses?.startsWith("actions/upload-artifact@"));
  assert.equal(failure.if, "failure()");
  assert.ok(failure.with["retention-days"] <= 7);
  assert.equal(failure.with.path.trim(), "site/test-results/\nsite/.cache/evidence/");
});

test("reusable callers provide exactly declared site inputs and preserve CI's no-upload default", async () => {
  const called = await workflow("site-checks");
  const inputs = called.on.workflow_call.inputs;
  for (const [file, jobName] of [["ci", "site"], ["pages", "validate"]]) {
    const job = (await workflow(file)).jobs[jobName];
    assert.ok(Object.keys(job.with).every((name) => name in inputs));
    assert.ok(Object.entries(inputs).filter(([, input]) => input.required).every(([name]) => name in job.with));
  }
  assert.equal((await workflow("ci")).jobs.site.with.upload_pages, undefined);
});
