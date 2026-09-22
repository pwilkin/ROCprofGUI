import type { Range } from "./types";
export function axisBounds(low: string, high: string): Range {
  if (!low.trim() || !high.trim()) throw Error("Enter both axis bounds");
  const min = Number(low),
    max = Number(high);
  if (
    !Number.isFinite(min) ||
    !Number.isFinite(max) ||
    !(max > min) ||
    !Number.isFinite(max - min)
  )
    throw Error(
      "Axis bounds must be finite, with maximum greater than minimum and a finite span",
    );
  return [min, max];
}
export function axisFraction(value: number, min: number, max: number): number {
  const span = max - min;
  const offset = value - min;
  if (Number.isFinite(span) && Number.isFinite(offset)) return offset / span;
  const scale = Math.max(Math.abs(value), Math.abs(min), Math.abs(max));
  return (value / scale - min / scale) / (max / scale - min / scale);
}
