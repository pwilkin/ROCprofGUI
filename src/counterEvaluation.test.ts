import { describe, expect, it } from "vitest";
import { counterDomains, counterGroups } from "./counterTensors";
import {
  counterPlotPage,
  evaluateCounters,
  hydrateResults,
  validateEvaluation,
} from "./counterEvaluation";
import { Tensor, withTensorBudget } from "./tensor";
const series = [
  {
    name: "A",
    unit: "events",
    scope: "SE 0 · CU 0 · bank 0",
    points: [
      [10, 2],
      [30, 6],
    ] as [number, number][],
  },
  {
    name: "A",
    unit: "events",
    scope: "SE 0 · CU 1 · bank 0",
    points: [
      [10, 4],
      [20, 8],
      [30, 10],
    ] as [number, number][],
  },
];
describe("counter worker evaluation", () => {
  it("preserves gaps, coordinates and methods across structured clone", () => {
    const result = evaluateCounters({
      series,
      scope: "ATT bank 0",
      definitions:
        "Copy := A\nTotal := sum[A,axis=CU]\nTail := select[A,1:3,axis=Time]",
    });
    const hydrated = hydrateResults(structuredClone(result));
    expect(hydrated.get("Copy")!.values).toEqual([2, NaN, 6, 4, 8, 10]);
    expect(hydrated.get("Total")!.values).toEqual([6, 8, 16]);
    expect(hydrated.get("Tail")!.axisIndices[3]).toEqual([1, 2]);
    expect(hydrated.get("Copy")!.coords(3)).toEqual([0, 0, 1, 0]);
    expect(result.times).toEqual([10, 20, 30]);
  });
  it("enumerates domains without allocating dense counter tensors", () => {
    const sparse = [{ ...series[0], scope: "SE 0 · CU 1000000 · bank 0" }];
    expect(counterDomains(sparse)[0].names).toEqual(["A"]);
    expect(() => counterGroups(sparse, 100)).toThrow();
    expect(() => counterGroups(series, 5)).toThrow("allocation budget");
  });
  it("rejects input and intermediate allocations and releases the budget after failure", () => {
    expect(() =>
      validateEvaluation({
        series: [{ ...series[0], points: { length: 1_000_001 } as any }],
        scope: "",
        definitions: "",
      }),
    ).toThrow("one million");
    expect(() =>
      withTensorBudget(1, () => new Tensor([1, 1, 1, 2], [0, 1])),
    ).toThrow("allocation budget");
    expect(new Tensor([1, 1, 1, 2], [0, 1]).values).toEqual([0, 1]);
    expect(() =>
      evaluateCounters({ series, scope: "ATT bank 0", definitions: "A := 1" }),
    ).toThrow("shadowing");
  });
  it("bounds combined results even when definitions alias the same raw tensor", () => {
    const points: [number, number][] = Array.from(
      { length: 700000 },
      (_, n) => [n, 1],
    );
    expect(() =>
      evaluateCounters({
        series: [{ ...series[0], points }],
        scope: "ATT bank 0",
        definitions: "Copy1 := A\nCopy2 := A\nCopy3 := A",
      }),
    ).toThrow("results exceed two million");
  });
  it("paginates large hardware domains without materializing every series", () => {
    const tensor = new Tensor([1, 1, 100000, 1], Array(100000).fill(3));
    const result = counterPlotPage(
      new Map([
        ["A", tensor],
        ["B", Tensor.scalar(4)],
      ]),
      8333,
    );
    expect(result.total).toBe(100001);
    expect(result.plots).toHaveLength(5);
    expect(result.plots[0].coords).toEqual([0, 0, 99996]);
    expect(result.plots.at(-1)?.name).toBe("B");
    expect(counterPlotPage(new Map(), 5)).toEqual({
      plots: [],
      total: 0,
      page: 0,
    });
  });
});

it("SPM clocks preserve per-XCC observations and upstream first-interval convention", () => {
  const record = (xcc: number, points: [number, number][]) => ({
    name: "BUSY",
    unit: "events",
    points,
    scope: `process 0 · agent 1 · dispatch 2 · counter 7 · [{"dimension_name":"DIMENSION_XCC","index":${xcc}}]`,
  });
  const input = [
    record(0, [
      [10, 2],
      [30, 6],
      [60, 9],
    ]),
    record(2, [[20, 4]]),
  ];
  const domain = counterDomains(input)[0];
  expect(domain.names).toEqual(["BUSY", "SCLOCK", "SPM_CLOCK"]);
  const result = hydrateResults(
    evaluateCounters({
      series: input,
      scope: domain.scope,
      definitions:
        "Clock := SCLOCK\nInterval := SPM_CLOCK\nRate := BUSY / SPM_CLOCK",
    }),
  );
  expect(result.get("Clock")!.shape).toEqual([3, 1, 1, 4]);
  expect(result.get("Clock")!.values).toEqual([
    10,
    NaN,
    30,
    60,
    NaN,
    NaN,
    NaN,
    NaN,
    NaN,
    20,
    NaN,
    NaN,
  ]);
  expect(result.get("Interval")!.values).toEqual([
    25,
    NaN,
    20,
    30,
    NaN,
    NaN,
    NaN,
    NaN,
    NaN,
    0,
    NaN,
    NaN,
  ]);
  expect(result.get("Rate")!.values.slice(0, 4)).toEqual([0.08, NaN, 0.3, 0.3]);
  expect(() => counterGroups(input, 20)).toThrow(/budget/);
});

it("recorded clock names stay addressable without replacing synthetic clocks", () => {
  const input = [
    {
      name: "SCLOCK",
      unit: "events",
      scope: "process 0 · agent 1 · counter 2 · []",
      points: [
        [10, 99],
        [20, 88],
      ] as [number, number][],
    },
  ];
  const domain = counterDomains(input)[0];
  expect(domain.bindings[0].symbol).toBe("RAW_SCLOCK_ID_2");
  const group = counterGroups(input)[0];
  expect(group.raw.RAW_SCLOCK_ID_2.values).toEqual([99, 88]);
  expect(group.raw.SCLOCK.values).toEqual([10, 20]);
  expect(group.raw.SPM_CLOCK.values).toEqual([10, 10]);
  expect(counterGroups(series)[0].raw.SCLOCK).toBeUndefined();
});

it("calibrated SPM keeps shader clocks separate from exact original realtime intervals", () => {
  const input = [
    {
      name: "BUSY",
      unit: "events",
      scope: "process 0 · agent 1 · counter 1 · []",
      points: [
        [500, 2],
        [530, 6],
        [580, 9],
      ] as [number, number][],
      spmClock: {
        anchorSe: 2,
        realtimeTimestamps: [
          "18446744073709551515",
          "18446744073709551525",
          "18446744073709551535",
        ],
      },
    },
  ];
  const group = counterGroups(input)[0];
  expect(group.raw.SCLOCK.values).toEqual([500, 530, 580]);
  expect(group.raw.SPM_CLOCK.values).toEqual([10, 10, 10]);
  for (const times of [
    ["1"],
    ["1", "1", "2"],
    ["3", "2", "1"],
    ["1", "2", "18446744073709551616"],
    ["1", "2", "bad"],
  ]) {
    expect(() =>
      counterGroups([
        { ...input[0], spmClock: { anchorSe: 2, realtimeTimestamps: times } },
      ]),
    ).toThrow();
  }
  expect(() =>
    counterGroups([
      ...input,
      {
        ...input[0],
        name: "B",
        scope: "process 0 · agent 1 · counter 2 · []",
        spmClock: undefined,
      },
    ]),
  ).toThrow(/mixes/);
});
