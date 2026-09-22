import type { Mark } from "./components/Plots";
import type { Range } from "./types";
import type { TimelineGlyph } from "./timelineDetail";
export function timelineOverlays(
  marks: Mark[],
  lanes: string[],
  range: Range,
  width: number,
  laneHeight: number,
): TimelineGlyph[] {
  if (width <= 169 || range[1] <= range[0]) return [];
  const laneEngines = new Map<string, number>();
  for (const mark of marks) {
    if (
      (mark.recordKind === "residency" ||
        mark.recordKind === "marker" ||
        (mark.waveId !== undefined && mark.tokenIndex !== undefined)) &&
      mark.shaderEngine !== undefined
    )
      laneEngines.set(mark.lane, mark.shaderEngine);
  }
  const spans = new Map<number, { start: number; end: number }[]>();
  lanes.forEach((lane, row) => {
    const se = laneEngines.get(lane);
    if (se === undefined) return;
    const rows = spans.get(se) || [];
    const last = rows.at(-1);
    if (last?.end === row) last.end++;
    else rows.push({ start: row, end: row + 1 });
    spans.set(se, rows);
  });
  const result: TimelineGlyph[] = [];
  for (const mark of marks) {
    if (
      mark.recordKind !== "event" ||
      mark.shaderEngine === undefined ||
      !mark.overlayWidth ||
      mark.start < range[0] ||
      mark.start > range[1]
    )
      continue;
    const rows = spans.get(mark.shaderEngine);
    if (!rows) continue;
    const size = Math.min(mark.overlayWidth, width - 169);
    const x = Math.max(
      155,
      Math.min(
        width - 14 - size,
        155 +
          ((mark.start - range[0]) / (range[1] - range[0])) * (width - 169) -
          size / 2,
      ),
    );
    for (const row of rows)
      result.push({
        mark,
        rect: {
          x,
          y: 27 + row.start * laneHeight,
          width: size,
          height: (row.end - row.start) * laneHeight,
        },
      });
  }
  return result;
}
