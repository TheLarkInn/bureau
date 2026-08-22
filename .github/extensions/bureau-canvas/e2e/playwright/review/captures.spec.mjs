import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { test, expect } from "../fixtures.mjs";

const REVIEW_DIR = fileURLToPath(
  new URL("../../../../../../.impeccable/review/", import.meta.url),
);

const captures = [
  ["desktop", { width: 1440, height: 1000 }],
  ["mobile", { width: 390, height: 844 }],
];

for (const [name, viewport] of captures) {
  test(`captures the pipeline editor at ${name} width`, async ({ editor }) => {
    await mkdir(REVIEW_DIR, { recursive: true });
    await editor.page.setViewportSize(viewport);
    await expect(editor.page.locator(".editor-flow")).toBeVisible();
    await editor.page.evaluate(() => document.fonts.ready.then(() => true));
    await editor.page.screenshot({
      path: join(REVIEW_DIR, `${name}.png`),
      fullPage: false,
    });
    expect(editor.errors).toEqual([]);
  });
}
