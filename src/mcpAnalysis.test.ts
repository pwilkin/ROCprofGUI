import { expect, it } from "vitest";
import protocol from "./mcpAnalysisProtocol.json";
import { kernelStats } from "./analysis";
import { globalMarks, defaultGlobalLayers } from "./globalTimeline";
import { goldenKernelStats, goldenGlobalMarks } from "../tests/fixtures/a1-golden";
import demo from "../public/demo.json";
import type { Capture } from "./types";

export function edgeCapture(): Capture {
  return { ...demo.capture, dispatches: [
    { kernel: "a", start: 0, end: 1e16 }, { kernel: "a", start: 0, end: 1 },
    { kernel: "a", start: 0, end: -1e16 }, { kernel: "b", start: -4, end: -4 },
    { kernel: "c", start: 0.1, end: 0.3 }, { kernel: "c", start: 0.2, end: 0.4 },
    { kernel: "d", start: 0, end: 0 },
  ], intervals: [
    { kernel: "b", start: -5, end: -1 }, { kernel: "b", start: 0, end: 2 },
    { kernel: "a", start: 1, end: 3 }, { kernel: "a", start: 2, end: 4 },
    { kernel: "a", start: 8, end: 6 }, { kernel: "unmatched", start: 0, end: 99 },
  ] } as unknown as Capture;
}
it("pins pre-A1 arithmetic, tie order and mark identity", () => {
  expect(kernelStats(edgeCapture())).toEqual(goldenKernelStats(edgeCapture()));
  expect(kernelStats({ dispatches: [], intervals: [] } as unknown as Capture)).toEqual([]);
  const c = demo.capture as unknown as Capture;
  expect(globalMarks(c, defaultGlobalLayers, "dispatch:2")).toEqual(goldenGlobalMarks(c, defaultGlobalLayers, "dispatch:2"));
});
it("pins v2 budgets and vocabulary", () => {
  expect(protocol.version).toBe(2);
  expect(protocol.modes).toEqual(["kernel-stream", "timeline-stream", "projected-json", "diagnostics-compatibility"]);
  expect(protocol.budgets.fileBytes).toBe(512 * 1024 * 1024);
  expect(protocol.budgets.inlineBytes).toBe(16 * 1024 * 1024);
  expect(protocol.budgets.kernelIntervals).toBe(2_000_000);
  expect(protocol.budgets.chunkBytes).toBe(1024 * 1024);
});
