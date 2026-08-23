import { defineConfig, devices } from "@playwright/test";

/**
 * The canvas is a loopback page served by `serve.mjs`, so there is no base
 * URL here: the `canvas` fixture boots one host per worker on an ephemeral
 * port and hands the spec its address. Offline like every other suite —
 * no network, no `bureau` binary, no Copilot SDK.
 */
export default defineConfig({
  testDir: "./specs",
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
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
