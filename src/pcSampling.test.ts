import { expect, it } from "vitest";
import { samplingTotals } from "./pcSampling";
import type { Instruction } from "./types";
it("retains zero, missing and unknown reason categories without renormalizing", () => {
  const rows = [
    {
      pcIssued: 0,
      pcStalled: 4,
      stallReasons: [0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 7],
    },
    { pcIssued: 3, pcStalled: null },
    {},
  ] as Instruction[];
  const t = samplingTotals(rows);
  expect([
    t.issued,
    t.stalled,
    t.issuedRows,
    t.stalledRows,
    t.reasonRows,
    t.rows,
  ]).toEqual([3, 4, 2, 1, 1, 3]);
  expect(t.reasons[10]).toEqual({
    index: 10,
    name: "Unknown category 10",
    count: 7,
  });
  expect(samplingTotals([{} as Instruction]).issued).toBeNull();
});
it("does not report unsafe integer totals as exact measurements", () => {
  expect(
    samplingTotals([
      { pcIssued: Number.MAX_SAFE_INTEGER },
      { pcIssued: 1 },
    ] as Instruction[]).issued,
  ).toBeNull();
  expect(
    samplingTotals([{ stallReasons: [NaN] }] as Instruction[]).reasonRows,
  ).toBe(0);
});
it("counts host-trap observations while leaving issued, stalled and reasons unavailable", () => {
  const result = samplingTotals([
    { pcSamples: 5, pcIssued: null, pcStalled: null, stallReasons: null },
    { pcSamples: 3 },
    { pcSamples: 0 },
  ] as Instruction[]);
  expect(result.samples).toBe(8);
  expect(result.sampleRows).toBe(2);
  expect(result.issued).toBeNull();
  expect(result.stalled).toBeNull();
  expect(result.reasonRows).toBe(0);
  expect(samplingTotals([{ pcSamples: 0 } as Instruction]).samples).toBeNull();
  expect(
    samplingTotals([{ pcSamples: 0, pcIssued: 0 } as Instruction]).samples,
  ).toBe(0);
  expect(
    samplingTotals([
      { pcSamples: Number.MAX_SAFE_INTEGER },
      { pcSamples: 1 },
    ] as Instruction[]).samples,
  ).toBeNull();
});
