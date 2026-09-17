import { fileURLToPath } from "node:url";
import { loadTool, requireToolVersion, resolveTool } from "./tool-paths.mjs";

export const playwrightConfig = fileURLToPath(new URL("./playwright.config.mjs", import.meta.url));

export function browserTools() {
  try {
    requireToolVersion("browser", "@playwright/test");
    return loadTool("browser", "@playwright/test");
  } catch (error) {
    if (error.code !== "MODULE_NOT_FOUND") throw error;
    throw new Error("Playwright is missing. Provision the pinned canvas Playwright package locally or through BUREAU_SITE_TOOLS before checking the site.", { cause: error });
  }
}

export function browserCli() {
  browserTools();
  return resolveTool("browser", "@playwright/test/cli");
}

export function axeSource() {
  try {
    requireToolVersion("audit", "axe-core");
    return loadTool("audit", "axe-core").source;
  } catch (error) {
    if (error.code !== "MODULE_NOT_FOUND") throw error;
    throw new Error("axe-core is missing. Provision the pinned site package locally or through BUREAU_SITE_TOOLS before checking accessibility.", { cause: error });
  }
}
