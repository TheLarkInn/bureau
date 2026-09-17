import { browserTools } from "../browser-tools.mjs";
import { onlyLocalRequests, temporaryPreview } from "../preview.mjs";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const { test: base, expect } = browserTools();
export { expect };

export async function screenshot(page, path, fullPage = false) {
  const height = await page.evaluate(() => document.documentElement.scrollHeight);
  expect(height, "full-page evidence stays below the 16000px height budget").toBeLessThanOrEqual(16_000);
  const image = await page.screenshot({ fullPage });
  expect(image.length, "each evidence image stays below 2 MiB").toBeLessThanOrEqual(2 * 1024 * 1024);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, image);
}

export const test = base.extend({
  siteBase: ["/bureau/", { option: true, scope: "worker" }],
  site: [async ({ siteBase }, use) => {
    const preview = await temporaryPreview(siteBase);
    try { await use(preview); }
    finally { await preview.close(); }
  }, { scope: "worker" }],
  page: async ({ page, context, site }, use) => {
    const errors = [];
    await onlyLocalRequests(context, new URL(site.url).origin, errors);
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(site.url, { waitUntil: "networkidle" });
    await use(page);
    expect(errors).toEqual([]);
  },
});
