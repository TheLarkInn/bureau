import { defineConfig, devices } from "@playwright/test";

/**
 * The canvas is a loopback page served by `serve.mjs`, so there is no base
 * URL here: the `canvas` fixture boots one host per worker on an ephemeral
 * port and hands the spec its address. Offline like every other suite —
 * no network, no `bureau` binary, no Copilot SDK.
 *
 * `globalSetup` opens a staging directory for this run's renders and
 * `globalTeardown` publishes it over the gallery — but only when the run put
 * something there, so the artefact a reviewer browses is exactly the states the
 * last matrix run produced, and a run that renders none of them leaves it
 * alone.
 *
 * The gallery's own audit is a *teardown project* rather than part of that
 * hook. Both can fail a run — a `globalTeardown` that throws does exit
 * non-zero, which was measured rather than assumed — but a hook failure is an
 * error attached to no test: the reporter says "1 error was not a part of any
 * test", CI lists no failing check, and the thing that found the defect is
 * nowhere in the run's own record of what it checked. A teardown project runs
 * at the same point, after every worker in the project it is attached to, and
 * its assertions are ordinary named test failures. In a change whose subject is
 * making the harness's marks answerable as checks, the audit ought to be one.
 * The hook stays for the runs where the audit spec is filtered out, and is a
 * no-op once the spec has published.
 */
export default defineConfig({
  testDir: "./specs",
  globalSetup: "./global-setup.mjs",
  globalTeardown: "./global-teardown.mjs",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : 4,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : [["list"]],
  timeout: 30_000,
  expect: { timeout: 5_000 },
  use: {
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    viewport: { width: 1280, height: 900 },
    colorScheme: "light",
    locale: "en-US",
    reducedMotion: "reduce",
    timezoneId: "UTC",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
      testIgnore: /gallery\.audit\.spec\.mjs/u,
      teardown: "gallery",
    },
    {
      // `retries: 0`, and not by omission. Publishing is a rename, so a retry
      // finds staging already gone, reads that as "this run rendered nothing"
      // and passes — a real finding turned green by being asked a second time,
      // which is the same vacuous pass the spec itself is written against.
      name: "gallery",
      testMatch: /gallery\.audit\.spec\.mjs/u,
      retries: 0,
    },
  ],
});
