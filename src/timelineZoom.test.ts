import { expect, it } from "vitest";
import {
  discreteZoomRange,
  stepZoomRange,
  waveMipLevels,
} from "./timelineZoom";
it("discrete spans follow upstream clock bases and retain the requested anchor", () => {
  const a = discreteZoomRange([10000, 20000], [0, 100000], 1169, 1, 3, 0.25);
  const b = discreteZoomRange([10000, 20000], [0, 100000], 1169, 4, 3, 0.25);
  expect(a[1] - a[0]).toBe(1328);
  expect(b[1] - b[0]).toBe(5328);
  expect(a[0] + 0.25 * (a[1] - a[0])).toBe(12500);
  expect(b[0] + 0.25 * (b[1] - b[0])).toBe(12500);
  expect(waveMipLevels).toEqual([-2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
});
it("steps double and halve canonical ranges without anchor drift", () => {
  const range = discreteZoomRange(
    [10000, 20000],
    [0, 100000],
    1169,
    1,
    3,
    0.25,
  );
  const wider = stepZoomRange(range, [0, 100000], 1169, 1, 1, 0.25);
  expect(wider[1] - wider[0]).toBe(2 * (range[1] - range[0]));
  expect(stepZoomRange(wider, [0, 100000], 1169, 1, -1, 0.25)).toEqual(range);
});
it("capture edges and terminal levels never reverse the requested zoom direction", () => {
  expect(discreteZoomRange([0, 1000], [0, 2000], 1169, 1, 3, 0)).toEqual([
    0, 1328,
  ]);
  expect(discreteZoomRange([0, 1000], [0, 2000], 1169, 1, 10)).toEqual([
    0, 2000,
  ]);
  expect(stepZoomRange([0, 1e12], [0, 1e12], 1169, 1, 1)).toEqual([0, 1e12]);
  expect(stepZoomRange([0, 1], [0, 100], 1169, 1, -1)).toEqual([0, 1]);
  const result = discreteZoomRange([100, 200], [100, 200], 100, 4, -2);
  expect(result[1] - result[0]).toBe(1);
});
