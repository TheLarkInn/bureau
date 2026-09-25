import assert from "node:assert/strict";
import test from "node:test";

import { github } from "./maintenance-http.mjs";

test("forge reads are scoped, finite and reject redirects; deterministic reads have no credential", async () => {
  const requests = [];
  const api = github({ fetchImpl: async (url, options) => {
    requests.push({ url, options });
    return new Response("[]");
  } });
  assert.deepEqual(await api.issues(), []);
  assert.equal(requests[0].url, "https://api.github.com/repos/TheLarkInn/bureau/issues?state=all&per_page=100&page=1");
  assert.equal(requests[0].options.redirect, "error");
  assert.deepEqual([requests[0].options.headers.Authorization, requests[0].options.headers["Cache-Control"]],
    [undefined, "no-cache"]);
  await assert.rejects(api.request("GET", "/issues/1/../../other"), /unsupported/u);
  await assert.rejects(api.request("POST", "/issues", {}), /credential/u);
});

test("full pagination and oversized responses fail instead of trusting a partial inventory", async () => {
  let count = 0;
  const api = github({ fetchImpl: async () => {
    count += 1;
    return new Response(JSON.stringify(Array.from({ length: 100 }, (_, number) => ({ number }))));
  } });
  await assert.rejects(api.issues(), /five pages/u);
  assert.equal(count, 5);
  const large = github({ fetchImpl: async () => new Response(JSON.stringify("x".repeat(2 * 1024 * 1024))) });
  await assert.rejects(large.issue(7), /byte limit/u);
});

test("rate limits and write ambiguity are not silently retried by the transport", async () => {
  let count = 0;
  const api = github({ token: "fake-unit-token", fetchImpl: async () => {
    count += 1;
    return new Response("unavailable", { status: 429 });
  } });
  await assert.rejects(api.request("POST", "/issues", {}), /HTTP 429/u);
  assert.equal(count, 1);
});
