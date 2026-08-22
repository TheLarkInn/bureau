import { defineConfig, devices } from "@playwright/test";

/**
 * The canvas is a loopback page served by `serve.mjs`, so there is no base
 * URL here: the `canvas` fixture boots one host per worker on an ephemeral
 * port and hands the spec its address. Offline like every other suite —
 * no network, no `bureau` binary, no Copilot SDK.
 *
 * `globalSetup` empties the render gallery, so the artefact a reviewer browses
 * is exactly the states this run produced.
 */
export default defineConfig({
  testDir: "./specs",
  globalSetup: "./global-setup.mjs",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 2 : undefined,
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
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
