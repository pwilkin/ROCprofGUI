import { timelineRect, type Rect, type MipGeometry } from "./timelineLayout";
import type { Mark } from "./components/Plots";
import type { Range } from "./types";
export interface TimelineGlyph {
  mark: Mark;
  rect: Rect;
}
export function timelineGlyphs(
  marks: Mark[],
  lanes: string[],
  range: Range,
  width: number,
  laneHeight: number,
  stacked: boolean,
  detailPixels: number,
  mip?: MipGeometry,
): TimelineGlyph[] {
  const rows = new Map(lanes.map((lane, index) => [lane, index]));
  const glyphs: TimelineGlyph[] = [],
    selected: TimelineGlyph[] = [];
  const buckets = new Map<string, TimelineGlyph>();
  for (const mark of marks) {
    const row = rows.get(mark.lane);
    if (row === undefined) continue;
    const slot = stacked ? (mark.displaySlot ?? 0) : 0;
    const rect = timelineRect(
      mark.start,
      mark.end,
      row,
      slot,
      range,
      width,
      laneHeight,
      mip,
    );
    if (!rect || rect.width <= 0) continue;
    if (!detailPixels) {
      glyphs.push({ mark, rect });
      continue;
    }
    if (mark.selected) {
      selected.push({ mark, rect });
      continue;
    }
    const projected =
      ((mark.end - mark.start) / (range[1] - range[0])) * (width - 169);
    if (projected > detailPixels) {
      glyphs.push({ mark, rect });
      continue;
    }
    const key = JSON.stringify([
      row,
      slot,
      mark.color,
      Math.floor((rect.x - 155) / detailPixels),
    ]);
    const previous = buckets.get(key);
    if (!previous) {
      const glyph = { mark: { ...mark }, rect };
      buckets.set(key, glyph);
      glyphs.push(glyph);
    } else {
      const count = (previous.mark.aggregateCount || 1) + 1;
      previous.mark = {
        ...previous.mark,
        start: Math.min(previous.mark.start, mark.start),
        end: Math.max(previous.mark.end, mark.end),
        aggregateCount: count,
        label: `${count} short events · aggregated span; click to zoom`,
      };
      previous.rect = timelineRect(
        previous.mark.start,
        previous.mark.end,
        row,
        slot,
        range,
        width,
        laneHeight,
        mip,
      )!;
    }
  }
  return [...glyphs, ...selected];
}
