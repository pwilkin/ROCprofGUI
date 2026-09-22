import { expect, it } from "vitest";
import { counterGaps } from "./counterGaps";
import type { Series } from "./types";
const series = (times: number[], xcc = 0, dispatch = 10): Series => ({
  name: "A",
  scope: `process 0 · agent 42 · dispatch ${dispatch} · counter 1 · [{"dimension_name":"DIMENSION_XCC","index":${xcc}}]`,
  unit: "count",
  points: times.map((t) => [t, t]),
});
it("breaks only at unobserved peer timestamps within a shared SPM clock", () => {
  const a = series([10, 30]),
    peer = series([10, 20, 30]),
    otherXcc = series([15], 1),
    otherDispatch = series([25], 0, 11);
  const before = JSON.stringify(a);
  const gaps = counterGaps([a, peer, otherXcc, otherDispatch]);
  expect(gaps.get(a)).toEqual({
    points: [
      [10, 10],
      [20, NaN],
      [30, 30],
    ],
    missing: 1,
  });
  expect(gaps.size).toBe(1);
  expect(JSON.stringify(a)).toBe(before);
});
it("does not guess regular cadence or insert zero observations", () => {
  const a = series([10, 1000]);
  expect(counterGaps([a]).size).toBe(0);
  const att = { ...series([10, 30]), scope: "SE 0 · CU 0 · bank 0" };
  expect(counterGaps([att, { ...att, points: [[20, 7]] }]).size).toBe(0);
});
it("compresses consecutive missing peer samples into one line break", () => {
  const a = series([0, 100]);
  const gaps = counterGaps([a, series([10, 20, 30, 40])]);
  expect(gaps.get(a)?.missing).toBe(4);
  expect(gaps.get(a)?.points).toHaveLength(3);
});
