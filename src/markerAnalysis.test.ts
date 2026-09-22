import { it, expect } from "vitest";
import { markerTree } from "./markerAnalysis";
import type { MarkerScope } from "./types";
function marker(
  id: number,
  parent: number | null,
  start: number,
  end: number | null,
  name: string,
): MarkerScope {
  return {
    id,
    parent,
    start,
    end,
    name,
    markerId: id,
    codeObject: "1",
    depth: parent === null ? 0 : 1,
    kind: "U",
    source: "",
    point: false,
    se: 0,
    cu: 0,
    simd: 0,
    slot: 0,
  };
}
it("clips inclusive durations without double-counting children into root cost", () => {
  const roots = markerTree(
    [marker(0, null, 0, 100, "parent"), marker(1, 0, 20, 70, "child")],
    [30, 80],
  );
  expect(roots[0].duration).toBe(50);
  expect(roots[0].children[0].duration).toBe(40);
});
it("keeps open scopes and points out of duration accounting", () => {
  const point = { ...marker(1, null, 20, 20, "point"), point: true };
  const roots = markerTree([marker(0, null, 0, null, "open"), point], [0, 100]);
  expect(roots.map((n) => n.duration)).toEqual([0, 0]);
  expect(roots[0].open).toBe(1);
  expect(roots[1].points).toBe(1);
});
it("aggregates repeated paths but separates code objects", () => {
  const list = [
    marker(0, null, 0, 10, "same"),
    { ...marker(1, null, 20, 30, "same"), markerId: 0 },
    { ...marker(2, null, 40, 50, "same"), markerId: 0, codeObject: "2" },
  ];
  const roots = markerTree(list, [0, 100]);
  expect(roots).toHaveLength(2);
  expect(roots[0].count).toBe(2);
  expect(roots[0].duration).toBe(20);
});
