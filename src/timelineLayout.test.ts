import { expect, it } from "vitest";
import { tokenSlots, timelineRect, insideRect } from "./timelineLayout";
import type { Token } from "./types";
const tokens = (rows: number[][]) =>
  rows.map(
    ([start, duration, stall = 0]) => ({ start, duration, stall }) as Token,
  );
it("assigns upstream same-clock slots and caps collisions at slot three", () => {
  expect(
    tokenSlots(
      tokens([
        [0, 10],
        [0, 10],
        [0, 10],
        [0, 10],
        [0, 10],
        [1, 20],
        [1, 20],
      ]),
    ),
  ).toEqual([0, 1, 2, 3, 3, 0, 1]);
  expect(
    tokenSlots(
      tokens([
        [0, 0],
        [0, 0],
        [0, 0, 4],
        [0, 1],
      ]),
    ),
  ).toEqual([0, 0, 0, 1]);
});
it("hit-testing follows stacked geometry, clipping and visible short-token widths", () => {
  const a = timelineRect(10, 50, 0, 0, [0, 100], 1000)!;
  const b = timelineRect(10, 50, 0, 2, [0, 100], 1000)!;
  expect(insideRect(a, a.x + 10, 47)).toBe(true);
  expect(insideRect(b, b.x + 10, 47)).toBe(false);
  expect(insideRect(b, b.x + 10, 33)).toBe(true);
  expect(insideRect(a, 100, 40)).toBe(false);
  expect(timelineRect(0, 10, -1, 0, [0, 100], 1000)).toBeNull();
  expect(timelineRect(-10, 0, 0, 0, [0, 100], 1000)).toBeNull();
  const point = timelineRect(10, 10, 0, 0, [0, 100], 1000)!;
  expect(point.width).toBe(2);
  const endpoint = timelineRect(100, 100, 0, 0, [0, 100], 1000)!;
  expect(endpoint.width).toBe(2);
  expect(endpoint.x + endpoint.width).toBe(986);
  expect(insideRect(point, point.x + 1, 40)).toBe(true);
  expect(timelineRect(-10, 40, 0, 0, [0, 100], 1000)!.x).toBe(155);
});
