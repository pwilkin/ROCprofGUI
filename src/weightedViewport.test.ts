import { describe, expect, it } from "vitest";
import { clipWeightedInterval, fitWeightedRange } from "./weightedViewport";
describe("weighted flamegraph viewport", () => {
  it("clips boundary leaves without changing their original weights or assigning outside leaves", () => {
    const intervals = [
      [0, 20],
      [20, 80],
      [80, 100],
    ];
    const clipped = intervals.map(([a, b]) =>
      clipWeightedInterval(a, b, [10, 90]),
    );
    expect(clipped.map((r) => r?.width)).toEqual([0.125, 0.75, 0.125]);
    expect(clipped.map((r) => r?.x)).toEqual([0, 0.125, 0.875]);
    expect(clipWeightedInterval(90, 100, [10, 90])).toBeNull();
    expect(clipWeightedInterval(0, 10, [10, 90])).toBeNull();
    expect(intervals).toEqual([
      [0, 20],
      [20, 80],
      [80, 100],
    ]);
  });
  it("fits pans at either boundary and retains fractional zoom spans", () => {
    expect(fitWeightedRange([-20, 30], 100)).toEqual([0, 50]);
    expect(fitWeightedRange([90, 140], 100)).toEqual([50, 100]);
    expect(fitWeightedRange([0.125, 0.25], 1)).toEqual([0.125, 0.25]);
    expect(fitWeightedRange([0, 200], 100)).toEqual([0, 100]);
    expect(fitWeightedRange([NaN, 5], 100)).toEqual([0, 100]);
    expect(fitWeightedRange([0, 1], 0)).toEqual([0, 1]);
  });
});
