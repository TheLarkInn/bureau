import { browserTools } from "./browser-tools.mjs";

const { defineConfig } = browserTools();

export default defineConfig({
  testDir: "./tests",
  testMatch: "**/*.spec.mjs",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: Boolean(process.env.CI),
  globalTimeout: 150_000,
  timeout: 30_000,
  expect: { timeout: 5000 },
  reporter: [["list"]],
  outputDir: "./test-results",
  use: {
    browserName: "chromium", viewport: { width: 1440, height: 960 },
    contextOptions: { reducedMotion: "reduce" },
    colorScheme: "light", locale: "en-US",
    timezoneId: "UTC", serviceWorkers: "block",
    screenshot: "only-on-failure", trace: "off", video: "off",
  },
  projects: [
    { name: "repository-path", use: { siteBase: "/bureau/" } },
    { name: "preview-root", use: { siteBase: "/" } },
  ],
});
