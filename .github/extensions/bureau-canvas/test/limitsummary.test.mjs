// The limits summary shown at rest. Its job is to be true at both extremes:
// it must never call a bounded assignment unbounded, and must never let an
// uncapped limit pass silently.
//
// The component lives in `web/app.mjs`, which imports React, so the rule it
// encodes is restated here against the same field table.

import assert from "node:assert/strict";
import test from "node:test";

/** Mirrors `LIMIT_FIELDS` in web/app.mjs. */
const LIMIT_FIELDS = [
  { key: "max_concurrent", short: "at once" },
  { key: "max_runs_per_hour", short: "/hour" },
  { key: "max_runs_per_day", short: "/day" },
  { key: "max_open_prs", short: "open PRs" },
  { key: "max_cost_per_day_usd", short: "USD/day" },
  { key: "max_run_hours", short: "h/run", defaulted: 24 },
];

const isSet = (limits, key) => limits[key] !== null && limits[key] !== undefined;

/** The chip labels `LimitsSummary` renders, in order. */
function summary(limits) {
  const capped = LIMIT_FIELDS.filter((field) => isSet(limits, field.key));
  const uncapped = LIMIT_FIELDS.filter((field) => !isSet(limits, field.key) && !field.defaulted);
  if (!capped.length) {
    return ["unbounded — no limits set"];
  }
  return [
    ...capped.map((field) => `${limits[field.key]} ${field.short}`),
    ...(uncapped.length ? [`${uncapped.length} unlimited`] : []),
    ...(isSet(limits, "max_run_hours") ? [] : ["24h/run default"]),
  ];
}

const NONE = {
  max_concurrent: null, max_runs_per_hour: null, max_runs_per_day: null,
  max_open_prs: null, max_cost_per_day_usd: null, max_run_hours: null,
};

test("an assignment with nothing capped says so outright", () => {
  assert.deepEqual(summary(NONE), ["unbounded — no limits set"]);
});

test("a run-length limit alone is not reported as unbounded", () => {
  // Regression: the collapse test once ignored `max_run_hours`, so setting
  // only that one still claimed no limits were set.
  assert.deepEqual(summary({ ...NONE, max_run_hours: 8 }), ["8 h/run", "5 unlimited"]);
});

test("the committed block lists its caps, the uncapped count, and the default", () => {
  const limits = { ...NONE, max_concurrent: 1, max_runs_per_hour: 4 };

  assert.deepEqual(summary(limits), ["1 at once", "4 /hour", "3 unlimited", "24h/run default"]);
});

test("a fully bounded block shows no warning chip at all", () => {
  const limits = {
    max_concurrent: 2, max_runs_per_hour: 6, max_runs_per_day: 40,
    max_open_prs: 5, max_cost_per_day_usd: 25, max_run_hours: 12,
  };
  const chips = summary(limits);

  assert.deepEqual(
    { count: chips.length, warns: chips.some((chip) => chip.includes("unlimited") || chip.includes("unbounded")) },
    { count: 6, warns: false },
  );
});

test("run length is never counted among the unlimited", () => {
  const chips = summary({ ...NONE, max_concurrent: 1 });

  assert.deepEqual(chips, ["1 at once", "4 unlimited", "24h/run default"]);
});
