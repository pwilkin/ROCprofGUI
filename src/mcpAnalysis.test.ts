import { expect, it } from "vitest";
import protocol from "./mcpAnalysisProtocol.json";
import { kernelStats } from "./analysis";
import { globalMarks, defaultGlobalLayers } from "./globalTimeline";
import {
  goldenKernelStats,
  goldenGlobalMarks,
} from "../tests/fixtures/a1-golden";
import demo from "../public/demo.json";
import type { Capture } from "./types";

export function edgeCapture(): Capture {
  return {
    ...demo.capture,
    dispatches: [
      { kernel: "a", start: 0, end: 1e16 },
      { kernel: "a", start: 0, end: 1 },
      { kernel: "a", start: 0, end: -1e16 },
      { kernel: "b", start: -4, end: -4 },
      { kernel: "c", start: 0.1, end: 0.3 },
      { kernel: "c", start: 0.2, end: 0.4 },
      { kernel: "d", start: 0, end: 0 },
    ],
    intervals: [
      { kernel: "b", start: -5, end: -1 },
      { kernel: "b", start: 0, end: 2 },
      { kernel: "a", start: 1, end: 3 },
      { kernel: "a", start: 2, end: 4 },
      { kernel: "a", start: 8, end: 6 },
      { kernel: "unmatched", start: 0, end: 99 },
    ],
  } as unknown as Capture;
}
it("pins pre-A1 arithmetic, tie order and mark identity", () => {
  expect(kernelStats(edgeCapture())).toEqual(goldenKernelStats(edgeCapture()));
  expect(
    kernelStats({ dispatches: [], intervals: [] } as unknown as Capture),
  ).toEqual([]);
  const c = demo.capture as unknown as Capture;
  expect(globalMarks(c, defaultGlobalLayers, "dispatch:2")).toEqual(
    goldenGlobalMarks(c, defaultGlobalLayers, "dispatch:2"),
  );
});
it("pins v2 budgets and vocabulary", () => {
  expect(protocol.version).toBe(2);
  expect(protocol.modes).toEqual([
    "kernel-stream",
    "timeline-stream",
    "projected-json",
    "diagnostics-compatibility",
  ]);
  expect(protocol.budgets.fileBytes).toBe(512 * 1024 * 1024);
  expect(protocol.budgets.inlineBytes).toBe(16 * 1024 * 1024);
  expect(protocol.budgets.kernelIntervals).toBe(2_000_000);
  expect(protocol.budgets.chunkBytes).toBe(1024 * 1024);
});

it("incremental kernels preserve every arithmetic edge and reject before insertion", async () => {
  const { createKernelStatsAccumulator } = await import("./analysis");
  for (const nonfinite of [false, true]) {
    const c = edgeCapture();
    if (nonfinite)
      c.intervals.push({ ...c.intervals[0], start: NaN, end: Infinity });
    for (let split = 0; split <= c.dispatches.length; split++) {
      const a = createKernelStatsAccumulator();
      for (const chunk of [
        c.dispatches.slice(0, split),
        c.dispatches.slice(split),
      ])
        for (const d of chunk) a.dispatch(d);
      for (const i of c.intervals) a.interval(i);
      expect(a.finish()).toEqual(goldenKernelStats(c));
    }
  }
  const a = createKernelStatsAccumulator({
    beforeKernel: (_name, count) => {
      if (count > 2) throw Error("groups");
    },
    beforeInterval: (count) => {
      if (count > 3) throw Error("pairs");
    },
  });
  const sample = { kernel: "a", start: 0, end: 1 };
  a.dispatch(sample);
  a.dispatch({ ...sample, kernel: "b" });
  expect(() => a.dispatch({ ...sample, kernel: "c" })).toThrow("groups");
  for (let i = 0; i < 3; i++) a.interval(sample);
  expect(() => a.interval(sample)).toThrow("pairs");
  expect(a.finish()).toEqual(
    goldenKernelStats({
      dispatches: [sample, { ...sample, kernel: "b" }],
      intervals: [sample, sample, sample],
    } as unknown as Capture),
  );
});
it("single marks retain sparse original ordinals, selection and event masks", async () => {
  const { dispatchMark, residencyMark, markerMark, decoderMark } =
    await import("./globalTimeline");
  const c = demo.capture as unknown as Capture;
  for (const index of [2, 17, 100]) {
    const d = { ...c.dispatches[0], start: 1, end: 2 };
    const intervals = Array.from({ length: index + 1 }, () => c.intervals[0]);
    const dispatches = Array.from({ length: index + 1 }, () => d);
    const markers = Array.from({ length: index + 1 }, () => ({
      start: 0,
      end: null,
      point: index === 17,
      se: 2,
      cu: 0,
      simd: 0,
      slot: 0,
      depth: 3,
      name: "λ<&",
      markerId: 4,
      codeObject: "x",
      source: "a.cpp",
    }));
    const e = {
      id: "e",
      se: 2,
      time: 1,
      kind: "event",
      fields: { type: "6", λ: "<&" },
    };
    const decoderEvents = Array.from({ length: index + 1 }, () => e);
    const input = {
      ...c,
      intervals,
      dispatches,
      markers,
      decoderEvents,
    } as unknown as Capture;
    const expected = goldenGlobalMarks(input, defaultGlobalLayers);
    const single = [
      residencyMark(intervals[index], index),
      markerMark(markers[index], index, c.end),
      dispatchMark(d, index),
      decoderMark(e, index, defaultGlobalLayers),
    ];
    for (const m of single)
      expect({ ...m, selected: false }).toEqual(
        expected.find((v) => v.key === m!.key),
      );
    expect(dispatchMark({ ...d, end: 1 }, index)).toBeNull();
    expect(
      decoderMark(e, index, { ...defaultGlobalLayers, eventGroups: 0 }),
    ).toBeNull();
  }
});
