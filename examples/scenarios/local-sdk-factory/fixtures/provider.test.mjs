import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { runReview } from "../provider/review.mjs";
import { context, REVIEW, VERIFICATION } from "./provider-context.mjs";

test("standard metadata matches the declaration and has no invented native ceilings", async () => {
  const meta = JSON.parse(await readFile(new URL("../provider/factory.json", import.meta.url), "utf8"));
  assert.equal(meta.name, "review");
  assert.deepEqual(meta.phases.map(({ title }) => title), ["Review", "Verify"]);
  assert.equal(meta.argsSchema.additionalProperties, false);
  assert.equal(Object.hasOwn(meta, "limits"), false);
});

test("two bounded children produce a complete Derived v2 result", async () => {
  const ctx = context();
  const result = await runReview(ctx);
  assert.deepEqual(Object.keys(result).sort(), ["artifacts", "message", "outcome", "outputs", "schema", "trust"]);
  assert.deepEqual([result.schema, result.outcome, result.trust, result.artifacts], ["v2", "success", "derived", []]);
  assert.match(result.outputs.review, /Offline fixture/);
  assert.deepEqual(ctx.calls.map(({ options }) => options.label), ["review-v1", "verify-v1"]);
  assert.deepEqual(ctx.phases, ["Review", "Verify"]);
  assert.deepEqual(ctx.steps, ["review-result-v1"]);
  for (const call of ctx.calls) {
    assert.deepEqual(Object.keys(call.options).sort(), ["label", "schema"]);
    assert.match(call.prompt, /bureau-io\.get_step_context/);
  }
});

test("ordinary missing or malformed review output is failure, not success", async () => {
  const invalid = [
    null, undefined, {}, [], "prose",
    { ...REVIEW, needs_human: "false" },
    { ...REVIEW, review: " " },
    { ...REVIEW, review: "x".repeat(12001) },
    { ...REVIEW, extra: true },
  ];
  for (const response of invalid) {
    const ctx = context([response]);
    const result = await runReview(ctx);
    assert.deepEqual([result.outcome, ctx.calls.length, ctx.steps.length], ["failure", 1, 0]);
  }
});

test("a human prerequisite blocks before a verification child is admitted", async () => {
  const ctx = context([{ ...REVIEW, needs_human: true }]);
  const result = await runReview(ctx);
  assert.deepEqual([result.outcome, ctx.calls.length], ["blocked", 1]);
});

test("missing, malformed or negative verification cannot complete successfully", async () => {
  const invalid = [
    null, {}, "prose",
    { ...VERIFICATION, verified: "true" },
    { ...VERIFICATION, verified: false },
    { ...VERIFICATION, reason: "" },
    { ...VERIFICATION, reason: "x".repeat(4001) },
    { ...VERIFICATION, extra: true },
  ];
  for (const response of invalid) {
    const ctx = context([REVIEW, response]);
    const result = await runReview(ctx);
    assert.deepEqual([result.outcome, ctx.calls.length, ctx.steps.length], ["failure", 2, 0]);
  }
});

test("invalid SDK arguments reject before any child work", async () => {
  for (const args of [undefined, [], "text", 2, { extra: true }, { focus: null }, { focus: " " }, { focus: "x".repeat(1001) }]) {
    const ctx = context([]);
    ctx.args = args;
    await assert.rejects(runReview(ctx), TypeError);
    assert.equal(ctx.calls.length, 0);
  }
});

test("null, empty and explicit focus arguments retain the fixed delegation bound", async () => {
  for (const args of [null, {}, { focus: "Check the supplied interface contract." }]) {
    const ctx = context([REVIEW, VERIFICATION], args);
    const result = await runReview(ctx);
    assert.deepEqual([result.outcome, ctx.calls.length], ["success", 2]);
  }
});

test("hard SDK errors propagate instead of becoming a success-shaped return", async () => {
  const failure = new Error("factory_limit_reached");
  const ctx = context([failure]);
  await assert.rejects(runReview(ctx), (error) => error === failure);
  assert.equal(ctx.calls.length, 1);
});

test("cancellation prevents startup and stops between children", async () => {
  const before = context();
  before.controller.abort();
  await assert.rejects(runReview(before), { name: "AbortError" });
  assert.equal(before.calls.length, 0);
  const during = context();
  const agent = during.agent;
  during.agent = async (...args) => {
    const response = await agent(...args);
    during.controller.abort();
    return response;
  };
  await assert.rejects(runReview(during), { name: "AbortError" });
  assert.equal(during.calls.length, 1);
});

test("a durable result-write failure is not swallowed", async () => {
  const ctx = context();
  const failure = new Error("factory storage unavailable");
  ctx.step = async () => { throw failure; };
  await assert.rejects(runReview(ctx), (error) => error === failure);
});
