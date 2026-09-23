import { describe, it, expect } from "vitest";
import {
  evaluate,
  hiddenLatencyStats,
  union,
  boundedRange,
  occupancy,
  instructionStats,
  kernelStats,
} from "./analysis";
import bundle from "../public/demo.json";
import type { Capture, Wave } from "./types";
describe("profiling semantics", () => {
  it("counts overlap once", () =>
    expect(
      union([
        [0, 10],
        [5, 15],
        [20, 30],
      ]),
    ).toBe(25));
  it("maintains a valid bounded range", () => {
    expect(boundedRange([-5, 5], [0, 100])).toEqual([0, 10]);
    expect(boundedRange([95, 105], [0, 100])).toEqual([90, 100]);
  });
  it("kernel stats add measured occupancy to point dispatches", () => {
    const capture = {
      dispatches: [
        { kernel: "a", start: 5, end: 5 },
        { kernel: "b", start: 0, end: 0 },
      ],
      intervals: [
        { kernel: "a", start: 10, end: 30 },
        { kernel: "a", start: 20, end: 40 },
        { kernel: "b", start: 0, end: 10 },
        { kernel: "unrecorded", start: 0, end: 99 },
      ],
    } as unknown as Capture;
    const [a, b] = kernelStats(capture);
    expect(a).toMatchObject({ name: "a", count: 1, total: 0, waves: 2, waveTime: 40, activeTime: 30 });
    expect(b).toMatchObject({ name: "b", waves: 1, activeTime: 10 });
  });
  it("occupancy returns to zero", () => {
    const series = occupancy(bundle.capture as unknown as Capture);
    expect(series.at(-1)?.[1]).toBe(0);
    expect(Math.min(...series.map((p) => p[1]))).toBeGreaterThanOrEqual(0);
  });
  it("instruction selection uses range and selected wave", () => {
    const wave = Object.values(bundle.waves)[0] as unknown as Wave;
    const rows = instructionStats(
      bundle.capture.instructions,
      wave,
      [wave.start, wave.tokens[1].start],
      "wave",
    );
    expect(rows.find((i) => i.index === 1)?.hits).toBe(1);
    expect(rows.find((i) => i.index === 2)?.hits).toBe(0);
  });
});
describe("counter expressions", () => {
  it("supports precedence and functions", () => {
    expect(evaluate("C0 / max(C1, 1) * 100", { C0: 4, C1: 8 })).toBe(50);
    expect(evaluate("abs(-3)+min(2,4)*2", {})).toBe(7);
  });
  it("rejects missing counters and division by zero", () => {
    expect(() => evaluate("C0 / C1", { C0: 1, C1: 0 })).toThrow("Division");
    expect(() => evaluate("C8+1", {})).toThrow("Unknown");
  });
  it("never executes JavaScript", () => {
    expect(() => evaluate("globalThis.alert(1)", {})).toThrow();
    expect(() => evaluate("1; 2", {})).toThrow();
  });
});

describe("hidden latency display", () => {
  it("clamps each hidden component to its matching measured component", () => {
    const capture = {
      ...bundle.capture,
      instructions: [
        {
          ...bundle.capture.instructions[1],
          hits: 2,
          latency: 20,
          stall: 5,
          idle: 10,
        },
      ],
    } as unknown as Capture;
    const result = hiddenLatencyStats(
      capture,
      {
        byInstruction: { 1: { idle: 100, stall: 100, issue: 100 } },
        byWave: {},
        model: "test",
      },
      "capture",
      "",
      0,
      [0, 100],
      true,
      true,
    );
    expect(result[0].latency).toBe(0);
    expect(result[0].stall).toBe(0);
  });
  it("keeps selected-wave mean separate from whole-capture hidden totals", () => {
    const capture = bundle.capture as unknown as Capture;
    const id = capture.waves[0].id;
    const analysis = {
      byInstruction: {},
      byWave: {
        [id]: [
          {
            start: 10,
            instruction: 1,
            duration: 20,
            stall: 5,
            idle: 2,
            hidden: { idle: 2, stall: 3, issue: 7 },
          },
          {
            start: 15,
            instruction: 1,
            duration: 10,
            stall: 0,
            idle: 0,
            hidden: { idle: 0, stall: 0, issue: 0 },
          },
        ],
      },
      model: "test",
    };
    const result = hiddenLatencyStats(
      capture,
      analysis,
      "mean",
      id,
      0,
      [0, 100],
      true,
      false,
    );
    expect(result.find((i) => i.index === 1)?.latency).toBe(10);
  });
});

it("wave maximum and occurrence costs use full-wave per-instruction order", () => {
  const code = bundle.capture.instructions.slice(1, 3);
  const tokens = [
    { start: 10, duration: 30, stall: 5, instruction: 1, category: 6 },
    { start: 20, duration: 12, stall: 2, instruction: 2, category: 6 },
    { start: 50, duration: 10, stall: 1, instruction: 1, category: 6 },
  ];
  const wave = { tokens } as Wave;
  const occurrence = instructionStats(code, wave, [40, 60], "iteration", 1);
  expect(occurrence.map((i) => [i.hits, i.latency])).toEqual([
    [1, 10],
    [0, 0],
  ]);
  const maximum = instructionStats(code, wave, [40, 60], "max");
  expect(maximum.map((i) => [i.hits, i.latency, i.stall])).toEqual([
    [2, 30, 5],
    [1, 12, 2],
  ]);
  const capture = {
    ...bundle.capture,
    instructions: code,
    waves: [{ id: "w", se: 0 }],
  } as unknown as Capture;
  const analysis = {
    model: "test",
    byInstruction: {},
    byWave: {
      w: tokens.map((t, n) => ({
        ...t,
        idle: 0,
        hidden: { idle: 0, stall: 0, issue: n === 0 ? 25 : 0 },
      })),
    },
  };
  expect(
    hiddenLatencyStats(
      capture,
      analysis,
      "iteration",
      "w",
      0,
      [40, 60],
      true,
      false,
      1,
    ).map((i) => [i.hits, i.latency]),
  ).toEqual([
    [1, 10],
    [0, 0],
  ]);
  expect(
    hiddenLatencyStats(
      capture,
      analysis,
      "max",
      "w",
      0,
      [40, 60],
      true,
      false,
    ).map((i) => [i.hits, i.latency, i.stall]),
  ).toEqual([
    [2, 10, 1],
    [1, 12, 2],
  ]);
  const mean = instructionStats(code, null, [0, 1], "capture-mean");
  expect(mean[0].latency).toBe(code[0].latency / code[0].hits);
  expect(
    hiddenLatencyStats(
      capture,
      analysis,
      "capture-mean",
      "w",
      0,
      [0, 1],
      false,
      false,
    )[0].latency,
  ).toBe(mean[0].latency);
});
