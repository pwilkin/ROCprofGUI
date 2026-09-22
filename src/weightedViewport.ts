import type { Range } from "./types";
export function clipWeightedInterval(start: number, end: number, range: Range) {
  const low = Math.max(start, range[0]),
    high = Math.min(end, range[1]);
  if (!(high > low) || !(range[1] > range[0])) return null;
  return {
    start: low,
    end: high,
    x: (low - range[0]) / (range[1] - range[0]),
    width: (high - low) / (range[1] - range[0]),
  };
}

export function fitWeightedRange(range: Range, total: number): Range {
  const end = Number.isFinite(total) && total > 0 ? total : 1;
  const span = Math.min(end, range[1] - range[0]);
  if (!range.every(Number.isFinite) || !(span > 0)) return [0, end];
  const start = Math.max(0, Math.min(end - span, range[0]));
  return start + span > start ? [start, start + span] : [0, end];
}
