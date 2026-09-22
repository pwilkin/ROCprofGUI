import { expect, test } from "@playwright/test";

test("timeline paint state reuse preserves overlapping and selected event pixels", async ({ page }) => {
  await page.goto("/");
  const result = await page.evaluate(async () => {
    const { paintTimelineGlyphs } = await import(/* @vite-ignore */ "/src/timelinePaint.ts");
    const glyphs = Array.from({ length: 1200 }, (_, i) => ({
      mark: {
        color: ["#a054dc", "#a054dc", "#23b78b80", "invalid", "#f49342"][i % 5],
        selected: i % 17 === 0,
      },
      rect: { x: (i % 170) * 0.9, y: (i % 4) * 7.25, width: 2.3, height: 16.5 },
    }));
    const overlays = glyphs.filter((_, i) => i % 71 === 0);
    const results = [];
    for (const ratio of [1, 1.25, 2]) {
      const canvases = [document.createElement("canvas"), document.createElement("canvas")];
      const contexts = canvases.map((canvas) => {
        canvas.width = 200 * ratio;
        canvas.height = 60 * ratio;
        const ctx = canvas.getContext("2d")!;
        ctx.scale(ratio, ratio);
        ctx.lineWidth = 1 / ratio;
        ctx.fillStyle = "#112233";
        ctx.fillRect(0, 0, 200, 60);
        return ctx;
      });
      const ctx = contexts[0];
      for (const group of [glyphs, overlays]) {
        for (const { mark, rect } of group) {
          ctx.fillStyle = mark.color;
          ctx.globalAlpha = mark.selected || group === overlays ? 1 : 0.75;
          ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
          if (mark.selected) {
            ctx.strokeStyle = "#ffffff";
            ctx.strokeRect(rect.x, rect.y, rect.width, rect.height);
          }
          ctx.globalAlpha = 1;
        }
      }
      paintTimelineGlyphs(contexts[1], glyphs, overlays, () => "#ffffff");
      const pixels = contexts.map((c) => c.getImageData(0, 0, canvases[0].width, canvases[0].height).data);
      results.push({ ratio, equal: pixels[0].every((value, index) => value === pixels[1][index]), alpha: contexts[1].globalAlpha });
    }
    return results;
  });
  expect(result).toEqual([1, 1.25, 2].map((ratio) => ({ ratio, equal: true, alpha: 1 })));
});
