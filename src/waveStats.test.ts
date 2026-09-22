import { expect, test } from "vitest";
import { applyWaveStats } from "./waveStats";
import type { Instruction } from "./types";
const code = [{index: 7, hits: 99, latency: 99, stall: 99, idle: 99}, {index: 8, hits: 99}] as Instruction[];
test("exact totals and mean preserve summed stall and reset missing instructions", () => {
  const rows = [{instruction: 7, hits: 3, latency: 16, stall: 6}];
  expect(applyWaveStats(code, rows)[0]).toMatchObject({hits: 3, latency: 16, stall: 6, idle: 0});
  expect(applyWaveStats(code, rows, true)[0].latency).toBe(16 / 3);
  expect(applyWaveStats(code, rows)[1]).toMatchObject({hits: 0, latency: 0, stall: 0, idle: 0});
});
test("pending statistics do not masquerade as zero measurements", () => {
  expect(applyWaveStats(code, null)).toEqual([]);
});
