import { expect, it } from "vitest";
import { lineSamples, lineSampleBudget } from "./lineSamples";
it("retains isolated observations and separates finite segments", () => {
  const result = lineSamples([
    [0, NaN],
    [1, 3],
    [2, NaN],
    [3, 5],
    [4, 7],
    [5, NaN],
  ]);
  expect(result.limited).toBe(false);
  expect(result.points).toEqual([
    [1, 3],
    [3, NaN],
    [3, 5],
    [4, 7],
  ]);
});
it("bounds samples while retaining endpoints and extrema in long runs", () => {
  const points: [number, number][] = Array.from({ length: 100000 }, (_, i) => [
    i,
    i === 41234 ? 1000 : i === 51321 ? -1000 : 0,
  ]);
  const result = lineSamples(points);
  expect(result.points.length).toBeLessThanOrEqual(2000);
  expect(result.points[0]).toEqual(points[0]);
  expect(result.points.at(-1)).toEqual(points.at(-1));
  expect(result.points).toContainEqual([41234, 1000]);
  expect(result.points).toContainEqual([51321, -1000]);
});
it("requests a narrower range rather than connecting excessive fragments", () => {
  const points: [number, number][] = Array.from({ length: 1001 }, (_, i) => [
    i,
    i % 2 ? NaN : 1,
  ]);
  expect(lineSamples(points)).toEqual({ points: [], limited: true });
  expect(lineSamples(points.slice(0, -2)).limited).toBe(false);
});

it("bias adjusts detail while preserving gaps, extrema and bounded output", () => {
  const input: [number, number][] = Array.from({ length: 100000 }, (_, i) => [
    i,
    Math.sin(i),
  ]);
  input[50000][1] = NaN;
  input[12345][1] = 100;
  input[67890][1] = -100;
  const fine = lineSamples(input, -20).points;
  const normal = lineSamples(input).points;
  const coarse = lineSamples(input, 20).points;
  expect(fine.length).toBeGreaterThan(normal.length);
  expect(normal.length).toBeGreaterThan(coarse.length);
  expect(fine.length).toBeLessThanOrEqual(32001);
  for (const output of [fine, normal, coarse]) {
    expect(output).toContainEqual([12345, 100]);
    expect(output).toContainEqual([67890, -100]);
    expect(output.filter((p) => Number.isNaN(p[1]))).toHaveLength(1);
    expect(output[0]).toEqual(input[0]);
    expect(output.at(-1)).toEqual(input.at(-1));
  }
  expect(lineSampleBudget(-20)).toBe(8000);
  expect(lineSampleBudget(20)).toBe(1);
  expect(lineSampleBudget(NaN)).toBe(500);
  expect(lineSampleBudget(1.5)).toBe(500);
});
