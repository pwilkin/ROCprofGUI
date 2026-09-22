import { expect, it } from "vitest";
import { scalarDefinition, scalarSeries } from "./scalarCounters";
import type { Series } from "./types";
const counters: Series[] = [
  {
    name: "cycles",
    scope: "SE0",
    unit: "cycles",
    points: [
      [1, 10],
      [2, 20],
      [3, 30],
    ],
  },
  {
    name: "waves",
    scope: "SE0",
    unit: "waves",
    points: [
      [1, 2],
      [3, 5],
    ],
  },
];
it("restores definitions and recomputes exact-overlap values from current inputs", () => {
  const definition = JSON.parse(
    JSON.stringify(scalarDefinition("ratio", "C0 / C1", counters)),
  );
  expect(scalarSeries(definition, counters).points).toEqual([
    [1, 5],
    [3, 6],
  ]);
  const changed = structuredClone(counters);
  changed[0].points[0][1] = 40;
  expect(scalarSeries(definition, changed).points[0]).toEqual([1, 20]);
  changed[0].name = "unrelated counter";
  expect(() => scalarSeries(definition, changed)).toThrow("identities changed");
});
it("rejects invalid references, cross-scope arithmetic and exhausted shared budgets", () => {
  expect(() => scalarDefinition("x", "C9", counters)).toThrow("Unknown series");
  const other = structuredClone(counters);
  other[1].scope = "SE1";
  expect(() =>
    scalarSeries(scalarDefinition("x", "C0+C1", other), other),
  ).toThrow("identical scopes");
  const definition = scalarDefinition("x", "C0", counters);
  const budget = { remaining: 6 };
  expect(scalarSeries(definition, counters, budget).points).toHaveLength(3);
  expect(() => scalarSeries(definition, counters, budget)).toThrow(
    "budget exceeded",
  );
});
