import { describe, it, expect } from "vitest";
import { Tensor, tensorExpression, tensorDefinitions } from "./tensor";
import { counterGroups } from "./counterTensors";
const a = new Tensor([1, 2, 2, 3], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
const run = (s: string) =>
  tensorExpression(s, (n) => {
    if (n === "A") return a;
    throw Error("unknown " + n);
  });
describe("dimension-aware counter expressions", () => {
  it("reduces named axes and broadcasts without flattening hardware identities", () => {
    const sum = run("sum[A, axis=[SE,CU]]");
    expect(sum.shape).toEqual([1, 1, 1, 3]);
    expect(sum.values).toEqual([22, 26, 30]);
    expect(run("A / mean[A, axis=Time]").values[0]).toBe(0.5);
    expect(run("sum[A]").scalar()).toBe(78);
  });
  it("selects slices, removes negative indices and calculates adjacent deltas", () => {
    expect(run("select[A, 0:3:2, axis=Time]").values).toEqual([
      1, 3, 4, 6, 7, 9, 10, 12,
    ]);
    expect(run("remove[A, -1, axis=SE]").values).toEqual([1, 2, 3, 4, 5, 6]);
    expect(run("delta[A, axis=Time]").values).toEqual(Array(8).fill(1));
  });
  it("propagates missing samples but reduces only valid observations", () => {
    const raw = new Tensor([1, 1, 2, 2], [2, NaN, 6, NaN]);
    const output = tensorExpression("mean[B,axis=CU]", () => raw);
    expect(output.values[0]).toBe(4);
    expect(output.values[1]).toBeNaN();
    expect(
      raw
        .binary(Tensor.scalar(0), (a, b) => a / b)
        .values.every((v) => !Number.isFinite(v)),
    ).toBe(true);
  });
  it("supports definitions, builtins, comparisons, logic and linear axes", () => {
    const result = tensorDefinitions(
      "R := sum[A,axis=CU]\nM := (R > 6) & (R <= 21)\nL := linear(NUM_SAMPLES)",
      { A: a },
    );
    expect(result.get("M")!.values).toEqual([0, 1, 1, 1, 1, 1]);
    expect(result.get("L")!.values).toEqual([0, 1, 2]);
    expect(() => tensorDefinitions("B := C\nC := B", { A: a })).toThrow(
      "Circular",
    );
  });
  it("rejects invalid syntax, incompatible dimensions and excessive allocations", () => {
    for (const s of [
      "globalThis.alert(1)",
      "select[A,99,axis=CU]",
      "select[A,0:3:0]",
      "sum[A,axis=GPU]",
      "linear(3000000)",
    ])
      expect(() => run(s)).toThrow();
    expect(() =>
      a.binary(new Tensor([1, 3, 1, 1], [1, 2, 3]), (x, y) => x + y),
    ).toThrow("Incompatible");
  });
  it("aligns exact timestamps with validity gaps and separates bank groups", () => {
    const groups = counterGroups([
      {
        name: "LOAD",
        scope: "SE 0 · CU 0 · bank 0",
        unit: "count",
        points: [
          [10, 1],
          [20, 2],
        ],
      },
      {
        name: "LOAD",
        scope: "SE 1 · CU 0 · bank 0",
        unit: "count",
        points: [[20, 3]],
      },
      {
        name: "LOAD",
        scope: "SE 0 · CU 0 · bank 1",
        unit: "count",
        points: [[10, 50]],
      },
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0].times).toEqual([10, 20]);
    expect(groups[0].raw.LOAD.reduce("sum", [1]).values).toEqual([1, 5]);
  });
});

it("preserves hardware and clock indices through slices and reductions", () => {
  const selected = run("select[A,1,axis=SE]");
  expect(selected.axisIndices[1]).toEqual([1]);
  const reduced = run("sum[A,axis=CU]");
  expect(reduced.axisIndices[2]).toBeNull();
  expect(run("delta[A,axis=Time]").axisIndices[3]).toEqual([1, 2]);
  expect(run("select[A,0:3:2,axis=Time]").axisIndices[3]).toEqual([0, 2]);
});

it("keeps SPM agents separate and broadcasts singleton instance counters", () => {
  const scope = (agent: number, instance: number) =>
    `process 0 · agent ${agent} · ${JSON.stringify([
      { dimension_name: "DIMENSION_XCC", index: 0 },
      { dimension_name: "DIMENSION_SHADER_ENGINE", index: 0 },
      { dimension_name: "DIMENSION_INSTANCE", index: instance },
    ])}`;
  const groups = counterGroups([
    {
      name: "SQ_CYCLES",
      scope: scope(42, 0),
      unit: "count",
      points: [
        [100, 10],
        [200, 20],
      ],
    },
    {
      name: "BUSY",
      scope: scope(42, 0),
      unit: "count",
      points: [
        [100, 2],
        [200, 4],
      ],
    },
    {
      name: "BUSY",
      scope: scope(42, 1),
      unit: "count",
      points: [
        [100, 3],
        [200, 6],
      ],
    },
    { name: "BUSY", scope: scope(43, 0), unit: "count", points: [[100, 99]] },
  ]);
  expect(groups).toHaveLength(2);
  const ratio = tensorExpression("BUSY / SQ_CYCLES", (n) => groups[0].raw[n]);
  expect(ratio.values).toEqual([0.2, 0.2, 0.3, 0.3]);
  expect(() =>
    counterGroups([
      {
        name: "A",
        scope: "SE 0 · CU 0 · bank 0",
        unit: "count",
        points: [
          [0, 1],
          [0, 2],
        ],
      },
    ]),
  ).toThrow("Duplicate");
});

it("recognizes counter-ID SPM scopes without separating expressions by counter ID", () => {
  const scope = (id: number, se: number) =>
    `process 0 · agent 42 · counter ${id} · ${JSON.stringify([
      { dimension_name: "DIMENSION_XCC", index: 0 },
      { dimension_name: "DIMENSION_SHADER_ENGINE", index: se },
      { dimension_name: "DIMENSION_INSTANCE", index: 0 },
    ])}`;
  const groups = counterGroups([
    {
      name: "BUSY",
      scope: scope(1, 0),
      unit: "count",
      points: [
        [100, 4],
        [200, 8],
      ],
    },
    { name: "BUSY", scope: scope(1, 1), unit: "count", points: [[200, 6]] },
    {
      name: "SQ_CYCLES",
      scope: scope(2, 0),
      unit: "count",
      points: [
        [100, 10],
        [200, 20],
      ],
    },
  ]);
  expect(groups).toHaveLength(1);
  expect(groups[0].scope).toBe("process 0 · agent 42");
  const ratio = tensorExpression(
    "sum[BUSY,axis=SE] / SQ_CYCLES",
    (n) => groups[0].raw[n],
  );
  expect(ratio.values).toEqual([0.4, 0.7]);
  const distinct = counterGroups([
    { name: "BUSY", scope: scope(1, 0), unit: "count", points: [[100, 4]] },
    { name: "BUSY", scope: scope(2, 1), unit: "count", points: [[200, 6]] },
  ])[0];
  expect(distinct.names).toEqual([
    "RAW_BUSY_ID_1",
    "RAW_BUSY_ID_2",
    "SCLOCK",
    "SPM_CLOCK",
  ]);
  expect(
    tensorExpression(
      "sum[RAW_BUSY_ID_1] + sum[RAW_BUSY_ID_2]",
      (n) => distinct.raw[n],
    ).scalar(),
  ).toBe(10);
});

it("rejects ambiguous and unsafe SPM dimension coordinates", () => {
  for (const dimensions of [
    [
      { dimension_name: "DIMENSION_XCC", index: 0 },
      { dimension_name: "DIMENSION_XCC", index: 1 },
    ],
    [{ dimension_name: "DIMENSION_XCC", index: 9007199254740992 }],
    [null],
    [{ index: 0 }],
  ]) {
    expect(() =>
      counterGroups([
        {
          name: "A",
          scope: `process 0 · agent 42 · counter 1 · ${JSON.stringify(dimensions)}`,
          unit: "count",
          points: [[0, 1]],
        },
      ]),
    ).toThrow("Invalid captured counter dimension");
  }
});

it("partitions SPM tensors by dispatch queue stream and unknown collection", () => {
  const partitions = [
    "dispatch 0 · queue 7 · stream 0",
    "dispatch 1 · queue 7 · stream 0",
    "dispatch 0 · queue 8 · stream 0",
    "dispatch 0 · queue 7 · stream 1",
    "collection 5 · queue 7 · stream 0",
    "collection 6 · queue 7 · stream 0",
  ];
  const input = partitions.map((partition, index) => ({
    name: "A",
    unit: "count",
    scope: `process 0 · agent 42 · ${partition} · counter 1 · [{"dimension_name":"DIMENSION_XCC","index":0}]`,
    points: [[0, index + 1] as [number, number]],
  }));
  const groups = counterGroups(input);
  expect(groups).toHaveLength(6);
  expect(groups.map((g) => g.raw.A.scalar())).toEqual([1, 2, 3, 4, 5, 6]);
  expect(groups.map((g) => g.scope)).toEqual(
    partitions.map((p) => `process 0 · agent 42 · ${p}`),
  );
});
