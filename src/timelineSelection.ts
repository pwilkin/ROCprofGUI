import type { TimelineGlyph } from "./timelineDetail";
import type { Mark } from "./components/Plots";
import { insideRect } from "./timelineLayout";

export function timelineHits(
  glyphs: TimelineGlyph[],
  overlays: TimelineGlyph[],
  x: number,
  y: number,
): Mark[] {
  const hits: Mark[] = [];
  const seen = new Set<string>();
  for (const layer of [overlays, glyphs]) {
    for (let i = layer.length - 1; i >= 0; i--) {
      const { mark, rect } = layer[i];
      if (insideRect(rect, x, y) && !seen.has(mark.key)) {
        seen.add(mark.key);
        hits.push(mark);
      }
    }
  }
  return hits;
}
