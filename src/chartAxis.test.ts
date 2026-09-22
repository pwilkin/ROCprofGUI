import { expect, it } from "vitest";
import { axisBounds, axisFraction } from "./chartAxis";
it("accepts signed finite bounds and rejects degenerate or unrepresentable spans", () => {
  expect(axisBounds("-2.5", "4.25")).toEqual([-2.5, 4.25]);
  expect(axisBounds("0", "1e4")).toEqual([0, 10000]);
  for (const bounds of [
    ["", "1"],
    ["0", ""],
    ["2", "2"],
    ["3", "2"],
    ["NaN", "2"],
    ["0", "Infinity"],
    ["-1e308", "1e308"],
  ])
    expect(() => axisBounds(...(bounds as [string, string]))).toThrow();
});

it("scales large signed values without overflowing the domain subtraction", () => {
  expect(axisFraction(0, -1e308, 1e308)).toBe(0.5);
  expect(axisFraction(-1e308, -1e308, 1e308)).toBe(0);
  expect(axisFraction(1e308, -1e308, 1e308)).toBe(1);
  expect(axisFraction(-10, 0, 10)).toBe(-1);
  expect(axisFraction(5e-301, 0, 1e-300)).toBe(0.5);
});
