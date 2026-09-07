import { expect, test } from "../fixtures.mjs";

for (const surface of ["viewer", "editor"]) {
  test(`${surface} exit captions sit on their own rendered handoff curves`, async ({ page, canvas }) => {
    await page.goto(canvas.url);
    await page.locator(".assignment-head").first().click();
    await page.getByRole("button", { name: "Open pipeline agent-eligible-pipeline" }).click();
    if (surface === "editor") {
      await page.getByRole("link", { name: "Edit" }).click();
    }
    await page.getByRole("tab", { name: /^graph$/iu }).click();
    const captions = page.locator('.edge-caption[data-edge-id*="terminal:"]');
    await expect(captions).toHaveCount(8);
    await expect.poll(() => captions.evaluateAll(detachedCaptions)).toEqual([]);
  });
}

function detachedCaptions(labels) {
  return labels.flatMap((label) => {
    const id = label.dataset.edgeId;
    const edge = [...document.querySelectorAll(".react-flow__edge")].find((node) => node.dataset.id === id);
    const path = edge?.querySelector(".react-flow__edge-path");
    const matrix = path?.getScreenCTM();
    if (!matrix) return [id];
    const box = label.getBoundingClientRect();
    const length = path.getTotalLength();
    let distance = Infinity;
    for (let index = 0; index <= 1200; index += 1) {
      const point = path.getPointAtLength(length * index / 1200).matrixTransform(matrix);
      distance = Math.min(distance, Math.hypot(point.x - box.x - box.width / 2, point.y - box.y - box.height / 2));
    }
    return distance > 2 ? [`${id}: ${distance.toFixed(1)}px from its curve`] : [];
  });
}
